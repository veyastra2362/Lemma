//! 非 Windows 平台的临时后端。
//!
//! 只保证契约成立、代码可编译、单测可跑；真正的 macOS（AX API）与
//! Linux（AT-SPI / XDG portal）实现在后续里程碑按契约逐个补齐。

use crate::platform::placement::{Point, Rect};

mod window;

pub(crate) fn cursor_position() -> Option<Point> {
    None
}

pub(crate) fn work_area_at(_point: Point) -> Option<Rect> {
    None
}

mod keyboard;
pub(crate) use keyboard::watch as watch_keyboard;

mod uia;
pub(crate) use uia::probe as uia_probe;
