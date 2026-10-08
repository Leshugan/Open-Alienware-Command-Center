//! Подсветка, которая отвечает на нажатия (как в клавиатурах Keychron / прошивке QMK).
//! Работает, пока запущена программа: перехват клавиш записывает нажатия,
//! а фоновый поток ~30 раз в секунду рисует кадр и отправляет его на клавиатуру.
use crate::keyboard::{Rgb, KEYS};
use std::sync::Mutex;
use std::time::Instant;

pub const SIMPLE: u8 = 100;
pub const WIDE: u8 = 101;
pub const CROSS: u8 = 102;
pub const NEXUS: u8 = 103;
pub const SPLASH: u8 = 104;
pub const SOLID_SPLASH: u8 = 105;
pub const HEATMAP: u8 = 106;

pub fn is_reactive(kind: u8) -> bool {
    (SIMPLE..=HEATMAP).contains(&kind)
}

struct Hit {
    led: u8,
    t: Instant,
}

struct Hits {
    list: Vec<Hit>,
    heat: [f32; KEYS],
    heat_t: Option<Instant>,
}

static HITS: Mutex<Hits> = Mutex::new(Hits { list: Vec::new(), heat: [0.0; KEYS], heat_t: None });

/// Нажата клавиша `led` (вызывается из перехвата клавиш или кликом по клавише в окне).
pub fn press(led: u8) {
    if let Ok(mut h) = HITS.lock() {
        h.list.push(Hit { led, t: Instant::now() });
        if h.list.len() > 24 {
            h.list.remove(0);
        }
        let i = led as usize;
        if i < KEYS {
            h.heat[i] = (h.heat[i] + 0.22).min(1.0);
            // соседи тоже немного нагреваются
            let pos = positions();
            if let Some(p) = pos[i] {
                for (j, q) in pos.iter().enumerate() {
                    if let Some(q) = q {
                        let d = ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt();
                        if j != i && d < 1.3 {
                            h.heat[j] = (h.heat[j] + 0.06).min(1.0);
                        }
                    }
                }
            }
        }
    }
}

/// Нажатия и «нагрев» для окна (предпросмотр): (клавиша, сколько мс назад).
pub fn export() -> (Vec<(u8, u32)>, Vec<(u8, f32)>) {
    let Ok(h) = HITS.lock() else { return (Vec::new(), Vec::new()) };
    let hits = h.list.iter().map(|x| (x.led, x.t.elapsed().as_millis().min(60_000) as u32)).collect();
    let heat = h.heat.iter().enumerate().filter(|(_, v)| **v > 0.0).map(|(i, v)| (i as u8, *v)).collect();
    (hits, heat)
}

/// Принять нажатия из фоновой части (в окне).
pub fn import(hits: &[(u8, u32)], heat: &[(u8, f32)]) {
    let Ok(mut h) = HITS.lock() else { return };
    let now = Instant::now();
    h.list = hits.iter().map(|(led, ms)| Hit { led: *led, t: now - std::time::Duration::from_millis(*ms as u64) }).collect();
    h.heat = [0.0; KEYS];
    for (i, v) in heat {
        if (*i as usize) < KEYS {
            h.heat[*i as usize] = *v;
        }
    }
    h.heat_t = Some(now);
}

/// Положение каждой клавиши в «клавишах»: x — по горизонтали, y — номер ряда.
fn positions() -> &'static [Option<(f32, f32)>; KEYS] {
    static P: std::sync::OnceLock<[Option<(f32, f32)>; KEYS]> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        let mut a = [None; KEYS];
        for k in crate::layout::keys() {
            let x = ((k.x0 + k.x1) / 2.0 - crate::layout::LEFT) / 85.0;
            let y = k.row as f32 + if k.tall { 0.5 } else { 0.0 };
            a[k.id as usize] = Some((x, y));
        }
        a
    })
}

