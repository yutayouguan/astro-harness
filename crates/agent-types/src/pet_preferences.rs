//! User-controlled pet placement and low-distraction behavior.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PetPosition {
    pub monitor: Option<String>,
    pub monitor_x: i32,
    pub monitor_y: i32,
    /// Normalized travel range within the monitor's work area, not screen pixels.
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PetPreferences {
    pub position: Option<PetPosition>,
    pub position_locked: bool,
    pub snap_to_edge: bool,
    pub quiet_mode: bool,
    pub hide_in_fullscreen: bool,
    pub presentation_mode: bool,
    pub activity_interval_secs: u32,
}

impl Default for PetPreferences {
    fn default() -> Self {
        Self {
            position: None,
            position_locked: false,
            snap_to_edge: true,
            quiet_mode: false,
            hide_in_fullscreen: true,
            presentation_mode: false,
            activity_interval_secs: 45,
        }
    }
}
impl PetPreferences {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (15..=300).contains(&self.activity_interval_secs),
            "动作间隔应为15–300秒"
        );
        if let Some(p) = &self.position {
            anyhow::ensure!(
                p.x.is_finite()
                    && p.y.is_finite()
                    && (0.0..=1.0).contains(&p.x)
                    && (0.0..=1.0).contains(&p.y),
                "桌宠位置无效"
            );
            anyhow::ensure!(
                p.monitor.as_ref().is_none_or(|name| name.len() <= 512),
                "显示器名称过长"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PetPreferencesPatch {
    pub position_locked: Option<bool>,
    pub snap_to_edge: Option<bool>,
    pub quiet_mode: Option<bool>,
    pub hide_in_fullscreen: Option<bool>,
    pub presentation_mode: Option<bool>,
    pub activity_interval_secs: Option<u32>,
}
impl PetPreferencesPatch {
    pub fn apply(&self, target: &mut PetPreferences) {
        if let Some(v) = self.position_locked {
            target.position_locked = v;
        }
        if let Some(v) = self.snap_to_edge {
            target.snap_to_edge = v;
        }
        if let Some(v) = self.quiet_mode {
            target.quiet_mode = v;
        }
        if let Some(v) = self.hide_in_fullscreen {
            target.hide_in_fullscreen = v;
        }
        if let Some(v) = self.presentation_mode {
            target.presentation_mode = v;
        }
        if let Some(v) = self.activity_interval_secs {
            target.activity_interval_secs = v;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PetScenePreferences {
    pub scale: f64,
    pub behavior: PetPreferences,
}
impl PetScenePreferences {
    pub fn from_state(state: &crate::DesktopPetState) -> Self {
        let mut behavior = state.preferences.clone();
        // Presentation is a temporary global intent, never a scene effect.
        behavior.presentation_mode = false;
        Self {
            scale: state.scale,
            behavior,
        }
    }
    pub fn apply(&self, state: &mut crate::DesktopPetState) {
        let presentation = state.preferences.presentation_mode;
        state.scale = self.scale;
        state.preferences = self.behavior.clone();
        state.preferences.presentation_mode = presentation;
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.scale.is_finite()
                && (crate::desktop_pet::DESKTOP_PET_MIN_SCALE
                    ..=crate::desktop_pet::DESKTOP_PET_MAX_SCALE)
                    .contains(&self.scale),
            "场景桌宠大小无效"
        );
        self.behavior.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pet_preferences_patch_preserves_unspecified_fields() {
        let mut prefs = PetPreferences::default();
        let patch: PetPreferencesPatch = serde_json::from_str(r#"{"quietMode":true}"#).unwrap();
        patch.apply(&mut prefs);
        assert!(prefs.quiet_mode && prefs.snap_to_edge && prefs.hide_in_fullscreen);
        prefs.activity_interval_secs = 0;
        assert!(prefs.validate().is_err());
    }
    #[test]
    fn pet_scene_preferences_do_not_cancel_global_presentation_intent() {
        let mut state = crate::DesktopPetState::default();
        state.preferences.presentation_mode = true;
        let scene = PetScenePreferences::from_state(&state);
        assert!(!scene.behavior.presentation_mode);
        scene.apply(&mut state);
        assert!(state.preferences.presentation_mode);
    }
}
