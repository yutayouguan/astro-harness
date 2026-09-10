//! Astro motion clips: independent grids with explicit entry/loop/exit timing.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PetMotionClip {
    pub path: String,
    pub frame_width: u32,
    pub frame_height: u32,
    pub columns: u32,
    pub durations_ms: Vec<u32>,
    pub loop_start: usize,
    pub loop_end: usize,
    pub loop_repeats: u32,
    /// Explicit opt-in: use the main atlas's neutral pose at entry/exit.
    #[serde(default)]
    pub neutral_bookends: bool,
}

impl PetMotionClip {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.path.trim().is_empty() && self.path.len() <= 4096,
            "Invalid motion asset path"
        );
        anyhow::ensure!(
            (32..=512).contains(&self.frame_width) && (32..=512).contains(&self.frame_height),
            "Motion cell dimensions out of bounds"
        );
        anyhow::ensure!(
            (1..=16).contains(&self.columns) && (1..=128).contains(&self.durations_ms.len()),
            "Motion grid dimensions out of bounds"
        );
        anyhow::ensure!(
            self.durations_ms
                .iter()
                .all(|duration| (20..=2000).contains(duration)),
            "Motion frame duration out of bounds"
        );
        anyhow::ensure!(
            self.loop_start < self.loop_end
                && self.loop_end <= self.durations_ms.len()
                && (1..=8).contains(&self.loop_repeats),
            "Invalid motion loop"
        );
        let (width, height) = self.dimensions();
        anyhow::ensure!(
            width <= 4096 && height <= 4096 && self.duration_ms() <= 60_000,
            "Motion clip exceeds limits"
        );
        Ok(())
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (
            self.columns * self.frame_width,
            (self.durations_ms.len() as u32).div_ceil(self.columns.max(1)) * self.frame_height,
        )
    }

    pub fn duration_ms(&self) -> u64 {
        let total: u64 = self
            .durations_ms
            .iter()
            .map(|value| u64::from(*value))
            .sum();
        let loop_time: u64 = self
            .durations_ms
            .get(self.loop_start..self.loop_end)
            .unwrap_or_default()
            .iter()
            .map(|value| u64::from(*value))
            .sum();
        total + loop_time * u64::from(self.loop_repeats.saturating_sub(1))
    }
}

pub type PetMotionClips = BTreeMap<String, PetMotionClip>;

pub fn validate_motion_clips(clips: &PetMotionClips) -> anyhow::Result<()> {
    anyhow::ensure!(clips.len() <= 12, "Too many motion clips");
    let mut pixels = 0_u64;
    for (name, clip) in clips {
        anyhow::ensure!(
            !name.is_empty()
                && name.len() <= 32
                && name.bytes().all(|ch| ch.is_ascii_lowercase() || ch == b'-'),
            "Invalid motion clip name"
        );
        clip.validate()?;
        let (width, height) = clip.dimensions();
        pixels += u64::from(width) * u64::from(height);
        anyhow::ensure!(
            pixels <= 16 * 1024 * 1024,
            "Motion clips exceed decoded-memory budget"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pet_motion_supports_more_than_eight_frames_with_entry_and_exit() {
        let mut clip = PetMotionClip {
            path: "kneading.webp".into(),
            frame_width: 192,
            frame_height: 208,
            columns: 4,
            durations_ms: vec![90; 16],
            loop_start: 4,
            loop_end: 12,
            loop_repeats: 3,
            neutral_bookends: false,
        };
        clip.validate().unwrap();
        assert_eq!(clip.dimensions(), (768, 832));
        assert_eq!(clip.duration_ms(), 2880);
        clip.loop_end = 17;
        assert!(clip.validate().is_err());
        clip.loop_end = 12;
        clip.columns = u32::MAX;
        assert!(clip.validate().is_err());
    }

    #[test]
    fn pet_motion_bounds_the_combined_preload_cost() {
        let clip = PetMotionClip {
            path: "large.webp".into(),
            frame_width: 512,
            frame_height: 512,
            columns: 8,
            durations_ms: vec![20; 64],
            loop_start: 0,
            loop_end: 1,
            loop_repeats: 1,
            neutral_bookends: false,
        };
        clip.validate().unwrap();
        let clips = [("kneading".into(), clip.clone()), ("grooming".into(), clip)].into();
        assert!(validate_motion_clips(&clips).is_err());
    }
}
