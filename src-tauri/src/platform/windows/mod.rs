//! Windows 后端。
mod window;

use crate::platform::placement::{Point, Rect, Size};
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

pub(crate) fn cursor_position() -> Option<Point> {
    let mut point = POINT::default();
    // SAFETY: point 是本栈帧上的有效可写结构。
    if unsafe { GetCursorPos(&mut point) }.is_err() {
        return None;
    }
    Some(Point {
        x: point.x,
        y: point.y,
    })
}

pub(crate) fn work_area_at(point: Point) -> Option<Rect> {
    let probe = POINT {
        x: point.x,
        y: point.y,
    };
    // SAFETY: 纯查询调用，无所有权转移。
    let monitor = unsafe { MonitorFromPoint(probe, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return None;
    }

    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: cbSize 已按约定填好，info 为本栈帧有效可写结构。
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }

    let work = info.rcWork;
    Some(Rect::new(
        Point {
            x: work.left,
            y: work.top,
        },
        Size {
            width: (work.right - work.left).max(0) as u32,
            height: (work.bottom - work.top).max(0) as u32,
        },
    ))
}

mod keyboard;
pub(crate) use keyboard::watch as watch_keyboard;

mod uia;
pub(crate) use uia::{capture as uia_capture, probe as uia_probe};

mod clipboard;
pub(crate) use clipboard::{copy_selection, restore_clipboard};
