//! 设置页使用的本机 CLI 依赖探测与白名单安装入口。

use serde::Serialize;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_OUTPUT_CHARS: usize = 12_000;

#[derive(Clone, Copy)]
struct DependencySpec {
    id: &'static str,
    name: &'static str,
    binary: &'static str,
}

const DEPENDENCIES: &[DependencySpec] = &[
    DependencySpec {
        id: "uv",
        name: "uv",
        binary: "uv",
    },
    DependencySpec {
        id: "rtk",
        name: "RTK",
        binary: "rtk",
    },
    DependencySpec {
        id: "fd",
        name: "fd",
        binary: "fd",
    },
    DependencySpec {
        id: "ripgrep",
        name: "ripgrep",
        binary: "rg",
    },
    DependencySpec {
        id: "bun",
        name: "Bun",
        binary: "bun",
    },
    DependencySpec {
        id: "lark-cli",
        name: "Lark CLI",
        binary: "lark-cli",
    },
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDependencyDto {
    pub id: String,
    pub name: String,
    pub binary: String,
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub install_path: String,
    pub install_command: Option<String>,
    pub can_install: bool,
    pub install_unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallEnvironmentDependencyResult {
    pub dependency: EnvironmentDependencyDto,
    pub output: String,
}

struct InstallRecipe {
    display: String,
    program: OsString,
    args: Vec<OsString>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();

    if let Some(home) = home_dir() {
        dirs.extend([
            home.join(".local/bin"),
            home.join(".cargo/bin"),
            home.join(".bun/bin"),
            home.join(".npm-global/bin"),
        ]);
    }

    #[cfg(target_os = "macos")]
    dirs.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ]);

    #[cfg(target_os = "linux")]
    dirs.extend([
        PathBuf::from("/home/linuxbrew/.linuxbrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ]);

    dirs.sort();
    dirs.dedup();
    dirs
}

fn executable_filename(binary: &str) -> String {
    if cfg!(windows) && !binary.to_ascii_lowercase().ends_with(".exe") {
        format!("{binary}.exe")
    } else {
        binary.to_string()
    }
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    true
}

fn find_executable_in_dirs(binary: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let filename = executable_filename(binary);
    dirs.iter()
        .map(|dir| dir.join(&filename))
        .find(|path| is_executable(path))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn find_with_login_shell(binary: &str) -> Option<PathBuf> {
    // `binary` 只来自上方静态白名单，不接受前端透传。
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/sh"));
    let command = format!("command -v -- {binary}");
    let child = Command::new(shell)
        .args(["-l", "-c", &command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let output = wait_for_output(child, PROBE_TIMEOUT).ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    is_executable(&path).then_some(path)
}

#[cfg(target_os = "windows")]
fn find_with_login_shell(binary: &str) -> Option<PathBuf> {
    let output = Command::new("where.exe").arg(binary).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .find(|path| is_executable(path))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn find_with_login_shell(_binary: &str) -> Option<PathBuf> {
    None
}

fn find_executable(binary: &str) -> Option<PathBuf> {
    find_executable_in_dirs(binary, &candidate_dirs()).or_else(|| find_with_login_shell(binary))
}

fn wait_for_output(
    mut child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|error| error.to_string()),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "command timed out after {} seconds",
                    timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn first_output_line(path: &Path) -> Option<String> {
    let child = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let output = wait_for_output(child, PROBE_TIMEOUT).ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(160).collect())
}

fn display_home_path(relative: &str) -> String {
    home_dir()
        .map(|home| home.join(relative).to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("~/{relative}"))
}

fn suggested_install_path(spec: DependencySpec) -> String {
    match spec.id {
        "uv" => display_home_path(".local/bin/uv"),
        "bun" => display_home_path(".bun/bin/bun"),
        "lark-cli" => "npm global bin/lark-cli".to_string(),
        _ if cfg!(target_os = "macos") => format!("Homebrew prefix/bin/{}", spec.binary),
        _ => display_home_path(&format!(".local/bin/{}", spec.binary)),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn unix_recipe(spec: DependencySpec) -> Option<InstallRecipe> {
    let command = match spec.id {
        "uv" if find_executable("curl").is_some() => {
            "curl -LsSf https://astral.sh/uv/install.sh | sh".to_string()
        }
        "rtk" if find_executable("brew").is_some() => "brew install rtk".to_string(),
        "rtk" if find_executable("curl").is_some() => "curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/refs/heads/master/install.sh | sh".to_string(),
        "fd" if find_executable("brew").is_some() => "brew install fd".to_string(),
        "fd" if find_executable("cargo").is_some() => "cargo install fd-find".to_string(),
        "ripgrep" if find_executable("brew").is_some() => "brew install ripgrep".to_string(),
        "ripgrep" if find_executable("cargo").is_some() => "cargo install ripgrep".to_string(),
        "bun" if find_executable("curl").is_some() => {
            "curl -fsSL https://bun.sh/install | bash".to_string()
        }
        "lark-cli" if find_executable("npm").is_some() => "npm install -g @larksuite/cli".to_string(),
        _ => return None,
    };
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/sh"));
    Some(InstallRecipe {
        display: command.clone(),
        program: shell,
        args: vec![
            OsString::from("-l"),
            OsString::from("-c"),
            OsString::from(command),
        ],
    })
}

#[cfg(target_os = "windows")]
fn windows_recipe(spec: DependencySpec) -> Option<InstallRecipe> {
    let command = match spec.id {
        "uv" => "irm https://astral.sh/uv/install.ps1 | iex",
        "rtk" if find_executable("cargo").is_some() => "cargo install --git https://github.com/rtk-ai/rtk",
        "fd" if find_executable("winget").is_some() => "winget install --id sharkdp.fd --exact --accept-package-agreements --accept-source-agreements",
        "ripgrep" if find_executable("winget").is_some() => "winget install --id BurntSushi.ripgrep.MSVC --exact --accept-package-agreements --accept-source-agreements",
        "bun" => "irm https://bun.sh/install.ps1 | iex",
        "lark-cli" if find_executable("npm").is_some() => "npm install -g @larksuite/cli",
        _ => return None,
    };
    Some(InstallRecipe {
        display: command.to_string(),
        program: OsString::from("powershell.exe"),
        args: vec![
            OsString::from("-NoProfile"),
            OsString::from("-NonInteractive"),
            OsString::from("-ExecutionPolicy"),
            OsString::from("Bypass"),
            OsString::from("-Command"),
            OsString::from(command),
        ],
    })
}

fn install_recipe(spec: DependencySpec) -> Option<InstallRecipe> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    return unix_recipe(spec);
    #[cfg(target_os = "windows")]
    return windows_recipe(spec);
    #[allow(unreachable_code)]
    None
}

fn status_for(spec: DependencySpec) -> EnvironmentDependencyDto {
    let path = find_executable(spec.binary);
    let recipe = install_recipe(spec);
    EnvironmentDependencyDto {
        id: spec.id.to_string(),
        name: spec.name.to_string(),
        binary: spec.binary.to_string(),
        installed: path.is_some(),
        version: path.as_deref().and_then(first_output_line),
        path: path.map(|value| value.to_string_lossy().into_owned()),
        install_path: suggested_install_path(spec),
        install_command: recipe.as_ref().map(|value| value.display.clone()),
        can_install: recipe.is_some(),
        install_unavailable_reason: recipe.is_none().then(|| {
            if spec.id == "lark-cli" {
                "npm_required".to_string()
            } else {
                "installer_missing".to_string()
            }
        }),
    }
}

#[tauri::command]
pub async fn list_environment_dependencies() -> Result<Vec<EnvironmentDependencyDto>, String> {
    tauri::async_runtime::spawn_blocking(|| DEPENDENCIES.iter().copied().map(status_for).collect())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn install_environment_dependency(
    dependency_id: String,
) -> Result<InstallEnvironmentDependencyResult, String> {
    let (spec, recipe) = tauri::async_runtime::spawn_blocking(move || {
        let spec = DEPENDENCIES
            .iter()
            .copied()
            .find(|spec| spec.id == dependency_id)
            .ok_or_else(|| "unknown environment dependency".to_string())?;
        let recipe = install_recipe(spec).ok_or_else(|| {
            status_for(spec)
                .install_unavailable_reason
                .unwrap_or_default()
        })?;
        Ok::<_, String>((spec, recipe))
    })
    .await
    .map_err(|error| error.to_string())??;

    let mut command = tokio::process::Command::new(&recipe.program);
    command
        .args(&recipe.args)
        .env("CI", "1")
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(INSTALL_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            format!(
                "installer timed out after {} seconds",
                INSTALL_TIMEOUT.as_secs()
            )
        })?
        .map_err(|error| format!("failed to run installer: {error}"))?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let tail: String = combined
        .chars()
        .rev()
        .take(MAX_OUTPUT_CHARS)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if !output.status.success() {
        return Err(format!(
            "installer exited with {}\n{}",
            output.status,
            tail.trim()
        ));
    }
    let dependency = tauri::async_runtime::spawn_blocking(move || status_for(spec))
        .await
        .map_err(|error| error.to_string())?;
    Ok(InstallEnvironmentDependencyResult {
        dependency,
        output: tail.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn dependency_catalog_has_unique_ids_and_binaries() {
        let ids: HashSet<_> = DEPENDENCIES.iter().map(|spec| spec.id).collect();
        let binaries: HashSet<_> = DEPENDENCIES.iter().map(|spec| spec.binary).collect();
        assert_eq!(ids.len(), DEPENDENCIES.len());
        assert_eq!(binaries.len(), DEPENDENCIES.len());
        assert_eq!(
            ids,
            HashSet::from(["uv", "rtk", "fd", "ripgrep", "bun", "lark-cli"])
        );
    }

    #[test]
    fn directory_lookup_returns_an_absolute_existing_candidate() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join(executable_filename("astro-test-bin"));
        std::fs::write(&executable, b"test").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&executable, permissions).unwrap();
        }

        let resolved = find_executable_in_dirs("astro-test-bin", &[temp.path().to_path_buf()]);

        assert_eq!(resolved.as_deref(), Some(executable.as_path()));
        assert!(resolved.unwrap().is_absolute());
    }

    #[test]
    fn every_dependency_exposes_an_install_path() {
        for spec in DEPENDENCIES {
            assert!(
                !suggested_install_path(*spec).trim().is_empty(),
                "{}",
                spec.id
            );
        }
    }
}
