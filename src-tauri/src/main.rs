// 发布版不带控制台窗口；调试版保留，便于看 stderr 里的性能打点。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // CLI 子命令（探针 / 词库导入 / 查词）不启动 GUI，跑完即退。
    if lemma_lib::cli::try_run() {
        return;
    }

    lemma_lib::run()
}
