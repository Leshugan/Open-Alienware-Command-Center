//! Клавиатура m18 R2 с подсветкой каждой клавиши (контроллер 0D62:AAB1).
//! Команды повторяют то, что делает Alienware Command Center.
use crate::calib;
use crate::hid::HidDevice;
use crate::log;
use std::thread::sleep;
use std::time::Duration;

pub const KEYS: usize = 140;
pub type Rgb = [u8; 3];

pub struct Keyboard {
    dev: HidDevice,
    len: usize,
    pub layout: u8,
    ready: std::cell::Cell<bool>,
    last_mask: std::cell::RefCell<Option<[u8; KEYS]>>,
}

impl Keyboard {
    pub fn open() -> Option<Keyboard> {
        for pid in [0xAAB1u16, 0xAAB2, 0xAAB0] {
            let list = HidDevice::find(0x0D62, pid);
            for d in &list {
                log::write(&format!("Клавиатура: найдено {:04X}:{:04X} usage={:04X}/{:02X} feature={} путь={}", 0x0D62, pid, d.usage_page, d.usage, d.feature_len, d.path));
            }
            if let Some(dev) = list.into_iter().find(|d| d.usage_page == 0xFF89 && d.usage == 0xCC && d.feature_len >= 64) {
                let len = dev.feature_len;
                let mut kb = Keyboard { dev, len, layout: 17, ready: std::cell::Cell::new(false), last_mask: std::cell::RefCell::new(None) };
                if let Some(info) = kb.read(0x93) {
                    kb.layout = info[3];
                    log::write(&format!("Клавиатура: сведения {}", log::hex(&info)));
                }
                log::write(&format!("Клавиатура подключена, длина пакета {len}, раскладка {}", kb.layout));
                return Some(kb);
            }
        }
        log::write("Клавиатура не найдена");
        None
    }

