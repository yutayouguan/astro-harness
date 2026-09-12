//! Native UI adapters for shared scene storage. Generation never replaces the active scene.
use super::desktop_pet::{present_committed_state, DesktopPetStateDto};
use super::pet_generation::PetGeneration;
use std::{io::Cursor, path::Path};
use tauri::AppHandle;
use types::pet_scene::{PetIdentity, PetScene, SceneApplyMode};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PetScenePreview {
    #[serde(flatten)]
    scene: PetScene,
    wallpaper_path: Option<String>,
    in_use: bool,
    favorite: bool,
}

#[tauri::command]
pub async fn apply_library_pet(
    app: AppHandle,
    pet_id: String,
) -> Result<DesktopPetStateDto, String> {
    let state = types::pet_library::apply_pet(&home::default_memory_dir(), &pet_id)
        .map_err(|e| e.to_string())?;
    present_committed_state(&app, state)
}

#[tauri::command]
pub async fn edit_pet_library(
    app: AppHandle,
    request: types::pet_library::PetLibraryEdit,
) -> Result<DesktopPetStateDto, String> {
    let state = types::pet_library::edit_library(&home::default_memory_dir(), request)
        .map_err(|e| e.to_string())?;
    present_committed_state(&app, state)
}

#[tauri::command]
pub fn get_pet_scenes() -> Result<Vec<PetScenePreview>, String> {
    let base = home::default_memory_dir();
    let state = types::read_desktop_pet_state(&base).map_err(|e| e.to_string())?;
    Ok(state
        .scenes
        .iter()
        .cloned()
        .rev()
        .map(|scene| PetScenePreview {
            in_use: state.active_scene_id.as_ref() == Some(&scene.id),
            favorite: state.favorite_scene_ids.contains(&scene.id),
            wallpaper_path: scene
                .wallpaper_path(&base)
                .map(|p| p.to_string_lossy().into_owned()),
            scene,
        })
        .collect())
}

#[tauri::command]
pub async fn create_pet_scene(
    app: AppHandle,
    name: String,
    description: Option<String>,
    use_current: bool,
    request_id: String,
    pet_name: Option<String>,
) -> Result<DesktopPetStateDto, String> {
    let base = home::default_memory_dir();
    let state = types::read_desktop_pet_state(&base).map_err(|e| e.to_string())?;
    let mut generation = PetGeneration::start(&request_id)?;
    // Validate before a billable request.
    if name.trim().is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err("场景名称应为 1–80 个字符".into());
    }
    if state.scenes.len() >= 100 {
        return Err("场景收藏已达 100 个上限".into());
    }
    if pet_name.as_ref().is_some_and(|name| {
        name.trim().is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control)
    }) {
        return Err("宠物名称应为 1–80 个字符".into());
    }
    let mut pet = if use_current {
        PetIdentity::from_state(&state).map_err(|e| e.to_string())?
    } else {
        let source = state.source_path.as_deref().ok_or("请先上传宠物照片")?;
        generation
            .run(super::desktop_pet::generate_pet_identity(
                &base,
                source,
                description.as_deref().unwrap_or_default(),
            ))
            .await?
    };
    generation.check()?;
    if !use_current {
        pet.display_name = pet_name.map(|s| s.trim().to_string());
    }
    let scene = PetScene {
        id: format!("pet-{}", uuid::Uuid::new_v4().simple()),
        name: name.trim().into(),
        pet,
        style: None,
        wallpaper_source_path: None,
        preferences: None,
    };
    let state = types::pet_scene::save_scene(&base, scene).map_err(|e| e.to_string())?;
    present_committed_state(&app, state)
}

fn find_scene(base: &Path, id: &str) -> Result<PetScene, String> {
    types::read_desktop_pet_state(base)
        .map_err(|e| e.to_string())?
        .scenes
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| "场景不存在".into())
}

