//! HUD 与设置窗口的生命周期。
//!
//! 单例预热原则：窗口在启动时就创建好（WebView2 也一并起来），之后只做
//! `show` / `hide` / `set_position`。绝不在呼出路径上重建窗口——那会带来
//! 白屏闪烁 + 几十毫秒的 WebView 初始化 + 内存泄漏。

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Runtime, WebviewWindow};

use crate::core::logs;
use crate::core::perf::Perf;
use crate::core::settings::{HUD_MIN_HEIGHT, HUD_WIDTH};
use crate::platform::{self, PlatformWindow, placement, window::WindowOps};

pub const HUD_LABEL: &str = "shell";
pub const SETTINGS_LABEL: &str = "settings";

/// 光标与浮层之间的物理像素间距。
const CURSOR_GAP: i32 = 14;

/// 预热窗口用的屏幕外坐标（远离任何真实显示器）。
const OFFSCREEN: i32 = -32_000;

/// 预热兜底超时：超时后强制收尾。
const PREWARM_TIMEOUT_MS: u64 = 2_000;

/// 是否处在"预热中"：窗口为加载 WebView 被临时显示在屏幕外。
static PREWARMING: AtomicBool = AtomicBool::new(false);

/// 预热 HUD。
///
/// 注意：`tauri.conf.json` 里 `visible: false` 的窗口，其 WebView **不会**在启动时
/// 真正加载页面（settings / ocr-region 会加载，shell 不会，实测如此）。不预热的话，
/// 用户第一次按热键看到的是"先白一下再出内容"。
///
/// 做法：把窗口挪到屏幕外 → 显示 → 等前端回报就绪后再藏回来。
pub fn prewarm<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(HUD_LABEL) else {
        return;
    };
    PREWARMING.store(true, Ordering::SeqCst);
    let _ = window.set_position(PhysicalPosition::new(OFFSCREEN, OFFSCREEN));
    let _ = window.show();
    PlatformWindow::new(window).force_show();
    logs::line("hud prewarm requested");

    // 兜底：前端的就绪回报可能丢（IPC 通道建立时机有竞争），
    // 超时后无论如何把预热窗口藏回去，否则它会一直停在屏幕外算作"可见"。
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(PREWARM_TIMEOUT_MS));
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || finish_prewarm(&handle));
    });
}

/// 前端就绪后收尾预热：藏回窗口。
///
/// 如果用户在这一瞬间已经按了热键（presents > 0），就不要抢着把它藏掉。
pub fn finish_prewarm<R: Runtime>(app: &AppHandle<R>) {
    if !PREWARMING.swap(false, Ordering::SeqCst) {
        return;
    }
    if app.state::<Perf>().presents() > 0 {
        logs::line("hud prewarm kept visible (user presented first)");
        return;
    }
    hide(app);
    logs::line("hud prewarm done");
}

/// 呼出 HUD：重新定位 → 显示 → 抢焦点 → 通知前端开始记首帧。
pub fn present<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(HUD_LABEL) else {
        eprintln!("[hud] 窗口 {HUD_LABEL} 不存在");
        return;
    };

    let seq = app.state::<Perf>().begin_present();
    // 用户主动呼出时，预热窗口已经变成"正在用的窗口"，不能再被收尾逻辑藏掉。
    PREWARMING.store(false, Ordering::SeqCst);
    logs::line(format!("hud present seq={seq}"));

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
    // 预热窗口此刻是"屏幕外可见"的，直接按 is_visible 判断会把用户的操作变成隐藏。
    if PREWARMING.swap(false, Ordering::SeqCst) {
        present(app);
        return;
    }

    let Some(window) = app.get_webview_window(HUD_LABEL) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        hide(app);
    } else {
        present(app);
    }
}

