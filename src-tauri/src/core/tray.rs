//! 系统托盘：常驻入口。关闭所有窗口后应用继续活在托盘里。

use tauri::{
    AppHandle,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::core::shell;

pub fn install(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let hud_item = MenuItem::with_id(app, "hud", "显示浮层", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
    let capture_item = MenuItem::with_id(app, "capture", "截图识别…", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出 Lemma", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &hud_item,
            &capture_item,
            &settings_item,
            &separator,
            &quit_item,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Lemma — 双击 Ctrl 呼出")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "hud" => shell::toggle(app),
            "capture" => {
                if let Err(error) = shell::present_region_select(app) {
                    eprintln!("[capture] {error}");
                }
            }
            "settings" => {
                if let Err(error) = shell::present_settings(app) {
                    eprintln!("[settings] {error}");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                shell::toggle(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    } else {
        eprintln!("[tray] 缺少默认图标，托盘图标将不可见");
    }

    builder.build(app)?;
    Ok(())
}