#[tauri::command]
pub async fn generate_pet_scene_wallpaper(
    app: AppHandle,
    scene_id: String,
    description: String,
    include_pet: bool,
    request_id: String,
) -> Result<DesktopPetStateDto, String> {
    let base = home::default_memory_dir();
    let scene = find_scene(&base, &scene_id)?;
    let mut generation = PetGeneration::start(&request_id)?;
    if description.chars().count() > 2000 {
        return Err("场景描述不能超过 2000 个字符".into());
    }
    let path = types::pet_scene::managed_file(&base, Path::new(&scene.pet.pet_path))
        .map_err(|e| e.to_string())?;
    let mut bytes = types::desktop_pet::read_limited_pet_file(&path, 50 * 1024 * 1024)
        .map_err(|e| e.to_string())?;
    // Reference one canonical idle frame, never send the entire animation grid as a character.
    if matches!(scene.pet.sprite_version_number, Some(2 | 3)) {
        let mut reader = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(types::DESKTOP_PET_V2_WIDTH);
        limits.max_image_height = Some(types::DESKTOP_PET_V2_HEIGHT);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let frame = reader.decode().map_err(|e| e.to_string())?.crop_imm(
            0,
            0,
            types::DESKTOP_PET_V2_CELL_WIDTH,
            types::DESKTOP_PET_V2_CELL_HEIGHT,
        );
        let mut png = Cursor::new(Vec::new());
        frame
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        bytes = png.into_inner();
    }
    let mime =
        if image::guess_format(&bytes).map_err(|e| e.to_string())? == image::ImageFormat::WebP {
            "image/webp"
        } else {
            "image/png"
        };
    let prompt = wallpaper_prompt(&description, include_pet);
    let generated = generation
        .run(crate::commands::chat::generate_image_data_with_reference(
            &prompt,
            1536,
            1024,
            Some((&bytes, mime, "pet-reference.png")),
        ))
        .await?;
    generation.check()?;
    // Compare against the captured scene so late generation cannot replace a newer binding.
    let state = tools::builtin::pet_scene::attach_wallpaper(&base, &scene, &generated.data, None)
        .map_err(|e| e.to_string())?;
    present_committed_state(&app, state)
}

fn wallpaper_prompt(description: &str, include_pet: bool) -> String {
    format!("Create a polished landscape wallpaper for an app background, matching the reference pet's art style and palette. Scene: {}. {} Keep the center quiet for readable UI and leave open space near the lower right for a separate floating desktop pet. No text, logo, watermark, collage, or sprite grid.", description.trim(), if include_pet { "Include exactly one portrait of the reference pet, preserving its identity and markings." } else { "Environment only: do NOT depict any pet, animal, character, or duplicate of the reference. Use the reference only for style and colors." })
}

#[tauri::command]
pub async fn edit_pet_scene(
    app: AppHandle,
    request: types::pet_scene::PetSceneEdit,
) -> Result<DesktopPetStateDto, String> {
    let state = types::pet_scene::edit_scene(&home::default_memory_dir(), request)
        .map_err(|e| e.to_string())?;
    present_committed_state(&app, state)
}

#[tauri::command]
pub async fn export_pet_scene(scene_id: String, destination: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        types::pet_scene::export_scene(
            &home::default_memory_dir(),
            &scene_id,
            Path::new(&destination),
        )
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn bind_pet_scene_wallpaper(
    app: AppHandle,
    scene_id: String,
    source_path: String,
) -> Result<DesktopPetStateDto, String> {
    let base = home::default_memory_dir();
    let scene = find_scene(&base, &scene_id)?;
    let path = Path::new(&source_path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let bytes = types::desktop_pet::read_limited_pet_file(&path, 25 * 1024 * 1024)
        .map_err(|e| e.to_string())?;
    let state = tools::builtin::pet_scene::attach_wallpaper(
        &base,
        &scene,
        &bytes,
        Some(path.to_string_lossy().into_owned()),
    )
    .map_err(|e| e.to_string())?;
    present_committed_state(&app, state)
}

#[tauri::command]
pub async fn apply_pet_scene(
    app: AppHandle,
    scene_id: String,
    mode: SceneApplyMode,
) -> Result<DesktopPetStateDto, String> {
    let state = types::pet_scene::apply_scene(&home::default_memory_dir(), &scene_id, mode)
        .map_err(|e| e.to_string())?;
    present_committed_state(&app, state)
}

#[tauri::command]
pub async fn set_pet_scene_follow_wallpaper(
    app: AppHandle,
    enabled: bool,
) -> Result<DesktopPetStateDto, String> {
    let state = types::update_desktop_pet_state(&home::default_memory_dir(), |state| {
        state.follow_wallpaper = enabled;
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    present_committed_state(&app, state)
}

#[tauri::command]
pub async fn sync_pet_scene_wallpaper(
    app: AppHandle,
    path: Option<String>,
) -> Result<DesktopPetStateDto, String> {
    let base = home::default_memory_dir();
    // Resolve active-theme priority inside the shared coordinator, not from a stale snapshot.
    let state =
        types::pet_scene::sync_wallpaper(&base, path.as_deref()).map_err(|e| e.to_string())?;
    present_committed_state(&app, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pet_scene_wallpaper_environment_and_portrait_are_distinct() {
        assert!(wallpaper_prompt("forest", false).contains("do NOT depict"));
        assert!(wallpaper_prompt("forest", true).contains("exactly one portrait"));
    }
}
