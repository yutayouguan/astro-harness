//! Global toolset loading preferences, independent of Agent enable gates.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolLoadingMode {
    #[default]
    Auto,
    Always,
    OnDemand,
}

pub fn load_tool_loading_modes() -> anyhow::Result<HashMap<String, ToolLoadingMode>> {
    Ok(
        crate::settings::read(&crate::default_memory_dir(), &["desktop", "tool_loading"])?
            .unwrap_or_default(),
    )
}

/// Patch one key under the shared TOML lock; Auto restores the bundled default.
pub fn set_tool_loading_mode(toolset: &str, mode: ToolLoadingMode) -> anyhow::Result<()> {
    anyhow::ensure!(
        crate::KNOWN_TOOLSET_IDS.contains(&toolset),
        "unknown toolset: {toolset}"
    );
    crate::settings::update(&crate::default_memory_dir(), |doc| {
        let mut modes: HashMap<String, ToolLoadingMode> =
            crate::settings::get(doc, &["desktop", "tool_loading"])?.unwrap_or_default();
        if mode == ToolLoadingMode::Auto {
            modes.remove(toolset);
        } else {
            modes.insert(toolset.into(), mode);
        }
        crate::settings::put(doc, &["desktop", "tool_loading"], &modes)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_preferences_patch_only_their_section_and_auto_restores_default() {
        let dir = tempfile::TempDir::new().unwrap();
        let _env = crate::test_env::AstroMemoryDirGuard::set(dir.path());
        assert!(load_tool_loading_modes().unwrap().is_empty());
        assert!(!crate::settings::path(dir.path()).exists());
        crate::settings::write(
            dir.path(),
            &["desktop", "tools"],
            &HashMap::from([("browser", false)]),
        )
        .unwrap();
        set_tool_loading_mode("browser", ToolLoadingMode::Always).unwrap();
        set_tool_loading_mode("memory", ToolLoadingMode::OnDemand).unwrap();
        let modes = load_tool_loading_modes().unwrap();
        assert_eq!(modes["browser"], ToolLoadingMode::Always);
        assert_eq!(modes["memory"], ToolLoadingMode::OnDemand);
        assert_eq!(crate::load_tools_enabled().unwrap()["browser"], false);
        set_tool_loading_mode("browser", ToolLoadingMode::Auto).unwrap();
        let modes = load_tool_loading_modes().unwrap();
        assert!(!modes.contains_key("browser"));
        assert_eq!(modes["memory"], ToolLoadingMode::OnDemand);
        assert!(set_tool_loading_mode("system", ToolLoadingMode::OnDemand).is_err());
    }
}
