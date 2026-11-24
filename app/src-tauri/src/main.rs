// 发布版在 Windows 上不弹黑色控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    td_launcher_lib::run()
}