    fn packet(&self, head: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; self.len];
        b[..head.len()].copy_from_slice(head);
        b
    }

    fn write(&self, b: &[u8]) -> bool {
        let ok = self.dev.set_feature(b);
        if !ok {
            log::write(&format!("Клавиатура: запись не прошла ({}) {}", HidDevice::last_error(), log::hex(b)));
        }
        ok
    }

    /// Запрос: отправить команду и прочитать ответ.
    pub fn read(&self, cmd: u8) -> Option<Vec<u8>> {
        let mut b = self.packet(&[0xCC, cmd]);
        if !self.dev.set_feature(&b) {
            log::write(&format!("Клавиатура: запрос {cmd:02X} не отправлен ({})", HidDevice::last_error()));
            return None;
        }
        sleep(Duration::from_millis(20));
        b.iter_mut().for_each(|x| *x = 0);
        b[0] = 0xCC;
        if self.dev.get_feature(&mut b) {
            Some(b)
        } else {
            log::write(&format!("Клавиатура: ответ на {cmd:02X} не получен ({})", HidDevice::last_error()));
            None
        }
    }

    fn calibrate(&self, id: usize, c: Rgb) -> Rgb {
        let (r, g, b, w) = match self.layout {
            1 => (&calib::RED_UK, &calib::GREEN_UK, &calib::BLUE_UK, &calib::WHITE_UK),
            16 => (&calib::RED_JP, &calib::GREEN_JP, &calib::BLUE_JP, &calib::WHITE_JP),
            _ => (&calib::RED_US, &calib::GREEN_US, &calib::BLUE_US, &calib::WHITE_US),
        };
        if id >= KEYS || c == [0, 0, 0] {
            return c;
        }
        let f = |v: u8, k: f32| (v as f32 * k).clamp(0.0, 255.0) as u8;
        // Точно как в AWCC: чистый красный/зелёный/синий — своя поправка цвета,
        // любой смешанный цвет (голубой, жёлтый, белый…) — поправка белого.
        match (c[0] != 0, c[1] != 0, c[2] != 0) {
            (true, false, false) => [f(c[0], r[id]), 0, 0],
            (false, true, false) => [0, f(c[1], g[id]), 0],
            (false, false, true) => [0, 0, f(c[2], b[id])],
            _ => {
                let wk = w[id];
                [f(c[0], wk[1] * wk[0] / 255.0), f(c[1], wk[2] * wk[0] / 255.0), f(c[2], wk[3] * wk[0] / 255.0)]
            }
        }
    }

    /// Зажечь клавиши заданными цветами (None — клавиша не трогается и гаснет).
    pub fn apply(&self, colors: &[Option<Rgb>; KEYS], brightness: u8) -> bool {
        // Подготовка (режим своих цветов, маска клавиш) — только когда что-то поменялось,
        // чтобы ползунки и выбор цвета работали плавно.
        let mut mask = [0u8; KEYS];
        for (i, c) in colors.iter().enumerate() {
            if c.is_some() {
                mask[i] = 1;
            }
        }
        let first_time = !self.ready.get();
        let mut level = None;
        if first_time {
            let st = self.read(0x94);
            level = st.as_ref().map(|s| s[21]);
            let custom = st.map(|s| s[2] == 0x8C).unwrap_or(false);
            if !custom {
                self.write(&self.packet(&[0xCC, 0x8C, 0x10]));
                for k in [1u8, 2, 5, 8, 9, 14] {
                    self.write(&self.packet(&[0xCC, 0x8C, 0x01, k]));
                }
            }
        }
        if first_time || self.last_mask.borrow().as_ref() != Some(&mask) {
            for (part, range) in [(5u8, 0..60), (6, 60..120), (7, 120..140)] {
                let mut p = self.packet(&[0xCC, 0x8C, part, 0]);
                for (j, i) in range.enumerate() {
                    p[4 + j] = mask[i];
                }
                self.write(&p);
            }
            let first = colors.iter().flatten().next().copied().unwrap_or([0, 0, 0]);
            let mut cfg = self.packet(&[0xCC, 0x8C, 0x01, 0x01, 0x01, 0, 0, 1, 1, 1, 0]);
            cfg[11..14].copy_from_slice(&first);
            cfg[14..17].copy_from_slice(&first);
            cfg[17] = 1;
            self.write(&cfg);
            *self.last_mask.borrow_mut() = Some(mask);
        }

        let mut quads: Vec<u8> = Vec::new();
        for (i, c) in colors.iter().enumerate() {
            if let Some(c) = c {
                let cc = self.calibrate(i, *c);
                quads.extend_from_slice(&[(i + 1) as u8, cc[0], cc[1], cc[2]]);
            }
        }
        let mut ok = true;
        for chunk in quads.chunks(60) {
            let mut p = self.packet(&[0xCC, 0x8C, 0x02, 0]);
            p[4..4 + chunk.len()].copy_from_slice(chunk);
            ok &= self.write(&p);
        }
        self.write(&self.packet(&[0xCC, 0x8C, 0x13]));
        if first_time {
            // яркость оставляем ту, что сейчас стоит (её меняют клавиши Fn)
            match level {
                Some(v) => {
                    self.write(&self.packet(&[0xCC, 0x8B, 0x01, v]));
                    self.write(&self.packet(&[0xCC, 0x83, 0x38, 0x9C, (v as u32 * 254 / 255) as u8]));
                }
                _ => self.set_brightness(brightness),
            }
            self.ready.set(true);
        }
        let mut seen: Vec<Rgb> = Vec::new();
        let mut shown: Vec<String> = Vec::new();
        for (i, c) in colors.iter().enumerate() {
            if let Some(c) = c {
                if !seen.contains(c) && seen.len() < 4 {
                    seen.push(*c);
                    let cc = self.calibrate(i, *c);
                    shown.push(format!("{} → {}", crate::state::to_hex(*c), crate::state::to_hex(cc)));
                }
            }
        }
        log::write(&format!("Клавиатура: цвета отправлены ({} клавиш), яркость {brightness}%, цвета: {}", quads.len() / 4, shown.join(", ")));
        ok
    }

    /// Встроенный эффект на всю клавиатуры (работает сам, без программы).
    /// kind: 7 дыхание, 8 спектр, 16 радужная волна, 17 сканер; speed: 0 медленно … 2 быстро.
    pub fn effect(&self, kind: u8, speed: u8, c: Rgb, dir: u8) -> bool {
        let st = self.read(0x94);
        let custom = st.as_ref().map(|s| s[2] == 0x8C).unwrap_or(false);
        let level = st.as_ref().map(|s| s[21]).unwrap_or(255);
        if custom {
            self.write(&self.packet(&[0xCC, 0x8C, 0x10]));
            for k in [1u8, 2, 5, 8, 9, 14] {
                self.write(&self.packet(&[0xCC, 0x8C, 0x01, k]));
            }
        }
        // коды и скорости — как в AWCC (от быстрого к медленному)
        let (code, flag, sp): (u8, u8, [(u8, u8); 3]) = match kind {
            7 => (2, 0, [(1, 2), (7, 5), (9, 6)]),
            8 => (14, 1, [(2, 4), (6, 1), (7, 2)]),
            16 => (3, 1, [(2, 2), (5, 5), (9, 6)]),
            17 => (10, 0, [(4, 5), (7, 6), (15, 7)]),
            _ => return false,
        };
        let (a, b) = sp[2 - speed.min(2) as usize];
        let col = if kind == 8 || kind == 16 { [255, 0, 0] } else { c };
        let mut p = self.packet(&[0xCC, 0x80, code, a, 0, 0, if kind == 16 { dir.clamp(1, 4) } else { 1 }, 1, 1, flag]);
        p[10..13].copy_from_slice(&col);
        p[13..16].copy_from_slice(&col);
        p[16] = b;
        let ok = self.write(&p);
        self.write(&self.packet(&[0xCC, 0x8B, 0x01, level]));
        self.write(&self.packet(&[0xCC, 0x83, 0x38, 0x9C, (level as u32 * 254 / 255) as u8]));
        // после эффекта свои цвета надо будет включать заново
        self.ready.set(false);
        *self.last_mask.borrow_mut() = None;
        log::write(&format!("Клавиатура: эффект {kind}, скорость {speed}, цвет {}, направление {dir} — {}", crate::state::to_hex(col), if ok { "ок" } else { "ошибка" }));
        ok
    }

    /// Быстрый кадр для подсветки «реакция на нажатия»: без записи в журнал.
    pub fn frame(&self, colors: &[Option<Rgb>; KEYS]) {
        let mut mask = [0u8; KEYS];
        for (i, c) in colors.iter().enumerate() {
            if c.is_some() {
                mask[i] = 1;
            }
        }
        if !self.ready.get() || self.last_mask.borrow().as_ref() != Some(&mask) {
            self.apply(colors, 100);
            return;
        }
        let mut quads: Vec<u8> = Vec::new();
        for (i, c) in colors.iter().enumerate() {
            if let Some(c) = c {
                let cc = self.calibrate(i, *c);
                quads.extend_from_slice(&[(i + 1) as u8, cc[0], cc[1], cc[2]]);
            }
        }
        for chunk in quads.chunks(60) {
            let mut p = self.packet(&[0xCC, 0x8C, 0x02, 0]);
            p[4..4 + chunk.len()].copy_from_slice(chunk);
            self.dev.set_feature(&p);
        }
        self.dev.set_feature(&self.packet(&[0xCC, 0x8C, 0x13]));
    }

    pub fn set_brightness(&self, percent: u8) {
        let p = percent.min(100) as u32;
        let mut v = (255 * p / 100) as u8;
        if v == 254 {
            v = 255;
        }
        self.write(&self.packet(&[0xCC, 0x8B, 0x01, v]));
        let mut v2 = (254 * p / 100) as u8;
        if p >= 100 {
            v2 = 254;
        }
        self.write(&self.packet(&[0xCC, 0x83, 0x38, 0x9C, v2]));
    }

    /// Состояние клавиатуры (в том числе яркость, которую меняют клавиши Fn).
    pub fn status(&self) -> Option<Vec<u8>> {
        let mut b = self.packet(&[0xCC, 0x94]);
        if !self.dev.set_feature(&b) {
            return None;
        }
        sleep(Duration::from_millis(15));
        b.iter_mut().for_each(|x| *x = 0);
        b[0] = 0xCC;
        if self.dev.get_feature(&mut b) { Some(b) } else { None }
    }

    /// Запомнить текущие цвета в самой клавиатуре (сохранятся после перезагрузки).
    pub fn save(&self) {
        self.write(&self.packet(&[0xCC, 0x84, 0x03, 0x00]));
        log::write("Клавиатура: цвета сохранены в память клавиатуры");
    }
}
