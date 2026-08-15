#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn contains(self, point: Point) -> bool {
        point.x >= self.x
            && point.y >= self.y
            && point.x < self.x.saturating_add(self.width)
            && point.y < self.y.saturating_add(self.height)
    }
}

pub fn monitor_at_pointer(monitors: &[Rect], pointer: Point) -> Option<Rect> {
    monitors
        .iter()
        .copied()
        .find(|monitor| monitor.contains(pointer))
}

pub fn clamp_popup_origin(desired: Point, popup: Size, monitor: Rect, margin: i32) -> Point {
    Point {
        x: clamp_axis(desired.x, popup.width, monitor.x, monitor.width, margin),
        y: clamp_axis(desired.y, popup.height, monitor.y, monitor.height, margin),
    }
}

fn clamp_axis(desired: i32, item_size: i32, area_start: i32, area_size: i32, margin: i32) -> i32 {
    let margin = margin.max(0);
    let lower = area_start.saturating_add(margin);
    let upper = area_start
        .saturating_add(area_size)
        .saturating_sub(item_size)
        .saturating_sub(margin);

    if upper < lower {
        area_start
    } else {
        desired.clamp(lower, upper)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Rect = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };

    const POPUP: Size = Size {
        width: 430,
        height: 250,
    };

    #[test]
    fn leaves_in_bounds_position_unchanged() {
        let desired = Point { x: 500, y: 300 };
        assert_eq!(clamp_popup_origin(desired, POPUP, MONITOR, 12), desired);
    }

    #[test]
    fn clamps_at_bottom_right_edge() {
        assert_eq!(
            clamp_popup_origin(Point { x: 1910, y: 1070 }, POPUP, MONITOR, 12),
            Point { x: 1478, y: 818 }
        );
    }

    #[test]
    fn clamps_on_monitor_with_negative_origin() {
        let left_monitor = Rect {
            x: -1920,
            y: -120,
            width: 1920,
            height: 1080,
        };

        assert_eq!(
            clamp_popup_origin(Point { x: -1918, y: -118 }, POPUP, left_monitor, 12),
            Point { x: -1908, y: -108 }
        );
    }

    #[test]
    fn chooses_monitor_containing_pointer() {
        let monitors = [
            Rect {
                x: -1280,
                y: 0,
                width: 1280,
                height: 1024,
            },
            MONITOR,
        ];

        assert_eq!(
            monitor_at_pointer(&monitors, Point { x: -10, y: 500 }),
            Some(monitors[0])
        );
        assert_eq!(
            monitor_at_pointer(&monitors, Point { x: 100, y: 500 }),
            Some(MONITOR)
        );
    }

    #[test]
    fn returns_none_for_pointer_outside_all_monitors() {
        assert_eq!(monitor_at_pointer(&[MONITOR], Point { x: -1, y: -1 }), None);
    }

    #[test]
    fn anchors_oversized_popup_at_monitor_origin() {
        assert_eq!(
            clamp_popup_origin(
                Point { x: 40, y: 40 },
                Size {
                    width: 2000,
                    height: 1200,
                },
                MONITOR,
                12,
            ),
            Point { x: 0, y: 0 }
        );
    }
}
