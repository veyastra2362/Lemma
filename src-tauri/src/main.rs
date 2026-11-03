// 发布版不带控制台窗口；调试版保留，便于看 stderr 里的性能打点。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    trans_lib::run()
}
