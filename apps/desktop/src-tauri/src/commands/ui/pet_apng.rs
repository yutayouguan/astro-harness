//! v3 APNG packages: explicit actions, exact embedded timing, bounded decoding.
use image::{AnimationDecoder, ImageDecoder};
use std::io::Cursor;

pub(super) fn validate(
    bytes: &[u8],
    clip: &types::pet_motion::PetMotionClip,
) -> Result<(), String> {
    clip.validate().map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024
        || clip.columns != 1
        || clip.frame_width != 192
        || clip.frame_height != 208
        || clip.neutral_bookends
        || clip.loop_start != 0
        || clip.loop_end != clip.durations_ms.len()
        || clip.loop_repeats != 1
    {
        return Err("APNG需要192×208独立帧；时序与循环必须烘焙在文件中".into());
    }
    let mut decoder =
        image::codecs::png::PngDecoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(192);
    limits.max_image_height = Some(208);
    limits.max_alloc = Some(16 * 1024 * 1024);
    decoder.set_limits(limits).map_err(|e| e.to_string())?;
    if decoder.dimensions() != (192, 208) || !decoder.is_apng().map_err(|e| e.to_string())? {
        return Err("需要真正的192×208 APNG，不能将静态PNG改后缀".into());
    }
    let mut count = 0;
    for frame in decoder.apng().map_err(|e| e.to_string())?.into_frames() {
        if count >= clip.durations_ms.len() {
            return Err("APNG帧数超出清单".into());
        }
        let frame = frame.map_err(|e| e.to_string())?;
        let (num, den) = frame.delay().numer_denom_ms();
        if (f64::from(num) / f64::from(den) - f64::from(clip.durations_ms[count])).abs() > 1.0 {
            return Err("APNG帧时长与清单不匹配".into());
        }
        let pixels = frame.buffer();
        if !pixels.pixels().any(|p| p[3] == 0) || !pixels.pixels().any(|p| p[3] > 0) {
            return Err("APNG帧为空或缺少透明背景".into());
        }
        count += 1;
    }
    if count < 2 || count != clip.durations_ms.len() {
        return Err("APNG帧数与清单不匹配".into());
    }
    Ok(())
}

pub(super) fn validate_manifest(manifest: &types::DesktopPetManifest) -> Result<(), String> {
    if manifest.grooming_spritesheet_path.is_some() {
        return Err("v3包不使用旧舔爪图条".into());
    }
    for name in [
        "idle",
        "running-right",
        "running-left",
        "waving",
        "jumping",
        "failed",
        "waiting",
        "running",
        "review",
        "look",
    ] {
        let clip = manifest
            .motion_clips
            .get(name)
            .ok_or_else(|| format!("缺少APNG动作：{name}"))?;
        if name == "look" && clip.durations_ms.len() != 16 {
            return Err("注视APNG需要16个方向".into());
        }
    }
    if manifest.spritesheet_path != manifest.motion_clips["idle"].path {
        return Err("v3主形象必须指向idle.apng动作".into());
    }
    if manifest
        .motion_clips
        .values()
        .any(|clip| !clip.path.ends_with(".apng"))
    {
        return Err("v3动作必须全部使用APNG".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_real_frames_and_rejects_timing_or_static_spoofs() {
        let clips: types::pet_motion::PetMotionClips = serde_json::from_str(include_str!(
            "../../../../src/assets/pets/naitang/apng/motion-clips.json"
        ))
        .unwrap();
        let bytes = include_bytes!("../../../../src/assets/pets/naitang/apng/kneading.apng");
        let mut clip = clips["kneading"].clone();
        validate(bytes, &clip).unwrap();
        clip.durations_ms[1] += 10;
        assert!(validate(bytes, &clip).is_err());
        assert!(validate(&bytes[..bytes.len() / 2], &clips["kneading"]).is_err());
        let image = image::RgbaImage::new(192, 208);
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        assert!(validate(&png.into_inner(), &clips["kneading"]).is_err());
    }
}
