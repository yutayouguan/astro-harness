use super::pet_placement::{capture, target, Screen};
use types::pet_motion::PetMotionClip;

#[derive(Debug, Clone)]
pub(super) struct WalkPlan {
    pub start: (i32, i32),
    pub travel: f64,
    pub entry_ms: u64,
    pub moving_ms: u64,
    pub duration_ms: u64,
    pub clip: PetMotionClip,
    pub topology: String,
}
impl WalkPlan {
    pub fn point(&self, elapsed_ms: u64) -> (i32, i32) {
        let phase = elapsed_ms.saturating_sub(self.entry_ms).min(self.moving_ms);
        let cycle = self.moving_ms / u64::from(self.clip.loop_repeats);
        let completed = phase / cycle;
        let mut within = phase % cycle;
        let mut sampled = completed * cycle;
        for duration in &self.clip.durations_ms[self.clip.loop_start..self.clip.loop_end] {
            if within < u64::from(*duration) {
                break;
            }
            within -= u64::from(*duration);
            sampled += u64::from(*duration);
        }
        // Root position and APNG pose advance together, avoiding sub-frame foot sliding.
        let fraction = sampled as f64 / self.moving_ms.max(1) as f64;
        (
            (f64::from(self.start.0) + self.travel * fraction).round() as i32,
            self.start.1,
        )
    }
}

pub(super) fn ground(
    point: (i32, i32),
    size: (u32, u32),
    screens: &[Screen],
    logical_size: (f64, f64),
) -> Option<(types::pet_preferences::PetPosition, (i32, i32))> {
    let mut position = capture(point, size, screens, false)?;
    position.y = 1.0;
    let point = target(Some(&position), screens, logical_size)?;
    Some((position, point))
}

pub(super) fn plan(
    point: (i32, i32),
    physical_size: (u32, u32),
    screens: &[Screen],
    logical_size: (f64, f64),
    source: &PetMotionClip,
    right: bool,
    cycles: u32,
) -> Result<WalkPlan, String> {
    validate_walk(source)?;
    let stride = source
        .locomotion
        .as_ref()
        .ok_or("宠物缺少已验收的步幅数据")?
        .stride_px;
    if !(2..=4).contains(&cycles) {
        return Err("漫游只允许2–4个步态周期".into());
    }
    let (position, start) =
        ground(point, physical_size, screens, logical_size).ok_or("没有可用屏幕")?;
    let screen = screens
        .iter()
        .find(|s| {
            s.name == position.monitor && s.x == position.monitor_x && s.y == position.monitor_y
        })
        .ok_or("显示器已变化")?;
    if point.1.abs_diff(start.1) > (3.0 * screen.scale) as u32 {
        return Err("请先将宠物放到底部再开启漫游".into());
    }
    let mut edge = position;
    edge.x = if right { 1.0 } else { 0.0 };
    let limit = target(Some(&edge), screens, logical_size).ok_or("没有可用边界")?;
    // Canvas occupies the pet window width; frame-authored stride follows display scale/DPI.
    let step = f64::from(stride) * f64::from(physical_size.0) / f64::from(source.frame_width);
    if !step.is_finite() || step <= 0.0 {
        return Err("宠物窗口尺寸无效".into());
    }
    let room = (f64::from(limit.0) - f64::from(start.0)).abs();
    let cycles = cycles.min((room / step).floor() as u32);
    if cycles < 2 {
        return Err("此方向没有足够的步行空间".into());
    }
    let mut clip = source.clone();
    clip.loop_repeats = cycles;
    clip.validate().map_err(|error| error.to_string())?;
    let entry_ms = clip.durations_ms[..clip.loop_start]
        .iter()
        .map(|v| u64::from(*v))
        .sum();
    let cycle_ms: u64 = clip.durations_ms[clip.loop_start..clip.loop_end]
        .iter()
        .map(|v| u64::from(*v))
        .sum();
    Ok(WalkPlan {
        start,
        travel: step * f64::from(cycles) * if right { 1.0 } else { -1.0 },
        entry_ms,
        moving_ms: cycle_ms * u64::from(cycles),
        duration_ms: clip.duration_ms(),
        clip,
        topology: format!("{screens:?}"),
    })
}

pub(super) fn validate_walk(clip: &PetMotionClip) -> Result<(), String> {
    clip.validate().map_err(|error| error.to_string())?;
    if !clip.path.ends_with(".apng") || clip.locomotion.is_none() {
        return Err("宠物缺少已验收的 APNG 步幅数据".into());
    }
    let sum = |range: std::ops::Range<usize>| {
        clip.durations_ms[range]
            .iter()
            .map(|v| u64::from(*v))
            .sum::<u64>()
    };
    if clip.loop_start == 0
        || clip.loop_end == clip.durations_ms.len()
        || clip.loop_end - clip.loop_start < 4
        || sum(0..clip.loop_start) > 2000
        || sum(clip.loop_end..clip.durations_ms.len()) > 2000
    {
        return Err("步态需要完整循环和短暂起停片段".into());
    }
    if !(400..=3000).contains(&sum(clip.loop_start..clip.loop_end)) {
        return Err("步态周期不在安全范围内".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn clip() -> PetMotionClip {
        PetMotionClip {
            path: "walk.apng".into(),
            frame_width: 192,
            frame_height: 208,
            columns: 1,
            durations_ms: vec![100; 12],
            loop_start: 2,
            loop_end: 10,
            loop_repeats: 1,
            neutral_bookends: false,
            locomotion: Some(types::pet_motion::PetLocomotion { stride_px: 80 }),
        }
    }
    #[test]
    fn walking_stays_on_ground_and_moves_only_during_complete_cycles() {
        let screens = vec![Screen {
            name: Some("left".into()),
            x: -1920,
            y: 50,
            width: 1920,
            height: 1080,
            scale: 1.0,
            primary: true,
        }];
        let (_, position) = ground((-1000, 200), (90, 102), &screens, (90.0, 102.0)).unwrap();
        let walk = plan(
            position,
            (90, 102),
            &screens,
            (90.0, 102.0),
            &clip(),
            true,
            3,
        )
        .unwrap();
        assert_eq!(walk.point(100), position);
        assert_eq!(walk.point(201), walk.point(299));
        assert_eq!(
            walk.point(walk.entry_ms + walk.moving_ms),
            walk.point(walk.duration_ms)
        );
        assert_eq!(walk.point(walk.duration_ms).1, position.1);
        assert!(walk.point(walk.duration_ms).0 < -90);
        assert!(plan(
            (-1000, 200),
            (90, 102),
            &screens,
            (90.0, 102.0),
            &clip(),
            true,
            3
        )
        .is_err());
    }
    #[test]
    fn rejects_unqualified_art_and_does_not_cross_display_edge() {
        let screens = vec![Screen {
            name: None,
            x: 0,
            y: 0,
            width: 300,
            height: 400,
            scale: 2.0,
            primary: true,
        }];
        let (_, point) = ground((180, 0), (180, 204), &screens, (90.0, 102.0)).unwrap();
        assert!(plan(point, (180, 204), &screens, (90.0, 102.0), &clip(), true, 2).is_err());
        let mut unqualified = clip();
        unqualified.locomotion = None;
        assert!(plan(
            point,
            (180, 204),
            &screens,
            (90.0, 102.0),
            &unqualified,
            false,
            2
        )
        .is_err());
    }
}
