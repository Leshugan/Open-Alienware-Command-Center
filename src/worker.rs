//! Фоновый поток, который общается с железом, чтобы окно не подвисало.
use crate::chassis::Chassis;
use crate::keyboard::{Keyboard, Rgb, KEYS};
use crate::log;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

pub enum Cmd {
    Keys(Box<[Option<Rgb>; KEYS]>, u8),
    Body(Rgb, Rgb, u8),
    /// None вместо цветов — сейчас работает эффект, сохраняем его
    Save(Option<Box<[Option<Rgb>; KEYS]>>, Rgb, Rgb, u8),
    Effect(u8, u8, Rgb, u8),
    /// реакция на нажатия: вид, скорость, цвет, фон
    Reactive(u8, u8, Rgb, Box<[Option<Rgb>; KEYS]>),
    /// цвета цифрового блока при выключенном Num Lock (поверх обычных)
    Numpad(Box<[Option<Rgb>; KEYS]>),
}

/// Обычные цвета, а при выключенном Num Lock — поверх них цвета цифрового блока.
fn merged(keys: &[Option<Rgb>; KEYS], pad: &[Option<Rgb>; KEYS]) -> [Option<Rgb>; KEYS] {
    let mut o = *keys;
    if !crate::hook::NUM.load(std::sync::atomic::Ordering::Relaxed) {
        for (i, c) in pad.iter().enumerate() {
            if c.is_some() && o[i].is_some() {
                o[i] = *c;
            }
        }
    }
    o
}

#[derive(Default, Clone)]
pub struct Status {
    pub started: bool,
    pub keyboard: bool,
    pub body: bool,
    pub fn_lock: bool,
    /// тачпад включён (как его видит Windows)
    pub touchpad: bool,
}

pub fn start(ctx: Option<eframe::egui::Context>) -> (Sender<Cmd>, Arc<Mutex<Status>>) {
    let (tx, rx) = channel::<Cmd>();
    let st = Arc::new(Mutex::new(Status::default()));
    let st2 = st.clone();
    std::thread::spawn(move || run(rx, st2, ctx));
    (tx, st)
}

#[link(name = "advapi32")]
extern "system" {
    fn RegGetValueW(hkey: isize, sub: *const u16, value: *const u16, flags: u32, typ: *mut u32, data: *mut u8, len: *mut u32) -> i32;
}

/// Включён ли тачпад: Windows хранит это в реестре и меняет при Fn+F12.
fn touchpad_on() -> Option<bool> {
    const HKCU: isize = 0x8000_0001u32 as i32 as isize;
    let w = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let sub = w("Software\\Microsoft\\Windows\\CurrentVersion\\PrecisionTouchPad\\Status");
    let name = w("Enabled");
    let mut v: u32 = 0;
    let mut len: u32 = 4;
    let r = unsafe { RegGetValueW(HKCU, sub.as_ptr(), name.as_ptr(), 0x10, std::ptr::null_mut(), &mut v as *mut u32 as *mut u8, &mut len) };
    if r == 0 { Some(v != 0) } else { None }
}

