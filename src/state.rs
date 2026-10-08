//! Сохранённые настройки программы (файл рядом с exe) и перенос цветов из AWCC.
use crate::keyboard::{Rgb, KEYS};
use crate::log;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct Scheme {
    pub name: String,
    pub keys: Vec<(u8, String)>,
    pub emblem: String,
    pub contour: String,
}

/// Эффект подсветки клавиатуры. kind 0 — свои цвета (эффект выключен).
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Effect {
    pub kind: u8,
    /// 0 — медленно, 1 — средне, 2 — быстро
    pub speed: u8,
    pub color: String,
    /// направление волны: 1 справа налево, 2 слева направо, 3 снизу вверх, 4 сверху вниз
    pub dir: u8,
    /// для «реакции на нажатия»: под эффектом горят свои цвета (иначе клавиатура тёмная)
    #[serde(default)]
    pub bg: bool,
}

impl Default for Effect {
    fn default() -> Self {
        Effect { kind: 0, speed: 1, color: "00F0F0".into(), dir: 2, bg: false }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct State {
    pub keys: Vec<(u8, String)>,
    pub emblem: String,
    pub contour: String,
    pub brightness: u8,
    #[serde(default)]
    pub key_bri: Vec<(u8, u8)>,
    #[serde(default = "full")]
    pub emblem_bri: u8,
    #[serde(default = "full")]
    pub contour_bri: u8,
    pub my_colors: Vec<String>,
    /// клавиши, у которых две функции поменяны местами (правый клик)
    #[serde(default)]
    pub swapped: Vec<u8>,
    #[serde(default)]
    pub effect: Effect,
    /// последняя открытая вкладка
    #[serde(default)]
    pub tab: u8,
    /// значок батареи в трее
    #[serde(default = "yes")]
    pub tray: bool,
    /// цвета цифрового блока, когда Num Lock выключен (если заданы)
    #[serde(default)]
    pub numpad_off: Vec<(u8, String)>,
    /// отладочная запись на рабочий стол
    #[serde(default)]
    pub debug: bool,
    pub schemes: Vec<Scheme>,
}

impl Default for State {
    fn default() -> Self {
        State {
            keys: Vec::new(),
            emblem: "00F0F0".into(),
            contour: "00F0F0".into(),
            brightness: 100,
            key_bri: Vec::new(),
            emblem_bri: 100,
            contour_bri: 100,
            my_colors: vec!["FF0000".into(), "FF6A00".into(), "FFFF00".into(), "00FF00".into(), "00F0F0".into(), "0000FF".into(), "8000FF".into(), "FF00FF".into(), "FFFFFF".into()],
            swapped: Vec::new(),
            effect: Effect::default(),
            tab: 0,
            tray: true,
            numpad_off: Vec::new(),
            debug: false,
            schemes: Vec::new(),
        }
    }
}

fn yes() -> bool {
    true
}

fn full() -> u8 {
    100
}

fn shade(c: Rgb, bri: u8) -> Rgb {
    let k = bri.min(100) as f32 / 100.0;
    [(c[0] as f32 * k) as u8, (c[1] as f32 * k) as u8, (c[2] as f32 * k) as u8]
}

pub fn to_hex(c: Rgb) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

pub fn from_hex(s: &str) -> Option<Rgb> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn file() -> std::path::PathBuf {
    let dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| ".".into());
    let new = dir.join("Open Alienware Command Center.json");
    // настройки от прежнего названия программы переносим
    let old = dir.join("AlienCenter.json");
    if !new.exists() && old.exists() {
        let _ = std::fs::rename(&old, &new);
    }
    new
}

impl State {
    pub fn load() -> State {
        if let Ok(s) = std::fs::read_to_string(file()) {
            if let Ok(st) = serde_json::from_str::<State>(&s) {
                log::write("Настройки загружены");
                return st;
            }
        }
        let mut st = State::default();
        if import_awcc(&mut st) {
            log::write("Первый запуск: цвета взяты из настроек AWCC");
        } else {
            log::write("Первый запуск: настроек AWCC нет, цвета по умолчанию");
            st.keys = crate::layout::all_ids().into_iter().map(|i| (i, "00F0F0".to_string())).collect();
        }
        st
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(file(), s);
        }
    }

