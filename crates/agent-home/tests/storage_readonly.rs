use std::fs;
use std::process::Command;

#[test]
fn doctor_process_never_bootstraps_or_repairs_the_selected_home() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing");
    let inspect = |root: &std::path::Path| {
        let output = Command::new(env!("CARGO_BIN_EXE_astro-doctor"))
            .arg("--root")
            .arg(root)
            .output()
            .unwrap();
        assert!(output.status.success());
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    assert_eq!(inspect(&missing)["state"], "new_install");
    assert!(!missing.exists());

    let root = temp.path().join("existing");
    fs::create_dir(&root).unwrap();
    let config = root.join("config.toml");
    let bytes = b"secret = 'DO_NOT_ECHO_THIS_INVALID_CONFIG";
    fs::write(&config, bytes).unwrap();
    let modified = config.metadata().unwrap().modified().unwrap();
    let report = inspect(&root);
    assert_eq!(report["state"], "invalid_config");
    assert!(!report.to_string().contains("DO_NOT_ECHO"));
    assert_eq!(fs::read(&config).unwrap(), bytes);
    assert_eq!(config.metadata().unwrap().modified().unwrap(), modified);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}
