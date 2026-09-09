//! Fresh-process acceptance for domain configuration and its separate state.
//! No server, provider request, MCP process or Hook command is started.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use hooks::{CommandHookRunner, HookTrustStatus};

const TOKEN_ENV: &str = "ASTRO_CONFIG_RESTART_MCP_TOKEN";
const ORIGINAL_TOKEN: &str = "fixture-only-original-token";

fn run_phase(root: &Path, phase: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "extension_config_restart_worker",
            "--ignored",
            "--nocapture",
        ])
        .env("ASTRO_CONFIG_RESTART_ROOT", root)
        .env("ASTRO_CONFIG_RESTART_PHASE", phase)
        .env(
            TOKEN_ENV,
            if phase == "changed" {
                "fixture-only-rotated-token"
            } else {
                ORIGINAL_TOKEN
            },
        )
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{phase} worker timed out (possible configuration lock deadlock)");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{phase} worker failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(&format!("astro-config-restart-verified:{phase}")),
        "{phase} child exited without executing the expected worker"
    );
}

fn definition_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    [
        "home/config.toml",
        "home/mcp/servers.toml",
        "home/hooks/rules.toml",
        "project/.astro/config.toml",
        "project/.astro/mcp.toml",
        "project/.astro/hooks.toml",
    ]
    .into_iter()
    .map(|path| {
        let path = root.join(path);
        let bytes = fs::read(&path).unwrap();
        (path, bytes)
    })
    .collect()
}

#[test]
fn extension_sources_and_separate_state_survive_process_restarts() {
    let scratch = tempfile::Builder::new()
        .prefix("astro-config-restart-")
        .tempdir()
        .unwrap();
    let root = scratch.path().canonicalize().unwrap();
    let home = root.join("home");
    let project = root.join("project");
    for directory in [
        home.join("mcp"),
        home.join("hooks"),
        project.join(".git"),
        project.join(".astro"),
    ] {
        fs::create_dir_all(directory).unwrap();
    }
    fs::write(home::config_path(&home), format!(
        "# global entry stays intact\n[config_sources]\nmcp=['mcp/servers.toml']\nhooks=['hooks/rules.toml']\n[projects.{:?}]\ntrust_level='trusted'\n",
        project.to_string_lossy())).unwrap();
    fs::write(home.join("mcp/servers.toml"), format!(
        "# retained source comment\n[mcp_servers.global_only]\ncommand='fixture-not-executed'\nenv_vars=[{TOKEN_ENV:?}]\n[mcp_servers.shared]\ncommand='global-not-executed'\nenv={{TOKEN='fixture-global-secret'}}\n")).unwrap();
    fs::write(
        home.join("hooks/rules.toml"),
        hook_rule("global-rule", "global-not-executed"),
    )
    .unwrap();
    fs::write(
        project.join(".astro/config.toml"),
        "[config_sources]\nmcp=['mcp.toml']\nhooks=['hooks.toml']\n",
    )
    .unwrap();
    fs::write(
        project.join(".astro/mcp.toml"),
        "[mcp_servers.shared]\ncommand='project-not-executed'\n",
    )
    .unwrap();
    fs::write(
        project.join(".astro/hooks.toml"),
        hook_rule("project-rule", "project-not-executed"),
    )
    .unwrap();
    let original = definition_snapshot(&root);

    run_phase(&root, "prepare");
    let saved = definition_snapshot(&root);
    for (path, bytes) in &original {
        if path != &home.join("mcp/servers.toml") {
            assert_eq!(&saved[path], bytes);
        }
    }
    let mcp_definition = fs::read_to_string(home.join("mcp/servers.toml")).unwrap();
    assert!(mcp_definition.contains("# retained source comment"));
    assert!(!mcp_definition.contains("discovered"));
    let trust = fs::read(home::hook_trust_path(&home)).unwrap();

    run_phase(&root, "restart");
    assert_eq!(definition_snapshot(&root), saved);
    assert_eq!(fs::read(home::hook_trust_path(&home)).unwrap(), trust);

    // Delete only cache envelopes inside this private fixture, never definitions or trust.
    let mut removed = 0;
    for file in fs::read_dir(home::mcp_cache_dir(&home)).unwrap() {
        let file = file.unwrap();
        if file.file_type().unwrap().is_file()
            && file.path().extension().is_some_and(|ext| ext == "json")
        {
            let bytes = fs::read(file.path()).unwrap();
            let contents = String::from_utf8(bytes).unwrap();
            assert!(!contents.contains(ORIGINAL_TOKEN));
            assert!(!contents.contains("fixture-global-secret"));
            fs::remove_file(file.path()).unwrap();
            removed += 1;
        }
    }
    assert_eq!(removed, 2);
    run_phase(&root, "cache-cleared");
    assert_eq!(definition_snapshot(&root), saved);
    assert_eq!(fs::read(home::hook_trust_path(&home)).unwrap(), trust);

    run_phase(&root, "prepare");
    fs::write(
        project.join(".astro/mcp.toml"),
        "[mcp_servers.shared]\ncommand='project-changed-not-executed'\n",
    )
    .unwrap();
    fs::write(
        home.join("hooks/rules.toml"),
        hook_rule("global-rule", "changed-not-executed"),
    )
    .unwrap();
    let changed = definition_snapshot(&root);
    let trust = fs::read(home::hook_trust_path(&home)).unwrap();
    run_phase(&root, "changed");
    assert_eq!(definition_snapshot(&root), changed);
    assert_eq!(fs::read(home::hook_trust_path(&home)).unwrap(), trust);

    fs::remove_file(home.join("mcp/servers.toml")).unwrap();
    run_phase(&root, "missing");
    assert!(!home.join("mcp/servers.toml").exists());
    assert_eq!(fs::read(home::hook_trust_path(&home)).unwrap(), trust);
}

