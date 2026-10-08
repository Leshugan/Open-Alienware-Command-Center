#![windows_subsystem = "windows"]
mod autostart;
mod calib;
mod chassis;
mod glyphs;
mod hid;
mod hook;
mod icons_data;
mod ipc;
mod keyboard;
mod layout;
mod power;
mod reactive;
mod service;
mod log;
mod state;
mod tray;
mod ui;
mod worker;

#[link(name = "shell32")]
extern "system" {
    fn IsUserAnAdmin() -> i32;
    fn ShellExecuteW(hwnd: isize, op: *const u16, file: *const u16, params: *const u16, dir: *const u16, show: i32) -> isize;
}

/// Режимы питания и датчики доступны только с правами администратора —
/// без них перезапускаемся с ними (окно подтверждения Windows, если оно включено).
fn elevate() -> bool {
    if unsafe { IsUserAnAdmin() } != 0 || std::env::args().any(|a| a == "--no-elevate") {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let w = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let bg = if std::env::args().any(|a| a == "--background") { " --background" } else { "" };
    let (op, file, params) = (w("runas"), w(&exe.to_string_lossy()), w(&format!("--no-elevate{bg}")));
    unsafe { ShellExecuteW(0, op.as_ptr(), file.as_ptr(), params.as_ptr(), std::ptr::null(), 1) > 32 }
}

#[link(name = "user32")]
extern "system" {
    fn FindWindowW(c: *const u16, n: *const u16) -> isize;
    fn ShowWindow(h: isize, cmd: i32) -> i32;
    fn SetForegroundWindow(h: isize) -> i32;
}

/// Окно программы уже открыто — показать его вместо второго.
fn show_existing_window() -> bool {
    let t: Vec<u16> = "Open Alienware Command Center".encode_utf16().chain(Some(0)).collect();
    let h = unsafe { FindWindowW(std::ptr::null(), t.as_ptr()) };
    if h == 0 {
        return false;
    }
    unsafe {
        ShowWindow(h, 5);
        ShowWindow(h, 9);
        SetForegroundWindow(h);
    }
    true
}

fn main() -> eframe::Result<()> {
    if elevate() {
        return Ok(());
    }
    // фоновая часть: без окна, мало памяти
    if std::env::args().any(|a| a == "--background") {
        if ipc::service_window() != 0 {
            return Ok(());
        }
        service::run();
    }
    // окно настроек
    if show_existing_window() {
        return Ok(());
    }
    if ipc::service_window() == 0 {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).args(["--background", "--no-elevate"]).spawn();
        }
        for _ in 0..60 {
            if ipc::service_window() != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    let icon = ui::app_icon();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Open Alienware Command Center")
            .with_inner_size([1300.0, 780.0])
            .with_min_inner_size([1180.0, 750.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native("Open Alienware Command Center", options, Box::new(|cc| Ok(Box::new(ui::App::new(cc)))))
}
