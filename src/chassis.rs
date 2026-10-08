//! Подсветка корпуса: эмблема на крышке и световой контур сзади (контроллер 187C:0551).
use crate::hid::HidDevice;
use crate::log;
use std::thread::sleep;
use std::time::Duration;

pub type Rgb = [u8; 3];

/// Номера ламп в контроллере.
pub const EMBLEM: &[u8] = &[2];
pub const CONTOUR: &[u8] = &[0, 1];
const PLAY_ID: u16 = 0xFFFF;
const DEFAULT_ID: u16 = 97;

pub struct Chassis {
    dev: HidDevice,
    len: usize,
    ready: std::cell::Cell<bool>,
}

impl Chassis {
    pub fn open() -> Option<Chassis> {
        let list = HidDevice::find(0x187C, 0x0551);
        for d in &list {
            log::write(&format!("Корпус: найдено usage={:04X}/{:02X} out={} in={} путь={}", d.usage_page, d.usage, d.output_len, d.input_len, d.path));
        }
        let dev = list.into_iter().max_by_key(|d| d.output_len)?;
        let len = dev.output_len.max(34);
        log::write(&format!("Корпус подключён, длина пакета {len}"));
        Some(Chassis { dev, len, ready: std::cell::Cell::new(false) })
    }

    fn send(&self, body: &[u8]) -> Option<Vec<u8>> {
        let mut p = vec![0u8; self.len];
        p[1..1 + body.len()].copy_from_slice(body);
        if !self.dev.set_output(&p) {
            log::write(&format!("Корпус: запись не прошла ({}) {}", HidDevice::last_error(), log::hex(&p)));
            return None;
        }
        sleep(Duration::from_millis(10));
        let mut r = vec![0u8; self.dev.input_len.max(self.len)];
        if self.dev.get_input(&mut r) {
            Some(r)
        } else {
            log::write(&format!("Корпус: ответ не получен ({})", HidDevice::last_error()));
            None
        }
    }

    fn anim(&self, id: u16, sub: u16, dur: u16) -> bool {
        let code = if (91..=96).contains(&id) { 0x22 } else { 0x21 };
        let b = [0x03, code, (sub >> 8) as u8, sub as u8, (id >> 8) as u8, id as u8, (dur >> 8) as u8, dur as u8];
        match self.send(&b) {
            Some(r) => r[7] == 0,
            None => false,
        }
    }

    fn series(&self, leds: &[u8]) -> bool {
        let mut b = vec![0x03, 0x23, 1, 0, leds.len() as u8];
        b.extend_from_slice(leds);
        matches!(self.send(&b), Some(r) if r[3] == 0)
    }

    fn color_action(&self, c: Rgb) -> bool {
        // эффект «цвет», длительность 2000, темп 250 — как у AWCC
        let b = [0x03, 0x24, 0, 0x07, 0xD0, 0x00, 0xFA, c[0], c[1], c[2]];
        matches!(self.send(&b), Some(r) if r[3] == 0)
    }

    fn build(&self, id: u16, finish: u16, groups: &[(&[u8], Rgb)]) -> bool {
        let mut ok = self.anim(id, 1, 0);
        for (leds, c) in groups {
            ok &= self.series(leds);
            ok &= self.color_action(*c);
        }
        ok &= self.anim(id, finish, 0);
        ok
    }

    /// Показать цвета сразу.
    pub fn apply(&self, emblem: Rgb, contour: Rgb, brightness: u8) -> bool {
        let ok = self.build(PLAY_ID, 3, &[(EMBLEM, emblem), (CONTOUR, contour)]);
        if !self.ready.get() {
            self.set_brightness(brightness);
            self.ready.set(true);
        }
        log::write(&format!("Корпус: эмблема {:02X?}, контур {:02X?}, яркость {brightness}% — {}", emblem, contour, if ok { "ок" } else { "ошибка" }));
        ok
    }

    pub fn set_brightness(&self, percent: u8) {
        let dim = 100 - percent.min(100);
        let leds = [0u8, 1, 2, 3];
        let mut b = vec![0x03, 0x26, dim, 0, leds.len() as u8];
        b.extend_from_slice(&leds);
        self.send(&b);
    }

    /// Погасить или вернуть всю подсветку корпуса, включая кнопку питания.
    pub fn all_off(&self, off: bool) {
        let dim = if off { 100 } else { 0 };
        let leds = [0u8, 1, 2, 3, 4];
        let mut b = vec![0x03, 0x26, dim, 0, leds.len() as u8];
        b.extend_from_slice(&leds);
        let ok = self.send(&b).is_some();
        log::write(&format!("Корпус: подсветка {} — {}", if off { "погашена" } else { "включена" }, if ok { "ок" } else { "ошибка" }));
    }

    /// Сохранить как цвета по умолчанию (после перезагрузки останутся).
    pub fn save(&self, emblem: Rgb, contour: Rgb) {
        self.anim(DEFAULT_ID, 4, 0);
        let ok = self.build(DEFAULT_ID, 2, &[(EMBLEM, emblem), (CONTOUR, contour)]);
        let ok2 = self.anim(DEFAULT_ID, 6, 0);
        log::write(&format!("Корпус: сохранение цветов по умолчанию — {}", if ok && ok2 { "ок" } else { "ошибка" }));
    }
}