    pub fn key_colors(&self) -> [Option<Rgb>; KEYS] {
        let mut a = [None; KEYS];
        for (id, h) in &self.keys {
            if (*id as usize) < KEYS {
                a[*id as usize] = from_hex(h);
            }
        }
        a
    }

    /// Цвета клавиш с учётом яркости каждой — то, что уходит на клавиатуру.
    pub fn keys_out(&self) -> [Option<Rgb>; KEYS] {
        let mut o = self.key_colors();
        let b = self.key_bri();
        for (i, c) in o.iter_mut().enumerate() {
            if let Some(c) = c {
                *c = shade(*c, b[i]);
            }
        }
        o
    }

    /// Цвета цифрового блока при выключенном Num Lock (с яркостью).
    pub fn pad_out(&self) -> [Option<Rgb>; KEYS] {
        let mut o = [None; KEYS];
        let b = self.key_bri();
        for (id, h) in &self.numpad_off {
            let i = *id as usize;
            if i < KEYS && crate::layout::is_numpad(*id) {
                o[i] = from_hex(h).map(|c| shade(c, b[i]));
            }
        }
        o
    }

    pub fn key_bri(&self) -> [u8; KEYS] {
        let mut a = [100u8; KEYS];
        for (id, b) in &self.key_bri {
            if (*id as usize) < KEYS {
                a[*id as usize] = (*b).min(100);
            }
        }
        a
    }

    pub fn set_key_bri(&mut self, a: &[u8; KEYS]) {
        self.key_bri = a.iter().enumerate().filter(|(_, b)| **b != 100).map(|(i, b)| (i as u8, *b)).collect();
    }

    pub fn set_key_colors(&mut self, a: &[Option<Rgb>; KEYS]) {
        self.keys = a.iter().enumerate().filter_map(|(i, c)| c.map(|c| (i as u8, to_hex(c)))).collect();
    }
}

/// Прочитать последние цвета, которые выставлял AWCC.
fn import_awcc(st: &mut State) -> bool {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let db = std::path::Path::new(&base).join("Alienware").join("Alienware Command Center").join("FX").join("FXRepository.db");
    let Ok(conn) = rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) else { return false };
    let Ok(mut q) = conn.prepare("SELECT DeviceInstanceId, DataJson FROM PresetDetailInfo p JOIN ActivePresets a ON a.PresetId = p.PresetId AND a.DeviceId = p.DeviceInstanceId") else { return false };
    let rows: Vec<(i64, String)> = match q.query_map([], |r| Ok((r.get(0)?, r.get::<_, String>(1)?))) {
        Ok(it) => it.flatten().collect(),
        Err(_) => return false,
    };
    let mut found = false;
    let mut keys = [None; KEYS];
    for (dev, json) in rows {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) else { continue };
        let pairs = color_pairs(&v["Static"]);
        for (leds, color) in pairs {
            let c = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
            for l in leds {
                if dev == 2 && (l as usize) < KEYS {
                    keys[l as usize] = Some(c);
                    found = true;
                } else if dev == 1 {
                    if crate::chassis::EMBLEM.contains(&(l as u8)) {
                        st.emblem = to_hex(c);
                        found = true;
                    }
                    if crate::chassis::CONTOUR.contains(&(l as u8)) {
                        st.contour = to_hex(c);
                        found = true;
                    }
                }
            }
        }
    }
    if keys.iter().any(|k| k.is_some()) {
        st.set_key_colors(&keys);
    }
    found
}

fn color_pairs(s: &serde_json::Value) -> Vec<(Vec<i64>, u32)> {
    let mut out = Vec::new();
    let seq = |seqs: &serde_json::Value, out: &mut Vec<(Vec<i64>, u32)>| {
        if let Some(arr) = seqs.as_array() {
            for sq in arr {
                let leds: Vec<i64> = sq["LEDs"].as_array().map(|a| a.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default();
                let col = sq["Actions"][0]["Color"][0].as_u64().unwrap_or(0) as u32;
                if !leds.is_empty() {
                    out.push((leds, col));
                }
            }
        }
    };
    if !s["Animation"].is_null() {
        seq(&s["Animation"]["Sequence"], &mut out);
    }
    if out.is_empty() {
        if let Some(arr) = s["PredefinedAnimations"].as_array() {
            for pa in arr {
                let id = pa["ID"].as_i64().unwrap_or(0);
                if (91..=96).contains(&id) {
                    continue;
                }
                seq(&pa["Preview"], &mut out);
            }
        }
    }
    out
}
