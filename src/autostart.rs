//! Автозапуск вместе с Windows: задача в Планировщике заданий с правами администратора
//! (обычный автозапуск не умеет запускать программу с правами без запроса).
use crate::log;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};

const TASK: &str = "Open Alienware Command Center";
const NO_WINDOW: u32 = 0x0800_0000;

/// 0 — ещё не знаем, 1 — выключен, 2 — включён.
pub static STATE: AtomicU8 = AtomicU8::new(0);

pub fn is_on() -> Option<bool> {
    match STATE.load(Ordering::Relaxed) {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

fn schtasks(args: &[&str]) -> bool {
    Command::new("schtasks").args(args).creation_flags(NO_WINDOW).output().map(|o| o.status.success()).unwrap_or(false)
}

/// Создать задачу: при входе в Windows, с правами администратора, и от батареи тоже
/// (обычная задача от батареи не запускается — поэтому описание в XML).
fn create() -> bool {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let user = std::env::var("USERDOMAIN").map(|d| format!("{d}\\")).unwrap_or_default() + &std::env::var("USERNAME").unwrap_or_default();
    let xml = format!(r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId><Delay>PT3S</Delay></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>5</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>{exe}</Command><Arguments>--background</Arguments></Exec></Actions>
</Task>"#, user = esc(&user), exe = esc(&exe));
    let path = std::env::temp_dir().join("oawcc_task.xml");
    let mut bytes = vec![0xFFu8, 0xFE];
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    if std::fs::write(&path, bytes).is_err() {
        return false;
    }
    let ok = schtasks(&["/Create", "/TN", TASK, "/XML", &path.to_string_lossy(), "/F"]);
    let _ = std::fs::remove_file(&path);
    ok
}

/// Если автозапуск включён, а программу перенесли в другую папку — обновить путь.
pub fn refresh_path() {
    std::thread::spawn(|| {
        let on = schtasks(&["/Query", "/TN", TASK]);
        STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
        if on {
            create();
        }
    });
}

/// Включить или выключить (в фоне).
pub fn set(on: bool) {
    std::thread::spawn(move || {
        let ok = if on { create() } else {
            schtasks(&["/Delete", "/TN", TASK, "/F"])
        };
        log::write(&format!("Автозапуск {} — {}", if on { "включение" } else { "выключение" }, if ok { "ок" } else { "ошибка" }));
        let now = schtasks(&["/Query", "/TN", TASK]);
        STATE.store(if now { 2 } else { 1 }, Ordering::Relaxed);
    });
}
