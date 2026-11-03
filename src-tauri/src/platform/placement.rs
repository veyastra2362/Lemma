//! HUD 定位：纯几何逻辑，不碰任何 OS API，因此可以在任何平台上跑单测。
//!
//! 规则（优先级从高到低）：
//! 1. 默认贴光标右下方 `gap` 像素；
//! 2. 右侧放不下就翻到光标左侧，下侧放不下就翻到光标上方；
//! 3. 翻转后仍溢出，钳制进工作区；
//! 4. 窗口比工作区还大时，贴工作区原点并把尺寸收缩到工作区大小。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    pub const fn new(origin: Point, size: Size) -> Self {
        Self { origin, size }
    }

    pub fn right(&self) -> i32 {
        saturating_i32(self.origin.x as i64 + self.size.width as i64)
    }

    pub fn bottom(&self) -> i32 {
        saturating_i32(self.origin.y as i64 + self.size.height as i64)
    }
}

fn saturating_i32(value: i64) -> i32 {
    value.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn clamp(value: i32, min: i32, max: i32) -> i32 {
    if max < min { min } else { value.clamp(min, max) }
}

/// 计算 HUD 落在哪里。返回的矩形保证完全位于 `work_area` 之内。
pub fn place_near_cursor(cursor: Point, size: Size, work_area: Rect, gap: i32) -> Rect {
    // 窗口比工作区大：收缩并贴左上角，避免露出屏幕之外。
    let width = size.width.min(work_area.size.width);
    let height = size.height.min(work_area.size.height);

    let gap = gap.max(0);
    let w = width as i64;
    let h = height as i64;
    let gap64 = gap as i64;

    let mut x = cursor.x as i64 + gap64;
    if x + w > work_area.right() as i64 {
        x = cursor.x as i64 - gap64 - w;
    }
    let mut y = cursor.y as i64 + gap64;
    if y + h > work_area.bottom() as i64 {
        y = cursor.y as i64 - gap64 - h;
    }

    let max_x = work_area.right() as i64 - w;
    let max_y = work_area.bottom() as i64 - h;

    Rect::new(
        Point {
            x: clamp(saturating_i32(x), work_area.origin.x, saturating_i32(max_x)),
            y: clamp(saturating_i32(y), work_area.origin.y, saturating_i32(max_y)),
        },
        Size { width, height },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect::new(
        Point { x: 0, y: 0 },
        Size {
            width: 1920,
            height: 1040,
        },
    );

    fn size() -> Size {
        Size {
            width: 420,
            height: 300,
        }
    }

    #[test]
    fn places_to_the_bottom_right_by_default() {
        let rect = place_near_cursor(Point { x: 400, y: 400 }, size(), WORK, 14);
        assert_eq!(rect.origin, Point { x: 414, y: 414 });
        assert_eq!(rect.size, size());
    }

    #[test]
    fn flips_to_the_left_when_the_right_edge_would_overflow() {
        // 1820 + 14 + 420 > 1920，应该翻到光标左侧
        let rect = place_near_cursor(Point { x: 1820, y: 100 }, size(), WORK, 14);
        assert_eq!(rect.origin.x, 1820 - 14 - 420);
        assert_eq!(rect.right(), 1806);
    }

    #[test]
    fn flips_above_when_the_bottom_would_overflow() {
        let rect = place_near_cursor(Point { x: 100, y: 1000 }, size(), WORK, 14);
        assert_eq!(rect.origin.y, 1000 - 14 - 300);
        assert_eq!(rect.bottom(), 986);
    }

    #[test]
    fn flips_both_axes_in_the_corner() {
        let rect = place_near_cursor(Point { x: 1910, y: 1030 }, size(), WORK, 14);
        assert!(rect.right() <= WORK.right());
        assert!(rect.bottom() <= WORK.bottom());
        assert!(rect.origin.x < 1910);
        assert!(rect.origin.y < 1030);
    }

    #[test]
    fn clamps_into_a_secondary_monitor_with_negative_origin() {
        let secondary = Rect::new(
            Point { x: -1920, y: -200 },
            Size {
                width: 1920,
                height: 1080,
            },
        );
        let rect = place_near_cursor(Point { x: -1915, y: -195 }, size(), secondary, 14);
        assert_eq!(rect.origin, Point { x: -1901, y: -181 });

        // 顶到负坐标屏的左上角时不能跑出工作区
        let rect = place_near_cursor(Point { x: -1919, y: -199 }, size(), secondary, -40);
        assert_eq!(rect.origin.x, secondary.origin.x);
        assert_eq!(rect.origin.y, secondary.origin.y);
    }

    #[test]
    fn shrinks_and_pins_when_the_window_is_larger_than_the_work_area() {
        let tiny = Rect::new(
            Point { x: 0, y: 0 },
            Size {
                width: 300,
                height: 200,
            },
        );
        let rect = place_near_cursor(Point { x: 150, y: 100 }, size(), tiny, 14);
        assert_eq!(rect.size, Size { width: 300, height: 200 });
        assert_eq!(rect.origin, Point { x: 0, y: 0 });
    }

    #[test]
    fn treats_negative_gap_as_zero() {
        let rect = place_near_cursor(Point { x: 400, y: 400 }, size(), WORK, -50);
        assert_eq!(rect.origin, Point { x: 400, y: 400 });
    }
}
