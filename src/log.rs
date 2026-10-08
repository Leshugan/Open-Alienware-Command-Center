//! Отладочная запись на рабочий стол: Open Alienware Command Center_debug.txt
//! Включается в «Настройках», по умолчанию выключена.
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static ON: AtomicBool = AtomicBool::new(false);

static LAST: Mutex<String> = Mutex::new(String::new());

fn path() -> std::path::PathBuf {
    let base = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    let desk = std::path::Path::new(&base).join("Desktop");
    let dir = if desk.exists() { desk } else { std::path::PathBuf::from(base) };
    dir.join("Open Alienware Command Center_debug.txt")
}

/// Включить или выключить запись (при включении файл начинается заново).
pub fn set(on: bool) {
    let was = ON.swap(on, Ordering::Relaxed);
    if on && !was {
        let _ = std::fs::write(path(), format!("Open Alienware Command Center {} — отладка включена\n", env!("CARGO_PKG_VERSION")));
    }
}

/// То же, но без начала файла заново (для окна — файл ведёт фоновая часть).
pub fn quiet(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

pub fn write(msg: &str) {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() % 100_000_000).unwrap_or(0);
    if ON.load(Ordering::Relaxed) {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path()) {
            let _ = writeln!(f, "[{t}] {msg}");
        }
    }
    if let Ok(mut l) = LAST.lock() {
        *l = msg.to_string();
    }
}

pub fn last() -> String {
    LAST.lock().map(|l| l.clone()).unwrap_or_default()
}

pub fn hex(b: &[u8]) -> String {
    let n = b.iter().rposition(|&x| x != 0).map(|i| i + 1).unwrap_or(0).max(4).min(b.len());
    b[..n].iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}
