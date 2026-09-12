//! Deterministic, critically-damped task-window morph. No task/request state here.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn from_tuple(r: (i32, i32, u32, u32)) -> Self {
        Self {
            x: r.0 as f64,
            y: r.1 as f64,
            width: r.2 as f64,
            height: r.3 as f64,
        }
    }
}
#[derive(Default)]
pub(super) struct Morph {
    pub rect: Rect,
    velocity: Rect,
    pub progress: f64,
    speed: f64,
    initialized: bool,
}
fn spring(value: &mut f64, velocity: &mut f64, target: f64, dt: f64) {
    let omega = 24.0;
    let delta = *value - target;
    let c = *velocity + omega * delta;
    let decay = (-omega * dt).exp();
    *value = target + (delta + c * dt) * decay;
    *velocity = (*velocity - omega * c * dt) * decay;
}
impl Morph {
    pub fn snap(&mut self, rect: Rect, expanded: bool) {
        self.rect = rect;
        self.velocity = Rect::default();
        self.progress = if expanded { 1.0 } else { 0.0 };
        self.speed = 0.0;
        self.initialized = true;
    }
    pub fn step(&mut self, target: Rect, expanded: bool, dt: f64, reduced: bool) -> bool {
        if !self.initialized || reduced {
            self.snap(target, expanded);
            return false;
        }
        let dt = dt.clamp(0.0, 0.05);
        let progress = if expanded { 1.0 } else { 0.0 };
        spring(&mut self.rect.x, &mut self.velocity.x, target.x, dt);
        spring(&mut self.rect.y, &mut self.velocity.y, target.y, dt);
        spring(
            &mut self.rect.width,
            &mut self.velocity.width,
            target.width,
            dt,
        );
        spring(
            &mut self.rect.height,
            &mut self.velocity.height,
            target.height,
            dt,
        );
        spring(&mut self.progress, &mut self.speed, progress, dt);
        let at_rest = (self.rect.x - target.x).abs() < 0.2
            && (self.rect.y - target.y).abs() < 0.2
            && (self.rect.width - target.width).abs() < 0.2
            && (self.rect.height - target.height).abs() < 0.2
            && (self.progress - progress).abs() < 0.002
            && self.speed.abs() < 0.02;
        if at_rest {
            self.snap(target, expanded);
        }
        !at_rest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn capsule() -> Rect {
        Rect {
            x: 600.0,
            y: 600.0,
            width: 166.0,
            height: 38.0,
        }
    }
    fn panel() -> Rect {
        Rect {
            x: 374.0,
            y: 238.0,
            width: 392.0,
            height: 400.0,
        }
    }
    #[test]
    fn opening_has_real_intermediate_frames_and_preserves_the_shared_edge() {
        let mut m = Morph::default();
        m.snap(capsule(), false);
        m.step(panel(), true, 1.0 / 60.0, false);
        assert!(m.rect.width > 166.0 && m.rect.width < 392.0);
        assert!(m.progress > 0.0 && m.progress < 1.0);
        assert!((m.rect.x + m.rect.width - 766.0).abs() < 0.001);
        for _ in 0..120 {
            m.step(panel(), true, 1.0 / 60.0, false);
        }
        assert_eq!(m.rect, panel());
        assert_eq!(m.progress, 1.0);
    }
    #[test]
    fn reversal_retargets_the_live_frame_without_resetting_position_or_velocity() {
        let mut m = Morph::default();
        m.snap(capsule(), false);
        for _ in 0..8 {
            m.step(panel(), true, 1.0 / 60.0, false);
        }
        let current = m.rect;
        m.step(capsule(), false, 0.0, false);
        assert_eq!(m.rect, current);
        for _ in 0..120 {
            m.step(capsule(), false, 1.0 / 60.0, false);
        }
        assert_eq!(m.rect, capsule());
        assert_eq!(m.progress, 0.0);
    }
    #[test]
    fn reduced_motion_and_suppression_snap_without_delayed_callbacks() {
        let mut m = Morph::default();
        m.snap(capsule(), false);
        assert!(!m.step(panel(), true, 0.016, true));
        assert_eq!(m.rect, panel());
        m.snap(capsule(), false);
        assert_eq!(m.progress, 0.0);
    }
}
