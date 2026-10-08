//! Фоновая часть программы: подсветка, Fn, клавиши, режимы, датчики и значок в трее.
//! Окна у неё нет, поэтому памяти она берёт мало. Окно настроек — отдельный запуск.
use crate::ipc::{self, Msg, Snapshot};
use crate::log;
use crate::state::State;
use crate::worker::Cmd;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

struct Links {
    kb: Sender<Cmd>,
    pw: Sender<crate::power::Cmd>,
}
static LINKS: OnceLock<Mutex<Links>> = OnceLock::new();
/// Сейчас работает «реакция на нажатия» — при выходе вернуть свои цвета.
static OWN: Mutex<Option<Box<[Option<crate::keyboard::Rgb>; crate::keyboard::KEYS]>>> = Mutex::new(None);

/// Команда от окна (вызывается из окна трея).
pub fn dispatch(m: Msg) {
    let Some(l) = LINKS.get().and_then(|l| l.lock().ok()) else { return };
    match m {
        Msg::Keys(k, b) => {
            crate::hook::REACTIVE.store(false, Ordering::Relaxed);
            let _ = l.kb.send(Cmd::Keys(ipc::unpack(&k), b));
        }
        Msg::Body(e, c, b) => {
            let _ = l.kb.send(Cmd::Body(e, c, b));
        }
        Msg::Save(k, e, c, b) => {
            let _ = l.kb.send(Cmd::Save(k.as_ref().map(ipc::unpack), e, c, b));
        }
        Msg::Effect(k, s, c, d) => {
            crate::hook::REACTIVE.store(false, Ordering::Relaxed);
            let _ = l.kb.send(Cmd::Effect(k, s, c, d));
        }
        Msg::Reactive(k, s, c, bg) => {
            crate::hook::REACTIVE.store(true, Ordering::Relaxed);
            let _ = l.kb.send(Cmd::Reactive(k, s, c, ipc::unpack(&bg)));
        }
        Msg::Numpad(p) => {
            let _ = l.kb.send(Cmd::Numpad(ipc::unpack(&p)));
        }
        Msg::SetMode(i) => {
            let _ = l.pw.send(crate::power::Cmd::SetMode(i));
            crate::tray::poke();
        }
        Msg::Watch(v) => {
            let _ = l.pw.send(crate::power::Cmd::Watch(v));
        }
        Msg::Swapped(s) => crate::hook::set(&s),
        Msg::Tray(on) => {
            crate::tray::SHOW.store(on, Ordering::Relaxed);
            crate::tray::poke();
        }
        Msg::Debug(on) => log::set(on),
        Msg::Press(id) => crate::reactive::press(id),
        Msg::Quit => quit(),
    }
}

/// Завершить фоновую часть.
pub fn quit() {
    log::write("Фон: выход");
    crate::tray::remove();
    // «реакцию на нажатия» рисует программа — без неё возвращаем свои цвета
    if crate::hook::REACTIVE.load(Ordering::Relaxed) {
        if let (Some(l), Ok(own)) = (LINKS.get().and_then(|l| l.lock().ok()), OWN.lock()) {
            if let Some(k) = own.as_ref() {
                let _ = l.kb.send(Cmd::Keys(k.clone(), 100));
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
        }
    }
    // окно настроек, если открыто, тоже закрываем
    let t: Vec<u16> = "Open Alienware Command Center".encode_utf16().chain(Some(0)).collect();
    unsafe {
        let h = FindWindowW(std::ptr::null(), t.as_ptr());
        if h != 0 {
            PostMessageW(h, 0x0010, 0, 0);
        }
    }
    std::process::exit(0);
}

#[link(name = "user32")]
extern "system" {
    fn FindWindowW(c: *const u16, n: *const u16) -> isize;
    fn PostMessageW(h: isize, m: u32, w: usize, l: isize) -> i32;
    fn SetProcessDpiAwarenessContext(v: isize) -> i32;
    fn SetProcessDPIAware() -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> isize;
    fn SetProcessWorkingSetSize(p: isize, min: usize, max: usize) -> i32;
}

/// Отдать Windows неиспользуемую память (после запуска и после закрытия окна).
pub fn trim() {
    unsafe {
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

/// Запустить фоновую часть и не возвращаться.
pub fn run() -> ! {
    // чёткий значок в трее при масштабе экрана (раньше это делало окно)
    unsafe {
        if SetProcessDpiAwarenessContext(-4) == 0 {
            SetProcessDPIAware();
        }
    }
    let st = State::load();
    log::set(st.debug);
    log::write("Фон: запуск");
    let (kb, status) = crate::worker::start(None);
    let (pw, sensors) = crate::power::start();
    crate::hook::set(&st.swapped);
    crate::hook::start();
    crate::tray::SHOW.store(st.tray, Ordering::Relaxed);
    let _ = LINKS.set(Mutex::new(Links { kb: kb.clone(), pw: pw.clone() }));
    crate::tray::start(pw.clone(), sensors.clone());

    // привести подсветку к сохранённому виду
    let own = Box::new(st.keys_out());
    if let Ok(mut o) = OWN.lock() {
        *o = Some(own.clone());
    }
    let _ = kb.send(Cmd::Numpad(Box::new(st.pad_out())));
    let e = &st.effect;
    let ecol = crate::state::from_hex(&e.color).unwrap_or([0, 240, 240]);
    if e.kind == 0 {
        let _ = kb.send(Cmd::Keys(own.clone(), 100));
    } else if crate::reactive::is_reactive(e.kind) {
        crate::hook::REACTIVE.store(true, Ordering::Relaxed);
        let bg = if e.bg { own.clone() } else { Box::new([None; crate::keyboard::KEYS]) };
        let _ = kb.send(Cmd::Reactive(e.kind, e.speed, ecol, bg));
    }
    // обычные эффекты клавиатура помнит сама — их не трогаем
    crate::autostart::refresh_path();
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(5));
        trim();
    });

    publish_loop(status, sensors)
}

/// Несколько раз в секунду выкладывать состояние для окна.
fn publish_loop(status: Arc<Mutex<crate::worker::Status>>, sensors: Arc<Mutex<crate::power::Sensors>>) -> ! {
    loop {
        let s = status.lock().map(|s| s.clone()).unwrap_or_default();
        let p = sensors.lock().map(|s| s.clone()).unwrap_or_default();
        let (hits, heat) = crate::reactive::export();
        let snap = Snapshot {
            started: s.started,
            keyboard: s.keyboard,
            body: s.body,
            fn_lock: s.fn_lock,
            touchpad: s.touchpad,
            num: crate::hook::NUM.load(Ordering::Relaxed),
            caps: crate::hook::CAPS.load(Ordering::Relaxed),
            last_log: log::last(),
            power_ok: p.ok,
            mode: p.mode,
            cpu_temp: p.cpu_temp,
            gpu_temp: p.gpu_temp,
            cpu_load: p.cpu_load,
            gpu_load: p.gpu_load,
            fans: p.fans,
            history: p.history,
            hits,
            heat,
        };
        ipc::publish(&snap);
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
}