fn hook_rule(id: &str, command: &str) -> String {
    format!("[[hooks.PreToolUse]]\nid={id:?}\n[[hooks.PreToolUse.hooks]]\ntype='command'\ncommand={command:?}\n")
}

fn discovered(name: &str) -> Vec<mcp::DiscoveredTool> {
    vec![mcp::DiscoveredTool {
        name: name.into(),
        description: "fixture".into(),
        annotations: Default::default(),
    }]
}

/// Invoked only by the parent above, in a fresh process with an isolated root.
#[test]
#[ignore = "private worker for extension_sources_and_separate_state_survive_process_restarts"]
fn extension_config_restart_worker() {
    let root = PathBuf::from(
        std::env::var_os("ASTRO_CONFIG_RESTART_ROOT").expect("run parent restart test"),
    )
    .canonicalize()
    .unwrap();
    assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("astro-config-restart-"));
    let home = root.join("home");
    let project = root.join("project");
    let _env = home::test_env::AstroMemoryDirGuard::set(&home);
    let phase = std::env::var("ASTRO_CONFIG_RESTART_PHASE").unwrap();
    if phase == "missing" {
        assert!(mcp::load_mcp_servers_scoped("global", None).is_err());
        assert!(mcp::load_mcp_servers_layered(Some(&project)).is_err());
        assert!(CommandHookRunner::load_for_project(&home, &project).is_err());
        println!("astro-config-restart-verified:{phase}");
        return;
    }

    let mut global = mcp::load_mcp_servers_scoped("global", None).unwrap();
    let effective = mcp::load_mcp_servers_layered(Some(&project)).unwrap();
    let shared = effective
        .iter()
        .find(|server| server.id == "shared")
        .unwrap();
    assert_eq!(
        shared.command,
        if phase == "changed" {
            "project-changed-not-executed"
        } else {
            "project-not-executed"
        }
    );
    assert!(
        !shared.env.contains_key("TOKEN"),
        "project definition inherited global credentials"
    );
    let runner = CommandHookRunner::load_for_project(&home, &project).unwrap();
    let rules = runner.list();
    assert_eq!(rules.len(), 2);
    if phase == "prepare" {
        let server = global
            .iter_mut()
            .find(|server| server.id == "global_only")
            .unwrap();
        server
            .tools
            .insert("write".into(), mcp::McpToolConfig::Enabled(false));
        mcp::save_mcp_servers_scoped("global", None, &global).unwrap();
        let server = global
            .into_iter()
            .find(|server| server.id == "global_only")
            .unwrap();
        mcp::config::persist_discovered(&[(server, discovered("global-tool"))]).unwrap();
        mcp::config::persist_discovered_layered(
            Some(&project),
            &[(shared.clone(), discovered("project-tool"))],
        )
        .unwrap();
        for rule in &rules {
            hooks::trust::record(&home, &rule.key, &rule.current_hash).unwrap();
        }
        println!("astro-config-restart-verified:{phase}");
        return;
    }

    let server = global
        .iter()
        .find(|server| server.id == "global_only")
        .unwrap();
    assert!(!server.is_tool_enabled("write"));
    assert_eq!(
        mcp::source_paths("global", None).unwrap()["global_only"],
        home.join("mcp/servers.toml").to_string_lossy()
    );
    if phase == "restart" {
        assert_eq!(server.discovered[0].name, "global-tool");
        assert_eq!(shared.discovered[0].name, "project-tool");
    } else {
        assert!(matches!(phase.as_str(), "cache-cleared" | "changed"));
        assert!(server.discovered.is_empty());
        assert!(shared.discovered.is_empty());
    }
    assert!(rules
        .iter()
        .any(|rule| rule.key.ends_with(":rule:global-rule:0")));
    assert!(rules
        .iter()
        .any(|rule| rule.key.ends_with(":rule:project-rule:0")));
    for rule in rules {
        let global_rule = rule.key.ends_with(":rule:global-rule:0");
        let changed = phase == "changed" && global_rule;
        assert_eq!(
            rule.command.as_deref(),
            Some(if changed {
                "changed-not-executed"
            } else if global_rule {
                "global-not-executed"
            } else {
                "project-not-executed"
            })
        );
        assert_eq!(
            rule.trust_status,
            if changed {
                HookTrustStatus::Modified
            } else {
                HookTrustStatus::Trusted
            }
        );
        assert_eq!(rule.enabled, !changed);
    }
    assert_eq!(hooks::trust::load(&home).unwrap().len(), 2);
    println!("astro-config-restart-verified:{phase}");
}
