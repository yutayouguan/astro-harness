use types::pet_preferences::PetPosition;

#[derive(Debug, Clone)]
pub(super) struct Screen {
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub primary: bool,
}

fn limits(screen: &Screen, size: (f64, f64)) -> (f64, f64, f64, f64) {
    let free_x = (f64::from(screen.width) - size.0).max(0.0);
    let free_y = (f64::from(screen.height) - size.1).max(0.0);
    let pad_x = (12.0 * screen.scale).min(free_x / 2.0);
    let pad_y = (12.0 * screen.scale).min(free_y / 2.0);
    (
        f64::from(screen.x) + pad_x,
        f64::from(screen.y) + pad_y,
        (free_x - pad_x * 2.0).max(0.0),
        (free_y - pad_y * 2.0).max(0.0),
    )
}

pub(super) fn target(
    position: Option<&PetPosition>,
    screens: &[Screen],
    logical_size: (f64, f64),
) -> Option<(i32, i32)> {
    let selected = position
        .and_then(|p| {
            screens
                .iter()
                .filter(|s| s.name == p.monitor)
                .min_by_key(|s| {
                    (i64::from(s.x) - i64::from(p.monitor_x)).abs()
                        + (i64::from(s.y) - i64::from(p.monitor_y)).abs()
                })
        })
        .or_else(|| screens.iter().find(|s| s.primary))
        .or_else(|| screens.first())?;
    let size = (
        logical_size.0 * selected.scale,
        logical_size.1 * selected.scale,
    );
    let (x, y, w, h) = limits(selected, size);
    Some((
        (x + w * position.map_or(1.0, |p| p.x)).round() as i32,
        (y + h * position.map_or(1.0, |p| p.y)).round() as i32,
    ))
}

pub(super) fn capture(
    point: (i32, i32),
    size: (u32, u32),
    screens: &[Screen],
    snap: bool,
) -> Option<PetPosition> {
    let cx = f64::from(point.0) + f64::from(size.0) / 2.0;
    let cy = f64::from(point.1) + f64::from(size.1) / 2.0;
    let selected = screens
        .iter()
        .find(|s| {
            cx >= f64::from(s.x)
                && cy >= f64::from(s.y)
                && cx < f64::from(s.x) + f64::from(s.width)
                && cy < f64::from(s.y) + f64::from(s.height)
        })
        .or_else(|| {
            screens.iter().min_by(|a, b| {
                let distance = |s: &Screen| {
                    let dx = cx - cx.clamp(f64::from(s.x), f64::from(s.x) + f64::from(s.width));
                    let dy = cy - cy.clamp(f64::from(s.y), f64::from(s.y) + f64::from(s.height));
                    dx * dx + dy * dy
                };
                distance(a).total_cmp(&distance(b))
            })
        })
        .or_else(|| screens.first())?;
    let (x, y, w, h) = limits(selected, (f64::from(size.0), f64::from(size.1)));
    let normalize = |value: f64, start: f64, travel: f64| {
        if travel == 0.0 {
            return 0.0;
        }
        let offset = (value - start).clamp(0.0, travel);
        if snap && offset <= 18.0 * selected.scale {
            0.0
        } else if snap && travel - offset <= 18.0 * selected.scale {
            1.0
        } else {
            offset / travel
        }
    };
    Some(PetPosition {
        monitor: selected.name.clone(),
        monitor_x: selected.x,
        monitor_y: selected.y,
        x: normalize(f64::from(point.0), x, w),
        y: normalize(f64::from(point.1), y, h),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn screens() -> Vec<Screen> {
        vec![
            Screen {
                name: Some("primary".into()),
                x: 0,
                y: 48,
                width: 2880,
                height: 1640,
                scale: 2.0,
                primary: true,
            },
            Screen {
                name: Some("left".into()),
                x: -1920,
                y: 24,
                width: 1920,
                height: 1016,
                scale: 1.0,
                primary: false,
            },
        ]
    }
    #[test]
    fn pet_position_round_trips_negative_monitor_coordinates() {
        let screens = screens();
        let p = capture((-1000, 500), (120, 136), &screens, false).unwrap();
        assert_eq!(
            target(Some(&p), &screens, (120.0, 136.0)),
            Some((-1000, 500))
        );
    }

    #[test]
    fn pet_dragged_over_secondary_dock_stays_on_the_nearest_monitor() {
        let screens = screens();
        let position = capture((-1000, 1020), (120, 136), &screens, true).unwrap();
        assert_eq!(position.monitor.as_deref(), Some("left"));
        assert_eq!(position.y, 1.0);
    }
    #[test]
    fn pet_snap_happens_only_near_edges_and_can_be_disabled() {
        let s = screens();
        let p = capture((-1900, 500), (120, 136), &s, true).unwrap();
        assert_eq!(p.x, 0.0);
        assert!(capture((-1900, 500), (120, 136), &s, false).unwrap().x > 0.0);
        assert_eq!(target(Some(&p), &s, (120.0, 136.0)).unwrap().0, -1908);
    }
    #[test]
    fn pet_disconnect_and_dpi_changes_keep_entire_pet_in_work_area() {
        let mut s = screens();
        let p = capture((-500, 800), (120, 136), &s, false).unwrap();
        s.pop();
        let (x, y) = target(Some(&p), &s, (180.0, 204.0)).unwrap();
        assert!(x >= 0 && y >= 48 && x + 360 <= 2880 && y + 408 <= 1688);
        assert_eq!(target(None, &[], (120.0, 136.0)), None);
    }
}
