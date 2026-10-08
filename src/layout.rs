//! Расположение клавиш m18 R2 — снято с фотографии клавиатуры ноутбука.
//! Координаты в точках фотографии: слева 240, справа 1787.

pub struct Key {
    pub label: &'static str,
    pub id: u8,
    pub x0: f32,
    pub x1: f32,
    pub row: u8,
    /// клавиша на два ряда (+ и Enter цифрового блока)
    pub tall: bool,
}

pub const LEFT: f32 = 240.0;
pub const RIGHT: f32 = 1787.0;
const P: f32 = 85.0; // шаг обычных клавиш
const W: f32 = 77.0; // ширина обычной клавиши
const NP: [f32; 4] = [1495.0, 1570.0, 1645.0, 1720.0]; // колонки цифрового блока
const NW: f32 = 67.0;

fn key(v: &mut Vec<Key>, row: u8, label: &'static str, id: u8, x0: f32, x1: f32) {
    v.push(Key { label, id, x0, x1, row, tall: false });
}

fn run(v: &mut Vec<Key>, row: u8, start: f32, keys: &[(&'static str, u8)]) {
    for (i, (l, id)) in keys.iter().enumerate() {
        let x = start + i as f32 * P;
        key(v, row, l, *id, x, x + W);
    }
}

fn pad(v: &mut Vec<Key>, row: u8, col: usize, label: &'static str, id: u8, tall: bool) {
    v.push(Key { label, id, x0: NP[col], x1: NP[col] + NW, row, tall });
}

pub fn keys() -> Vec<Key> {
    let mut v = Vec::new();
    // верхний ряд — 20 одинаковых клавиш на всю ширину
    let top: [(&str, u8); 20] = [
        ("ESC", 0), ("F1", 1), ("F2", 2), ("F3", 3), ("F4", 4), ("F5", 5), ("F6", 6), ("F7", 7), ("F8", 8), ("F9", 9),
        ("F10", 10), ("F11", 11), ("F12", 12), ("HOME", 13), ("END", 14), ("DEL", 15), ("🔇", 16), ("🔉", 17), ("🔊", 18), ("MIC", 19),
    ];
    let tp = (RIGHT - LEFT + 8.0) / 20.0;
    for (i, (l, id)) in top.iter().enumerate() {
        let x = LEFT + i as f32 * tp;
        key(&mut v, 0, l, *id, x, x + tp - 8.0);
    }
    // ряд цифр
    run(&mut v, 1, 240.0, &[("`", 20), ("1", 21), ("2", 22), ("3", 23), ("4", 24), ("5", 25), ("6", 26), ("7", 27), ("8", 28), ("9", 29), ("0", 30), ("-", 31), ("=", 32)]);
    key(&mut v, 1, "Backspace", 35, 1345.0, 1485.0);
    pad(&mut v, 1, 0, "Num", 36, false);
    pad(&mut v, 1, 1, "/", 37, false);
    pad(&mut v, 1, 2, "*", 38, false);
    pad(&mut v, 1, 3, "-", 39, false);
    // Q
    key(&mut v, 2, "TAB", 40, 240.0, 357.0);
    run(&mut v, 2, 365.0, &[("Q", 42), ("W", 43), ("E", 44), ("R", 45), ("T", 46), ("Y", 47), ("U", 48), ("I", 49), ("O", 50), ("P", 51), ("[", 52), ("]", 53)]);
    key(&mut v, 2, "\\", 55, 1385.0, 1485.0);
    pad(&mut v, 2, 0, "7", 56, false);
    pad(&mut v, 2, 1, "8", 57, false);
    pad(&mut v, 2, 2, "9", 58, false);
    pad(&mut v, 2, 3, "+", 79, true);
    // A
    key(&mut v, 3, "CAPS", 61, 240.0, 380.0);
    run(&mut v, 3, 388.0, &[("A", 62), ("S", 63), ("D", 64), ("F", 65), ("G", 66), ("H", 67), ("J", 68), ("K", 69), ("L", 70), (";", 71), ("'", 72)]);
    key(&mut v, 3, "Enter", 74, 1323.0, 1485.0);
    pad(&mut v, 3, 0, "4", 76, false);
    pad(&mut v, 3, 1, "5", 77, false);
    pad(&mut v, 3, 2, "6", 78, false);
    // Z
    key(&mut v, 4, "SHIFT", 81, 240.0, 412.0);
    run(&mut v, 4, 420.0, &[("Z", 83), ("X", 84), ("C", 85), ("V", 86), ("B", 87), ("N", 88), ("M", 89), (",", 90), (".", 91), ("/", 92)]);
    key(&mut v, 4, "SHIFT", 94, 1270.0, 1400.0);
    key(&mut v, 4, "▲", 114, 1408.0, 1485.0);
    pad(&mut v, 4, 0, "1", 96, false);
    pad(&mut v, 4, 1, "2", 97, false);
    pad(&mut v, 4, 2, "3", 98, false);
    pad(&mut v, 4, 3, "Enter", 119, true);
    // нижний ряд
    key(&mut v, 5, "CTRL", 100, 240.0, 322.0);
    key(&mut v, 5, "FN", 101, 330.0, 407.0);
    key(&mut v, 5, "Win", 103, 415.0, 492.0);
    key(&mut v, 5, "ALT", 104, 500.0, 577.0);
    key(&mut v, 5, "", 107, 585.0, 1012.0);
    key(&mut v, 5, "ALT", 111, 1020.0, 1097.0);
    key(&mut v, 5, "Copilot", 109, 1105.0, 1182.0);
    key(&mut v, 5, "CTRL", 112, 1190.0, 1315.0);
    key(&mut v, 5, "◀", 133, 1323.0, 1400.0);
    key(&mut v, 5, "▼", 134, 1408.0, 1485.0);
    pad(&mut v, 5, 0, "▶", 135, false);
    pad(&mut v, 5, 1, "0", 117, false);
    pad(&mut v, 5, 2, ".", 118, false);
    v
}

pub fn all_ids() -> Vec<u8> {
    keys().iter().map(|k| k.id).collect()
}

pub fn label(id: u8) -> String {
    for k in keys() {
        if k.id == id {
            return match k.label { "" => crate::i18n::t("Пробел").into(), "MIC" => crate::i18n::t("Микрофон").into(), "🔇" => crate::i18n::t("Без звука").into(), "🔉" => crate::i18n::t("Тише").into(), "🔊" => crate::i18n::t("Громче").into(), l => l.to_string() };
        }
    }
    format!("#{id}")
}

pub const F_KEYS: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
pub const NUMBERS: &[u8] = &[21, 22, 23, 24, 25, 26, 27, 28, 29, 30];
pub const QWER: &[u8] = &[42, 43, 44, 45];
pub const WASD: &[u8] = &[43, 62, 63, 64];

/// Вторая надпись на клавише (русская буква, символ при Shift и т.п.), как на клавиатуре ноутбука.
pub fn secondary(id: u8) -> &'static str {
    match id {
        20 => "~ Ё", 21 => "!", 22 => "@", 23 => "#", 24 => "$", 25 => "%", 26 => "^", 27 => "&", 28 => "*",
        29 => "(", 30 => ")", 31 => "_", 32 => "+",
        42 => "Й", 43 => "Ц", 44 => "У", 45 => "К", 46 => "Е", 47 => "Н", 48 => "Г", 49 => "Ш", 50 => "Щ", 51 => "З",
        52 => "{ Х", 53 => "} Ъ", 55 => "| /",
        62 => "Ф", 63 => "Ы", 64 => "В", 65 => "А", 66 => "П", 67 => "Р", 68 => "О", 69 => "Л", 70 => "Д", 71 => ": Ж", 72 => "\" Э",
        83 => "Я", 84 => "Ч", 85 => "С", 86 => "М", 87 => "И", 88 => "Т", 89 => "Ь", 90 => "< Б", 91 => "> Ю", 92 => "? .",
        13 => "PRT SCR", 14 => "PAUSE", 15 => "INSERT", 12 => "T-PAD",
        114 => "PAGE UP", 134 => "PAGE DN",
        56 => "HOME", 57 => "↑", 58 => "PG UP", 76 => "←", 77 => "—", 78 => "→", 96 => "END", 97 => "↓", 98 => "PG DN", 117 => "INS", 118 => "DEL",
        _ => "",
    }
}

/// Русский символ при Shift — мелко справа от цифры, как на клавише.
pub fn corner(id: u8) -> &'static str {
    match id {
        22 => "\"", 23 => "№", 24 => ";", 26 => ":", 27 => "?",
        _ => "",
    }
}

/// Клавиши цифрового блока (без самой Num Lock — её свет включает и выключает клавиатура).
pub const NUMPAD: &[u8] = &[37, 38, 39, 56, 57, 58, 79, 76, 77, 78, 96, 97, 98, 119, 117, 118];
pub fn is_numpad(id: u8) -> bool {
    NUMPAD.contains(&id)
}

pub const CAPS: u8 = 61;
pub const NUMLOCK: u8 = 36;
/// Клавиши цифрового блока, у которых без Num Lock другая функция.
pub fn numpad_nav(id: u8) -> bool {
    matches!(id, 56 | 57 | 58 | 76 | 77 | 78 | 96 | 97 | 98 | 117 | 118)
}