fn hue(h: f32) -> Rgb {
    let h = h.rem_euclid(1.0) * 6.0;
    let x = 1.0 - ((h % 2.0) - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

fn mix(a: Rgb, b: Rgb, v: f32) -> Rgb {
    let v = v.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (a[i] as f32 * (1.0 - v) + b[i] as f32 * v) as u8)
}

/// Кадр: цвет каждой клавиши. `bg` — фон (свои цвета или чёрный), speed 0..2.
pub fn frame(kind: u8, color: Rgb, speed: u8, bg: &[Option<Rgb>; KEYS]) -> [Option<Rgb>; KEYS] {
    let now = Instant::now();
    let sp = [0.6_f32, 1.0, 1.6][speed.min(2) as usize];
    let life = 1.6 / sp; // сколько секунд живёт след нажатия
    let pos = positions();
    let mut out = [None; KEYS];
    let Ok(mut h) = HITS.lock() else { return out };
    h.list.retain(|x| now.duration_since(x.t).as_secs_f32() < life * 2.5);
    if kind == HEATMAP {
        // остывание
        let dt = h.heat_t.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(0.0);
        h.heat_t = Some(now);
        for v in h.heat.iter_mut() {
            *v = (*v - dt * 0.12 * sp).max(0.0);
        }
    }
    for (i, p) in pos.iter().enumerate() {
        let Some((x, y)) = *p else { continue };
        let base = bg[i].unwrap_or([0, 0, 0]);
        let mut c = base;
        if kind == HEATMAP {
            let v = h.heat[i];
            if v > 0.0 {
                // от синего (холодно) к красному (горячо)
                c = mix(base, hue(0.66 * (1.0 - v)), (v * 3.0).min(1.0));
            }
            out[i] = Some(c);
            continue;
        }
        for hit in &h.list {
            let Some((hx, hy)) = pos[hit.led as usize] else { continue };
            let age = now.duration_since(hit.t).as_secs_f32();
            let (dx, dy) = (x - hx, y - hy);
            let d = (dx * dx + dy * dy).sqrt();
            let fade = (1.0 - age / life).max(0.0);
            let (v, col) = match kind {
                SIMPLE => (if hit.led as usize == i { fade } else { 0.0 }, color),
                WIDE => (fade - d * 0.35, color),
                CROSS => {
                    let dd = if dy.abs() < 0.5 { dx.abs() } else if dx.abs() < 0.6 { dy.abs() } else { 99.0 };
                    (fade - dd * 0.1, color)
                }
                NEXUS => {
                    let dd = if dy.abs() < 0.5 { dx.abs() } else if dx.abs() < 0.6 { dy.abs() } else { 99.0 };
                    let front = age * 9.0 * sp;
                    (((1.0 - (dd - front).abs() * 0.9).max(0.0)) * fade + if dd == 0.0 { fade } else { 0.0 }, color)
                }
                SPLASH | SOLID_SPLASH => {
                    let front = age * 10.0 * sp;
                    let v = (1.0 - (d - front).abs() * 0.7).max(0.0) * fade;
                    (v, if kind == SPLASH { hue(d * 0.12 + age * 0.8) } else { color })
                }
                _ => (0.0, color),
            };
            if v > 0.0 {
                c = mix(c, col, v);
            }
        }
        out[i] = Some(c);
    }
    out
}

/// Идёт ли ещё какой-то след (чтобы не слать кадры впустую).
pub fn active(kind: u8, speed: u8) -> bool {
    let sp = [0.6_f32, 1.0, 1.6][speed.min(2) as usize];
    let Ok(h) = HITS.lock() else { return false };
    if kind == HEATMAP {
        return h.heat.iter().any(|v| *v > 0.0);
    }
    let life = 1.6 / sp;
    h.list.iter().any(|x| x.t.elapsed().as_secs_f32() < life * 1.6)
}

/// Номер лампочки клавиши по коду Windows (vk) и признаку «расширенная» (ext).
pub fn led_for(vk: u32, ext: bool) -> Option<u8> {
    let letters: [(u8, u8); 26] = [
        (b'Q', 42), (b'W', 43), (b'E', 44), (b'R', 45), (b'T', 46), (b'Y', 47), (b'U', 48), (b'I', 49), (b'O', 50), (b'P', 51),
        (b'A', 62), (b'S', 63), (b'D', 64), (b'F', 65), (b'G', 66), (b'H', 67), (b'J', 68), (b'K', 69), (b'L', 70),
        (b'Z', 83), (b'X', 84), (b'C', 85), (b'V', 86), (b'B', 87), (b'N', 88), (b'M', 89),
    ];
    if let Some(&(_, id)) = letters.iter().find(|(c, _)| *c as u32 == vk) {
        return Some(id);
    }
    Some(match (vk, ext) {
        (0x31..=0x39, _) => (vk - 0x31 + 21) as u8,
        (0x30, _) => 30,
        (0x70..=0x7B, _) => (vk - 0x70 + 1) as u8,
        (0x1B, _) => 0,
        (0x24, true) | (0x2C, _) => 13,
        (0x23, true) | (0x13, _) => 14,
        (0x2E, true) | (0x2D, true) => 15,
        (0xAD, _) => 16,
        (0xAE, _) => 17,
        (0xAF, _) => 18,
        (0xC0, _) => 20,
        (0xBD, _) => 31,
        (0xBB, _) => 32,
        (0x08, _) => 35,
        (0x90, _) => 36,
        (0x6F, _) => 37,
        (0x6A, _) => 38,
        (0x6D, _) => 39,
        (0x09, _) => 40,
        (0xDB, _) => 52,
        (0xDD, _) => 53,
        (0xDC, _) => 55,
        (0x67, _) | (0x24, false) => 56,
        (0x68, _) | (0x26, false) => 57,
        (0x69, _) | (0x21, false) => 58,
        (0x6B, _) => 79,
        (0x14, _) => 61,
        (0xBA, _) => 71,
        (0xDE, _) => 72,
        (0x0D, false) => 74,
        (0x0D, true) => 119,
        (0x64, _) | (0x25, false) => 76,
        (0x65, _) | (0x0C, _) => 77,
        (0x66, _) | (0x27, false) => 78,
        (0xA0, _) | (0x10, false) => 81,
        (0xBC, _) => 90,
        (0xBE, _) => 91,
        (0xBF, _) => 92,
        (0xA1, _) => 94,
        (0x26, true) | (0x21, true) => 114,
        (0x61, _) | (0x23, false) => 96,
        (0x62, _) | (0x28, false) => 97,
        (0x63, _) | (0x22, false) => 98,
        (0xA2, _) | (0x11, false) => 100,
        (0x5B, _) => 103,
        (0xA4, _) | (0x12, false) => 104,
        (0x20, _) => 107,
        (0xA5, _) | (0x12, true) => 111,
        (0xA3, _) | (0x11, true) => 112,
        (0x25, true) => 133,
        (0x28, true) | (0x22, true) => 134,
        (0x27, true) => 135,
        (0x60, _) | (0x2D, false) => 117,
        (0x6E, _) | (0x2E, false) => 118,
        _ => return None,
    })
}
