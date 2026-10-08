//! Значки клавиш: картинки из инструкции Dell и пара стрелок, нарисованных линиями.
use eframe::egui::{self, vec2, Color32, Pos2, Rect, Shape, Stroke};

/// Стрелки Backspace и Enter (в инструкции Dell их нет).
pub fn draw(p: &egui::Painter, name: &str, c: Pos2, k: f32, col: Color32) {
    let s = Stroke::new(1.8 * k, col);
    match name {
        "back" => {
            p.line_segment([c + vec2(-9.0, 0.0) * k, c + vec2(9.0, 0.0) * k], s);
            p.add(Shape::line(vec![c + vec2(-5.0, -3.5) * k, c + vec2(-9.0, 0.0) * k, c + vec2(-5.0, 3.5) * k], s));
        }
        "enter" => {
            p.add(Shape::line(vec![c + vec2(7.0, -4.0) * k, c + vec2(7.0, 2.0) * k, c + vec2(-7.0, 2.0) * k], s));
            p.add(Shape::line(vec![c + vec2(-4.0, -1.5) * k, c + vec2(-7.0, 2.0) * k, c + vec2(-4.0, 5.5) * k], s));
        }
        _ => {}
    }
}

/// Где поставить картинку-значок на клавише.
pub enum At {
    /// там же, где на настоящей клавише (по отступам из инструкции Dell)
    Dell,
    /// по центру, `f` — во сколько раз крупнее
    Center(f32),
    /// в правом нижнем углу
    Corner,
}

struct Tex {
    name: &'static str,
    id: egui::TextureId,
    size: egui::Vec2,
    off: egui::Vec2,
    _h: egui::TextureHandle,
}

static TEX: std::sync::OnceLock<Vec<Tex>> = std::sync::OnceLock::new();

fn textures(ctx: &egui::Context) -> &'static Vec<Tex> {
    TEX.get_or_init(|| {
        crate::icons_data::ICONS.iter().map(|(name, w, h, r, b, data)| {
            let rgba: Vec<u8> = data.iter().flat_map(|a| [255, 255, 255, *a]).collect();
            let img = egui::ColorImage::from_rgba_unmultiplied([*w, *h], &rgba);
            let th = ctx.load_texture(*name, img, egui::TextureOptions::LINEAR);
            Tex { name, id: th.id(), size: vec2(*w as f32, *h as f32), off: vec2(*r as f32, *b as f32), _h: th }
        }).collect()
    })
}

/// Нарисовать значок из инструкции Dell на клавише `key` цветом `col`.
pub fn image(p: &egui::Painter, name: &str, key: Rect, at: At, col: Color32) {
    let Some(t) = textures(p.ctx()).iter().find(|t| t.name == name) else { return };
    let s = (key.width() / 105.0).min(key.height() / 64.0);
    let r = match at {
        At::Dell => {
            let max = key.right_bottom() - t.off * s * 0.6;
            Rect::from_min_max(max - t.size * s * 1.5, max)
        }
        At::Center(f) => Rect::from_center_size(key.center(), t.size * s * f),
        At::Corner => {
            let max = key.right_bottom() - vec2(7.0, 6.0) * s / 0.6;
            Rect::from_min_max(max - t.size * s * 1.5, max)
        }
    };
    p.image(t.id, r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), col);
}

/// Значок второй функции верхнего ряда.
pub fn icon_for(id: u8) -> Option<&'static str> {
    Some(match id {
        0 => "lock",
        1 => "F1", 2 => "F2", 3 => "F3", 4 => "F4", 5 => "F5", 6 => "F6",
        7 => "F7", 8 => "F8", 9 => "F9", 10 => "F10", 11 => "F11", 12 => "F12",
        _ => return None,
    })
}
