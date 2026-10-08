//! Перехват клавиш: у выбранных клавиш с двумя функциями меняем функции местами
//! (например, HOME работает как PRT SCR, а PRT SCR — как HOME).
use std::sync::atomic::{AtomicU32, Ordering};

/// Биты — номера клавиш верхнего ряда (13 HOME, 14 END, 15 DEL), у которых функции поменяны.
pub static SWAPPED: AtomicU32 = AtomicU32::new(0);
/// Включена подсветка «реакция на нажатия» — тогда запоминаем нажатия.
pub static REACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DOWN: std::sync::Mutex<[bool; 256]> = std::sync::Mutex::new([false; 256]);
/// Включён ли Num Lock / Caps Lock (следим по нажатиям — так верно, даже когда окно не в фокусе).
pub static NUM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
pub static CAPS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LOCK_DOWN: std::sync::Mutex<[bool; 2]> = std::sync::Mutex::new([false; 2]);

/// Клавиши, которые умеем переключать, и пары их кодов Windows.
pub const PAIRS: &[(u8, u16, u16)] = &[
    (13, 0x24, 0x2C), // HOME ↔ PRT SCR
    (14, 0x23, 0x13), // END ↔ PAUSE
    (15, 0x2E, 0x2D), // DEL ↔ INSERT
];

pub fn can_swap(id: u8) -> bool {
    PAIRS.iter().any(|p| p.0 == id)
}

pub fn is_swapped(id: u8) -> bool {
    SWAPPED.load(Ordering::Relaxed) & (1 << id) != 0
}

pub fn set(ids: &[u8]) {
    let mut m = 0u32;
    for i in ids {
        m |= 1 << i;
    }
    SWAPPED.store(m, Ordering::Relaxed);
}

#[repr(C)]
struct KbdLl {
    vk: u32,
    scan: u32,
    flags: u32,
    time: u32,
    extra: usize,
}

#[repr(C)]
struct KeybdInput {
    vk: u16,
    scan: u16,
    flags: u32,
    time: u32,
    extra: usize,
}

#[repr(C)]
struct Input {
    kind: u32,
    ki: KeybdInput,
    _pad: [u8; 8],
}

#[repr(C)]
struct Msg {
    hwnd: isize,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    pt: [i32; 2],
    private: u32,
}

#[link(name = "user32")]
extern "system" {
    fn SetWindowsHookExW(id: i32, f: extern "system" fn(i32, usize, isize) -> isize, hmod: isize, tid: u32) -> isize;
    fn CallNextHookEx(h: isize, code: i32, w: usize, l: isize) -> isize;
    fn GetMessageW(m: *mut Msg, hwnd: isize, a: u32, b: u32) -> i32;
    fn SendInput(n: u32, inputs: *const Input, size: i32) -> u32;
    fn MapVirtualKeyW(code: u32, map: u32) -> u32;
    fn GetKeyState(vk: i32) -> i16;
}

const TAG: usize = 0xA11E_C0DE;
const LLKHF_EXTENDED: u32 = 0x01;
const LLKHF_INJECTED: u32 = 0x10;
const LLKHF_UP: u32 = 0x80;

fn send(vk: u16, up: bool) {
    // HOME, END, DEL, INSERT, PRT SCR — «расширенные» клавиши; PAUSE — нет
    let ext = vk != 0x13;
    let scan = unsafe { MapVirtualKeyW(vk as u32, 0) } as u16;
    let flags = if ext { 0x1 } else { 0 } | if up { 0x2 } else { 0 };
    let i = Input { kind: 1, ki: KeybdInput { vk, scan, flags, time: 0, extra: TAG }, _pad: [0; 8] };
    unsafe { SendInput(1, &i, std::mem::size_of::<Input>() as i32) };
}

extern "system" fn proc(code: i32, w: usize, l: isize) -> isize {
    if code == 0 {
        let k = unsafe { &*(l as *const KbdLl) };
        let mine = k.flags & LLKHF_INJECTED != 0 && k.extra == TAG;
        if k.vk == 0x90 || k.vk == 0x14 {
            let up = k.flags & LLKHF_UP != 0;
            let n = (k.vk == 0x14) as usize;
            if let Ok(mut d) = LOCK_DOWN.lock() {
                if !up && !d[n] {
                    let f = if n == 0 { &NUM } else { &CAPS };
                    f.store(!f.load(Ordering::Relaxed), Ordering::Relaxed);
                }
                d[n] = !up;
            }
        }
        if !mine && REACTIVE.load(Ordering::Relaxed) {
            let up = k.flags & LLKHF_UP != 0;
            if let Ok(mut d) = DOWN.lock() {
                let i = (k.vk & 0xFF) as usize;
                // удержание клавиши (автоповтор) — не новое нажатие
                if !up && !d[i] {
                    if let Some(led) = crate::reactive::led_for(k.vk, k.flags & LLKHF_EXTENDED != 0) {
                        crate::reactive::press(led);
                    }
                }
                d[i] = !up;
            }
        }
        if !mine && SWAPPED.load(Ordering::Relaxed) != 0 {
            let vk = k.vk as u16;
            for &(id, a, b) in PAIRS {
                if !is_swapped(id) || (vk != a && vk != b) {
                    continue;
                }
                // HOME/END/DEL/INSERT цифрового блока не трогаем — у них нет признака «расширенная»
                if vk != 0x2C && vk != 0x13 && k.flags & LLKHF_EXTENDED == 0 {
                    continue;
                }
                let other = if vk == a { b } else { a };
                send(other, k.flags & LLKHF_UP != 0 || w == 0x101 || w == 0x105);
                return 1;
            }
        }
    }
    unsafe { CallNextHookEx(0, code, w, l) }
}

pub fn start() {
    unsafe {
        NUM.store(GetKeyState(0x90) & 1 != 0, Ordering::Relaxed);
        CAPS.store(GetKeyState(0x14) & 1 != 0, Ordering::Relaxed);
    }
    std::thread::spawn(|| unsafe {
        let h = SetWindowsHookExW(13, proc, 0, 0);
        crate::log::write(&format!("Перехват клавиш: {}", if h != 0 { "включён" } else { "не удалось включить" }));
        let mut m: Msg = std::mem::zeroed();
        while GetMessageW(&mut m, 0, 0, 0) > 0 {}
    });
}