fn run(rx: Receiver<Cmd>, st: Arc<Mutex<Status>>, ctx: Option<eframe::egui::Context>) {
    let kb = Keyboard::open();
    let ch = Chassis::open();
    if let Ok(mut s) = st.lock() {
        s.started = true;
        s.keyboard = kb.is_some();
        s.body = ch.is_some();
    }
    let mut last_status: Option<Vec<u8>> = None;
    let mut react: Option<(u8, u8, Rgb, Box<[Option<Rgb>; KEYS]>)> = None;
    let mut react_idle = false;
    let mut last_poll = std::time::Instant::now();
    // последние свои цвета и цвета цифрового блока — чтобы перекрасить при смене Num Lock
    let mut own: Option<(Box<[Option<Rgb>; KEYS]>, u8)> = None;
    let mut pad: Box<[Option<Rgb>; KEYS]> = Box::new([None; KEYS]);
    let mut num_was = crate::hook::NUM.load(std::sync::atomic::Ordering::Relaxed);
    let mut redo_at: Vec<std::time::Instant> = Vec::new();
    let mut lit: Option<bool> = None;
    loop {
        let wait = if react.is_some() || !redo_at.is_empty() { 33 } else { 150 };
        let first = match rx.recv_timeout(std::time::Duration::from_millis(wait)) {
            Ok(c) => c,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // кадр подсветки «реакция на нажатия»
                if let (Some(kb), Some((k, s, c, bg))) = (&kb, &react) {
                    if crate::reactive::active(*k, *s) {
                        kb.frame(&crate::reactive::frame(*k, *c, *s, bg));
                        react_idle = false;
                    } else if !react_idle {
                        kb.frame(&crate::reactive::frame(*k, *c, *s, bg));
                        react_idle = true;
                    }
                }
                // Num Lock переключили: клавиатура сама перекрашивает цифровой блок —
                // через мгновение заново включаем свои цвета с полной подготовкой
                let num = crate::hook::NUM.load(std::sync::atomic::Ordering::Relaxed);
                if num != num_was {
                    num_was = num;
                    log::write(&format!("Num Lock {}", if num { "включён" } else { "выключен" }));
                    // клавиатура перекрашивает блок не сразу, а с задержкой — повторяем несколько раз
                    let now = std::time::Instant::now();
                    // без вторых цветов ничего не досылаем — клавиатура ведёт себя как обычно
                    if pad.iter().any(|c| c.is_some()) { redo_at = [150u64, 500, 1000, 2000].iter().map(|ms| now + std::time::Duration::from_millis(*ms)).collect(); }
                }
                if let Some(t) = redo_at.first().copied() {
                    if std::time::Instant::now() >= t {
                        redo_at.remove(0);
                        // только досылаем цвета клавиш, без перезапуска — иначе мигает вся клавиатура
                        if let (Some(kb), Some((k, _))) = (&kb, &own) {
                            kb.frame(&merged(k, &pad));
                        }
                    }
                }
                if last_poll.elapsed() < std::time::Duration::from_millis(150) {
                    continue;
                }
                last_poll = std::time::Instant::now();
                if let Some(tp) = touchpad_on() {
                    if let Ok(mut stt) = st.lock() {
                        if stt.touchpad != tp {
                            stt.touchpad = tp;
                            log::write(&format!("Тачпад {}", if tp { "включён" } else { "выключен" }));
                            if let Some(c) = &ctx {
                                        c.request_repaint();
                                    }
                        }
                    }
                }
                // следим за клавишами Fn: погасла клавиатура — гасим и корпус
                if let Some(kb) = &kb {
                    if let Some(s) = kb.status() {
                        if let Some(prev) = &last_status {
                            let diff: Vec<String> = (0..s.len().min(prev.len())).filter(|&i| s[i] != prev[i]).map(|i| format!("[{i}] {:02X}→{:02X}", prev[i], s[i])).collect();
                            if !diff.is_empty() {
                                log::write(&format!("Клавиатура: состояние изменилось {}", diff.join(" ")));
                            }
                        } else {
                            log::write(&format!("Клавиатура: состояние {}", log::hex(&s)));
                        }
                        // ответ без заголовка CC 94 — пропускаем, чтобы не мигало
                        if s.len() < 22 || s[0] != 0xCC || s[1] != 0x94 {
                            continue;
                        }
                        // Fn Lock: байт 11 меняется AA ↔ 55 при Fn+Esc
                        if s[11] == 0x55 || s[11] == 0xAA {
                            let fl = s[11] == 0x55;
                            if let Ok(mut stt) = st.lock() {
                                if stt.fn_lock != fl {
                                    stt.fn_lock = fl;
                                    log::write(&format!("Fn Lock {}", if fl { "включён" } else { "выключен" }));
                                    if let Some(c) = &ctx {
                                        c.request_repaint();
                                    }
                                }
                            }
                        }
                        let on = s[21] > 0;
                        if lit != Some(on) {
                            if lit.is_some() || !on {
                                if let Some(ch) = &ch {
                                    ch.all_off(!on);
                                }
                            }
                            lit = Some(on);
                        }
                        last_status = Some(s);
                    }
                }
                continue;
            }
            Err(_) => break,
        };
        // берём только самые свежие команды, промежуточные пропускаем
        let mut keys = None;
        let mut body = None;
        let mut save = None;
        let mut effect = None;
        let mut newreact = None;
        let mut newpad = None;
        let mut take = |c: Cmd| match c {
            Cmd::Numpad(p) => newpad = Some(p),
            Cmd::Keys(k, b) => {
                effect = None;
                newreact = Some(None);
                keys = Some((k, b))
            }
            Cmd::Effect(k, s, c, d) => {
                keys = None;
                newreact = Some(None);
                effect = Some((k, s, c, d))
            }
            Cmd::Reactive(k, s, c, bg) => {
                keys = None;
                effect = None;
                newreact = Some(Some((k, s, c, bg)))
            }
            Cmd::Body(e, c, b) => body = Some((e, c, b)),
            Cmd::Save(k, e, c, b) => save = Some((k, e, c, b)),
        };
        take(first);
        while let Ok(c) = rx.try_recv() {
            take(c);
        }
        if let Some(p) = newpad {
            pad = p;
            if keys.is_none() && effect.is_none() && react.is_none() && newreact.is_none() {
                if let (Some(kb), Some((k, b))) = (&kb, &own) {
                    kb.apply(&merged(k, &pad), *b);
                }
            }
        }
        if let Some(r) = newreact {
            own = None;
            if let Some((k, ..)) = &r {
                log::write(&format!("Клавиатура: реакция на нажатия, вид {k}"));
            }
            react = r;
            react_idle = false;
        }
        if let (Some(kb), Some((k, b))) = (&kb, &keys) {
            kb.apply(&merged(k, &pad), *b);
        }
        if let Some((k, b)) = &keys {
            own = Some((k.clone(), *b));
        }
        if effect.is_some() {
            own = None;
        }
        if let (Some(kb), Some((k, s, c, d))) = (&kb, effect) {
            kb.effect(k, s, c, d);
        }
        if let (Some(ch), Some((e, c, b))) = (&ch, &body) {
            ch.apply(*e, *c, *b);
        }
        if let Some((k, e, c, b)) = save {
            if let Some(kb) = &kb {
                if let (None, Some(k)) = (&keys, &k) {
                    kb.apply(&merged(k, &pad), b);
                }
                // при «реакции на нажатия» в памяти клавиатуры остаются прежние свои цвета
                if react.is_none() {
                    kb.save();
                }
            }
            if let Some(ch) = &ch {
                ch.save(e, c);
                let _ = b;
            }
            log::write("Цвета сохранены");
        }
    }
}
