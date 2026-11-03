//! HUD 与设置窗口的生命周期。
//!
//! 单例预热原则：窗口在启动时就创建好（WebView2 也一并起来），之后只做
//! `show` / `hide` / `set_position`。绝不在呼出路径上重建窗口——那会带来
//! 白屏闪烁 + 几十毫秒的 WebView 初始化 + 内存泄漏。

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Runtime, WebviewWindow};

use crate::core::perf::Perf;
use crate::platform::{self, PlatformWindow, placement, window::WindowOps};

pub const HUD_LABEL: &str = "shell";
pub const SETTINGS_LABEL: &str = "settings";

/// 光标与浮层之间的物理像素间距。
const CURSOR_GAP: i32 = 14;

/// 呼出 HUD：重新定位 → 显示 → 抢焦点 → 通知前端开始记首帧。
pub fn present<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(HUD_LABEL) else {
        eprintln!("[hud] 窗口 {HUD_LABEL} 不存在");
        return;
    };

    let seq = app.state::<Perf>().begin_present();

    reposition(&window);

    let ops = PlatformWindow::new(window.clone());
    let _ = window.unminimize();
    let _ = window.show();
    ops.force_show();
    let _ = window.set_focus();

    // 前端收到后必须在下一帧回报 report_first_paint(seq)。
    let _ = app.emit("hud://presented", seq);
}

/// 收起 HUD。调用方通常是前端失焦 / Esc。
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(HUD_LABEL) {
        let ops = PlatformWindow::new(window.clone());
        let _ = window.hide();
        ops.force_hide();
    }
}

pub fn toggle<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(HUD_LABEL) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        hide(app);
    } else {
        present(app);
    }
}

/// 把 HUD 挪到光标附近，并钳制在光标所在显示器的工作区内。
fn reposition<R: Runtime>(window: &WebviewWindow<R>) {
    let Some(cursor) = platform::cursor_position() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    if size.width == 0 || size.height == 0 {
        // 首次呼出前某些平台会报 0 尺寸，此时保持配置里的居中位置。
        return;
    }
    let Some(work_area) = platform::work_area_at(cursor) else {
        return;
    };

    let target = placement::place_near_cursor(
        cursor,
        placement::Size {
            width: size.width,
            height: size.height,
        },
        work_area,
        CURSOR_GAP,
    );
    let _ = window.set_position(PhysicalPosition::new(target.origin.x, target.origin.y));
}

/// 显示设置窗口。
///
/// 注意这里必须分两步：先 `window.show()` 让宿主窗口出来，再显式给
/// WebView 控制器同步一次尺寸并 `webview.show()`。只做前者时，之前被隐藏
/// 过或尺寸为 0 的 WebView2 会在透明客户区外面留一圈可见边框。
pub fn present_settings<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let window = app
        .get_webview_window(SETTINGS_LABEL)
        .ok_or_else(|| "设置窗口不存在".to_string())?;

    window.unminimize().map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;

    let size = window.inner_size().map_err(|e| e.to_string())?;
    if size.width == 0 || size.height == 0 {
        return Err("设置窗口尺寸无效".to_string());
    }

    let webview: &tauri::Webview<R> = window.as_ref();
    webview.set_size(size).map_err(|e| e.to_string())?;
    webview.show().map_err(|e| e.to_string())?;

    PlatformWindow::new(window.clone()).force_show();
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}
