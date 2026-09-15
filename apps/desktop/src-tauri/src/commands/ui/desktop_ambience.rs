//! Native transaction adapter. Undo tokens are server-owned and bounded to one step.
use super::desktop_pet::{present_committed_state, DesktopPetStateDto};
use std::sync::Mutex;
use tauri::AppHandle;
use types::desktop_ambience::{AmbienceChange, AmbienceUndo};

static UNDO: Mutex<Option<(String, AmbienceUndo)>> = Mutex::new(None);

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbienceResult {
    state: DesktopPetStateDto,
    undo_token: String,
}

#[tauri::command]
pub async fn apply_desktop_ambience(
    app: AppHandle,
    change: AmbienceChange,
) -> Result<AmbienceResult, String> {
    let mut undo = UNDO.lock().map_err(|e| e.to_string())?;
    let (state, record) = types::desktop_ambience::change(&home::default_memory_dir(), change)
        .map_err(|e| e.to_string())?;
    let token = uuid::Uuid::new_v4().to_string();
    *undo = Some((token.clone(), record));
    Ok(AmbienceResult {
        state: present_committed_state(&app, state)?,
        undo_token: token,
    })
}

#[tauri::command]
pub async fn undo_desktop_ambience(
    app: AppHandle,
    token: String,
) -> Result<DesktopPetStateDto, String> {
    let mut undo = UNDO.lock().map_err(|e| e.to_string())?;
    let (saved_token, record) = undo.as_ref().ok_or("此撤销记录已失效")?;
    if saved_token != &token {
        return Err("此撤销记录已失效".into());
    }
    let state = types::desktop_ambience::undo(&home::default_memory_dir(), record)
        .map_err(|e| e.to_string())?;
    *undo = None;
    present_committed_state(&app, state)
}

#[tauri::command]
pub async fn save_desktop_ambience_scene(
    app: AppHandle,
    name: String,
    pet_id: String,
    wallpaper: types::UiStyleWallpaper,
    tokens: types::UiStyleTokens,
) -> Result<DesktopPetStateDto, String> {
    let base = home::default_memory_dir();
    present_committed_state(&app, save_scene_at(&base, name, pet_id, wallpaper, tokens)?)
}

fn save_scene_at(
    base: &std::path::Path,
    name: String,
    pet_id: String,
    wallpaper: types::UiStyleWallpaper,
    tokens: types::UiStyleTokens,
) -> Result<types::DesktopPetState, String> {
    let source = types::pet_scene::managed_file(base, std::path::Path::new(&wallpaper.path))
        .map_err(|e| e.to_string())?;
    super::wallpaper::analyze_wallpaper_at(base, &source)?;
    let bytes = types::desktop_pet::read_limited_pet_file(&source, 25 * 1024 * 1024)
        .map_err(|e| e.to_string())?;
    let id = format!("pet-{}", uuid::Uuid::new_v4().simple());
    let extension = source.extension().and_then(|s| s.to_str()).unwrap_or("png");
    let relative = format!("themes/{id}/wallpaper.{extension}");
    let target = types::ui_style_root(base).join(&relative);
    let style = types::UiStyleManifest {
        schema_version: types::UI_STYLE_SCHEMA_VERSION,
        id: id.clone(),
        name: name.clone(),
        revision: uuid::Uuid::new_v4().to_string(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        tokens,
        icons: Default::default(),
        wallpaper: Some(types::UiStyleWallpaper {
            path: relative,
            ..wallpaper
        }),
    };
    style.validate()?;
    types::pet_scene::atomic_write(&target, &bytes).map_err(|e| e.to_string())?;
    let result = types::update_desktop_pet_state(base, |state| {
        anyhow::ensure!(
            state.active_pet_id.as_deref() == Some(&pet_id),
            "当前宠物已切换，请重试"
        );
        let scene = types::pet_scene::PetScene {
            id,
            name,
            pet: types::pet_scene::PetIdentity::from_state(state)?,
            style: Some(style),
            wallpaper_source_path: Some(source.to_string_lossy().into_owned()),
            preferences: Some(types::pet_preferences::PetScenePreferences::from_state(
                state,
            )),
        };
        scene.validate()?;
        state.scenes.push(scene);
        Ok(())
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&target);
    }
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_as_keeps_original_scene_and_manual_colors() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let source = types::ui_style_root(base).join("reference.png");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        image::RgbaImage::from_pixel(8, 6, image::Rgba([120, 150, 160, 255]))
            .save(&source)
            .unwrap();
        let before = types::update_desktop_pet_state(base, |state| {
            state.pet_path = Some(source.to_string_lossy().into_owned());
            Ok(())
        })
        .unwrap();
        let wallpaper = types::UiStyleWallpaper {
            path: source.to_string_lossy().into_owned(),
            fit: Default::default(),
            shade: 22,
            blur: 1,
            adaptive_color: false,
            recommended_theme: None,
            accent_color: None,
            secondary_color: None,
        };
        let colors = std::collections::BTreeMap::from([
            ("--color-accent".into(), "#123456".into()),
            ("--color-accent-secondary".into(), "#789abc".into()),
        ]);
        let tokens = types::UiStyleTokens {
            light: colors.clone(),
            dark: colors,
        };
        let pet_id = before.active_pet_id.clone().unwrap();
        let first = save_scene_at(
            base,
            "First".into(),
            pet_id.clone(),
            wallpaper.clone(),
            tokens.clone(),
        )
        .unwrap();
        let second = save_scene_at(
            base,
            "Second".into(),
            pet_id.clone(),
            wallpaper.clone(),
            tokens.clone(),
        )
        .unwrap();
        assert_eq!(first.scenes[0], second.scenes[0]);
        assert_eq!(second.scenes.len(), 2);
        assert_eq!(second.active_pet_id, before.active_pet_id);
        assert_eq!(second.active_scene_id, before.active_scene_id);
        assert_eq!(second.scenes[1].style.as_ref().unwrap().tokens, tokens);
        assert!(
            !second.scenes[1]
                .style
                .as_ref()
                .unwrap()
                .wallpaper
                .as_ref()
                .unwrap()
                .adaptive_color
        );
        assert!(save_scene_at(base, " ".into(), pet_id, wallpaper, tokens).is_err());
        assert_eq!(types::read_desktop_pet_state(base).unwrap().scenes.len(), 2);
        assert!(types::read_active_ui_style(base).unwrap().is_none());
    }
}