/// 按内容高度调整 HUD（逻辑像素），并重新定位避免越界。
///
/// 高度由前端量出来（`ResizeObserver`），这里只做钳制与窗口操作——
/// 前端不该直接决定物理像素，那是 DPI 相关的事。
pub fn resize_hud<R: Runtime>(app: &AppHandle<R>, height: f64) -> Result<(), String> {
    let Some(window) = app.get_webview_window(HUD_LABEL) else {
        return Err("HUD 窗口不存在".to_string());
    };
    let max = app
        .state::<crate::core::settings::SettingsStore>()
        .get()
        .clamped_max_height();
    let height = height.clamp(HUD_MIN_HEIGHT, max);

    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            HUD_WIDTH, height,
        )))
        .map_err(|e| format!("调整 HUD 尺寸失败：{e}"))?;

    // 刻意**不**重新锚定到光标：用户点卡片时鼠标正在 HUD 上，
    // 重新锚定会让窗口每点一次就往右下挪一次。只在越界时把它推回来。
    if let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size())
        && let Some(work_area) = platform::work_area_at(placement::Point {
            x: position.x,
            y: position.y,
        })
    {
        let origin = placement::clamp_origin(
            placement::Point {
                x: position.x,
                y: position.y,
            },
            placement::Size {
                width: size.width,
                height: size.height,
            },
            work_area,
        );
        if origin.x != position.x || origin.y != position.y {
            let _ = window.set_position(PhysicalPosition::new(origin.x, origin.y));
        }
    }

    Ok(())
}

/// 从任意线程安全地切换 HUD。
///
/// 键盘钩子回调与全局热键回调都可能不在主线程，窗口操作必须回到主线程做。
pub fn toggle_on_main<R: Runtime>(app: &AppHandle<R>) {
    let runner = app.clone();
    let inner = app.clone();
    // 已经在主线程时 Tauri 会内联执行，不会死锁。
    let _ = runner.run_on_main_thread(move || toggle(&inner));
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

/// 按需获取窗口：已经存在就直接用，否则按 `tauri.conf.json` 里的声明构建。
///
/// settings / ocr-region 声明为 `"create": false`：三个窗口就是三个 WebView2
/// 渲染进程，启动时全建出来会让常驻内存多出几百 MB。声明仍然保留在配置里
/// （路由与透明配置依赖它），只是推迟到真正需要时再建。
pub fn ensure_window<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
) -> Result<WebviewWindow<R>, String> {
    if let Some(window) = app.get_webview_window(label) {
        return Ok(window);
    }

    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|candidate| candidate.label == label)
        .cloned()
        .ok_or_else(|| format!("窗口配置缺失：{label}"))?;

    let window = tauri::WebviewWindowBuilder::from_config(app, &config)
        .map_err(|e| format!("构建窗口 {label} 失败：{e}"))?
        .build()
        .map_err(|e| format!("创建窗口 {label} 失败：{e}"))?;

    if let Some(icon) = app.default_window_icon() {
        let _ = window.set_icon(icon.clone());
    }
    logs::line(format!("window created {label}"));
    Ok(window)
}

/// 设置窗口只挂一次关闭事件：点 × 只是隐藏，进程继续常驻托盘。
fn wire_settings_close<R: Runtime>(window: &WebviewWindow<R>) {
    static WIRED: AtomicBool = AtomicBool::new(false);
    if WIRED.swap(true, Ordering::SeqCst) {
        return;
    }

    let win = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = win.hide();
            PlatformWindow::new(win.clone()).force_hide();
        }
    });
}

/// 显示设置窗口。
///
/// 注意这里必须分两步：先 `window.show()` 让宿主窗口出来，再显式给
/// WebView 控制器同步一次尺寸并 `webview.show()`。只做前者时，之前被隐藏
/// 过或尺寸为 0 的 WebView2 会在透明客户区外面留一圈可见边框。
/// 关掉设置窗口（只隐藏，不退出进程）。
pub fn close_settings<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = window.hide();
        PlatformWindow::new(window).force_hide();
    }
}

pub fn present_settings<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let window = ensure_window(app, SETTINGS_LABEL)?;
    wire_settings_close(&window);

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
