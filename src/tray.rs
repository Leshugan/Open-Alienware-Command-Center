//! Значок батареи в трее: заряд в процентах, цвет по уровню, молния при питании от сети.
//! Левый клик — режим «Батарея» / обратно «Баланс» (с всплывающим подтверждением).
//! Правый клик — своё тёмное меню со всеми режимами.
use crate::log;
use crate::power::{Cmd, Sensors};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

/// Показывать значок (меняется из вкладки «Производительность»).
pub static SHOW: AtomicBool = AtomicBool::new(true);
static HWND_TRAY: AtomicIsize = AtomicIsize::new(0);

static HWND_MENU: AtomicIsize = AtomicIsize::new(0);

struct Ctx {
    ptx: Sender<Cmd>,
    sensors: Arc<Mutex<Sensors>>,
}
static CTX: OnceLock<Mutex<Ctx>> = OnceLock::new();

// ------------------------------------------------------------------ Win32
#[repr(C)]
struct WndClass {
    style: u32,
    proc_: extern "system" fn(isize, u32, usize, isize) -> isize,
    cls_extra: i32,
    wnd_extra: i32,
    instance: isize,
    icon: isize,
    cursor: isize,
    background: isize,
    menu_name: *const u16,
    class_name: *const u16,
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
#[repr(C)]
struct NotifyIconData {
    size: u32,
    hwnd: isize,
    id: u32,
    flags: u32,
    callback: u32,
    icon: isize,
    tip: [u16; 128],
    state: u32,
    state_mask: u32,
    info: [u16; 256],
    version: u32,
    info_title: [u16; 64],
    info_flags: u32,
    guid: [u8; 16],
    balloon_icon: isize,
}
#[repr(C)]
struct IconInfo {
    is_icon: i32,
    x: u32,
    y: u32,
    mask: isize,
    color: isize,
}
#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    xppm: i32,
    yppm: i32,
    clr_used: u32,
    clr_important: u32,
}
#[repr(C)]
struct PowerStatus {
    ac: u8,
    flag: u8,
    percent: u8,
    saver: u8,
    life: u32,
    full: u32,
}
#[repr(C)]
struct Blend {
    op: u8,
    flags: u8,
    alpha: u8,
    format: u8,
}
#[repr(C)]
struct TrackMouse {
    size: u32,
    flags: u32,
    hwnd: isize,
    hover: u32,
}

#[link(name = "user32")]
extern "system" {
    fn RegisterClassW(c: *const WndClass) -> u16;
    fn CreateWindowExW(ex: u32, class: *const u16, name: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32, parent: isize, menu: isize, inst: isize, param: *const std::ffi::c_void) -> isize;
    fn DefWindowProcW(h: isize, m: u32, w: usize, l: isize) -> isize;
    fn GetMessageW(m: *mut Msg, h: isize, a: u32, b: u32) -> i32;
    fn TranslateMessage(m: *const Msg) -> i32;
    fn DispatchMessageW(m: *const Msg) -> isize;
    fn SetTimer(h: isize, id: usize, ms: u32, f: *const std::ffi::c_void) -> usize;
    fn RegisterWindowMessageW(s: *const u16) -> u32;
    fn GetCursorPos(p: *mut [i32; 2]) -> i32;
    fn SetForegroundWindow(h: isize) -> i32;
    fn PostMessageW(h: isize, m: u32, w: usize, l: isize) -> i32;
    fn FindWindowW(c: *const u16, n: *const u16) -> isize;
    fn ShowWindow(h: isize, cmd: i32) -> i32;
    fn CreateIconIndirect(i: *const IconInfo) -> isize;
    fn DestroyIcon(i: isize) -> i32;
    fn GetSystemMetrics(i: i32) -> i32;
    fn GetDpiForSystem() -> u32;
    fn SystemParametersInfoW(a: u32, b: u32, p: *mut std::ffi::c_void, f: u32) -> i32;
    fn UpdateLayeredWindow(h: isize, dst: isize, pos: *const [i32; 2], size: *const [i32; 2], src: isize, srcpos: *const [i32; 2], key: u32, blend: *const Blend, flags: u32) -> i32;
    fn TrackMouseEvent(t: *mut TrackMouse) -> i32;
    fn SetCursor(c: isize) -> isize;
    fn LoadCursorW(i: isize, n: *const u16) -> isize;
}
#[link(name = "shell32")]
extern "system" {
    fn Shell_NotifyIconW(msg: u32, d: *const NotifyIconData) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn CreateDIBSection(dc: isize, bi: *const BitmapInfoHeader, usage: u32, bits: *mut *mut u8, sec: isize, off: u32) -> isize;
    fn CreateBitmap(w: i32, h: i32, planes: u32, bpp: u32, bits: *const std::ffi::c_void) -> isize;
    fn DeleteObject(o: isize) -> i32;
    fn CreateCompatibleDC(dc: isize) -> isize;
    fn SelectObject(dc: isize, o: isize) -> isize;
    fn DeleteDC(dc: isize) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetSystemPowerStatus(s: *mut PowerStatus) -> i32;
    fn GetModuleHandleW(n: *const u16) -> isize;
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

const WM_TRAY: u32 = 0x8000 + 1;
const WM_TIMER: u32 = 0x0113;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_ACTIVATE: u32 = 0x0006;
const WM_SETCURSOR: u32 = 0x0020;
const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIF_MESSAGE: u32 = 1;
const NIF_ICON: u32 = 2;
const NIF_TIP: u32 = 4;

// ------------------------------------------------------------------ рисование
type Rgba = [f32; 4];

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<Rgba>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, px: vec![[0.0; 4]; w * h] }
    }
    fn blend(&mut self, x: i32, y: i32, c: Rgba) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h || c[3] <= 0.0 {
            return;
        }
        let p = &mut self.px[y as usize * self.w + x as usize];
        let a = c[3] + p[3] * (1.0 - c[3]);
        for i in 0..3 {
            p[i] = (c[i] * c[3] + p[i] * p[3] * (1.0 - c[3])) / a;
        }
        p[3] = a;
    }
    fn span(&self, a: f32, b: f32, lim: usize) -> std::ops::Range<i32> {
        (a.floor() as i32 - 1).max(0)..((b.ceil() as i32 + 1).min(lim as i32))
    }
    /// Скруглённый прямоугольник; t > 0 — только рамка такой толщины.
    fn rrect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, t: f32, c: Rgba) {
        let r = r.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0).max(0.0);
        for y in self.span(y0, y1, self.h) {
            for x in self.span(x0, x1, self.w) {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let cx = px.clamp(x0 + r, x1 - r);
                let cy = py.clamp(y0 + r, y1 - r);
                let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - r;
                let mut cov = (0.5 - d).clamp(0.0, 1.0);
                if t > 0.0 {
                    cov = (cov - (0.5 - (d + t)).clamp(0.0, 1.0)).max(0.0);
                }
                self.blend(x, y, [c[0], c[1], c[2], c[3] * cov]);
            }
        }
    }
    fn circle(&mut self, cx: f32, cy: f32, r: f32, t: f32, c: Rgba) {
        self.rrect(cx - r, cy - r, cx + r, cy + r, r, t, c);
    }
    fn polygon(&mut self, pts: &[(f32, f32)], c: Rgba) {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (x, y) in pts {
            x0 = x0.min(*x);
            y0 = y0.min(*y);
            x1 = x1.max(*x);
            y1 = y1.max(*y);
        }
        for y in self.span(y0, y1, self.h) {
            for x in self.span(x0, x1, self.w) {
                let mut hit = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let (px, py) = (x as f32 + (sx as f32 + 0.5) / 4.0, y as f32 + (sy as f32 + 0.5) / 4.0);
                        let mut inside = false;
                        let mut j = pts.len() - 1;
                        for i in 0..pts.len() {
                            let (xi, yi) = pts[i];
                            let (xj, yj) = pts[j];
                            if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
                                inside = !inside;
                            }
                            j = i;
                        }
                        hit += inside as i32;
                    }
                }
                if hit > 0 {
                    self.blend(x, y, [c[0], c[1], c[2], c[3] * hit as f32 / 16.0]);
                }
            }
        }
    }
    fn text_width(font: &ab_glyph::FontVec, txt: &str, px: f32) -> f32 {
        use ab_glyph::{Font, ScaleFont};
        let sf = font.as_scaled(ab_glyph::PxScale::from(px));
        txt.chars().map(|ch| sf.h_advance(font.glyph_id(ch))).sum()
    }
    /// Текст: (x, y) — левый край и середина строки по высоте заглавных букв.
    fn text(&mut self, font: &ab_glyph::FontVec, txt: &str, px: f32, x: f32, cy: f32, c: Rgba) {
        use ab_glyph::{Font, ScaleFont};
        let sf = font.as_scaled(ab_glyph::PxScale::from(px));
        let cap = sf.ascent() * 0.72;
        let mut x = x;
        let base = cy + cap / 2.0;
        for ch in txt.chars() {
            let id = font.glyph_id(ch);
            let g = id.with_scale_and_position(px, ab_glyph::point(x, base));
            if let Some(o) = font.outline_glyph(g) {
                let b = o.px_bounds();
                o.draw(|gx, gy, v| {
                    self.blend(b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32, [c[0], c[1], c[2], c[3] * v.min(1.0)]);
                });
            }
            x += sf.h_advance(id);
        }
    }
    fn text_center(&mut self, font: &ab_glyph::FontVec, txt: &str, px: f32, cx: f32, cy: f32, c: Rgba) {
        let wd = Canvas::text_width(font, txt, px);
        self.text(font, txt, px, cx - wd / 2.0, cy, c);
    }
    /// В формат Windows: синий-зелёный-красный-прозрачность; premul — для окон с прозрачностью.
    fn bgra(&self, premul: bool) -> Vec<u8> {
        let mut out = vec![0u8; self.w * self.h * 4];
        for (i, p) in self.px.iter().enumerate() {
            let k = if premul { p[3] } else { 1.0 };
            out[i * 4] = (p[2] * k * 255.0) as u8;
            out[i * 4 + 1] = (p[1] * k * 255.0) as u8;
            out[i * 4 + 2] = (p[0] * k * 255.0) as u8;
            out[i * 4 + 3] = (p[3] * 255.0) as u8;
        }
        out
    }
    /// Уменьшить в k раз (сглаживание).
    fn down(&self, k: usize) -> Canvas {
        let (w, h) = (self.w / k, self.h / k);
        let mut c = Canvas::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0f32; 4];
                for dy in 0..k {
                    for dx in 0..k {
                        let p = self.px[(y * k + dy) * self.w + x * k + dx];
                        for i in 0..3 {
                            acc[i] += p[i] * p[3];
                        }
                        acc[3] += p[3];
                    }
                }
                let a = acc[3] / (k * k) as f32;
                let col = |i: usize| if acc[3] > 0.0 { acc[i] / acc[3] } else { 0.0 };
                c.px[y * w + x] = [col(0), col(1), col(2), a];
            }
        }
        c
    }
}

fn load_font(names: &[&str]) -> Option<ab_glyph::FontVec> {
    for f in names {
        if let Ok(b) = std::fs::read(format!("C:\\Windows\\Fonts\\{f}")) {
            if let Ok(font) = ab_glyph::FontVec::try_from_vec(b) {
                return Some(font);
            }
        }
    }
    None
}
fn font_bold() -> Option<&'static ab_glyph::FontVec> {
    static F: OnceLock<Option<ab_glyph::FontVec>> = OnceLock::new();
    F.get_or_init(|| load_font(&["segoeuib.ttf", "seguisb.ttf", "segoeui.ttf"])).as_ref()
}
fn font_semi() -> Option<&'static ab_glyph::FontVec> {
    static F: OnceLock<Option<ab_glyph::FontVec>> = OnceLock::new();
    F.get_or_init(|| load_font(&["seguisb.ttf", "segoeui.ttf"])).as_ref()
}
fn font_reg() -> Option<&'static ab_glyph::FontVec> {
    static F: OnceLock<Option<ab_glyph::FontVec>> = OnceLock::new();
    F.get_or_init(|| load_font(&["segoeui.ttf"])).as_ref()
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Rgba {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a]
}
const ACC: Rgba = [33.0 / 255.0, 212.0 / 255.0, 253.0 / 255.0, 1.0];
const TEXT: Rgba = [231.0 / 255.0, 234.0 / 255.0, 240.0 / 255.0, 1.0];
const DIM: Rgba = [139.0 / 255.0, 147.0 / 255.0, 163.0 / 255.0, 1.0];
const PANEL: Rgba = [23.0 / 255.0, 26.0 / 255.0, 32.0 / 255.0, 1.0];
const LINE: Rgba = [42.0 / 255.0, 47.0 / 255.0, 57.0 / 255.0, 1.0];

/// Листик (точки контура): начало в (x, y), длина len, наклон ang.
fn leaf_pts(x: f32, y: f32, len: f32, ang: f32, grow: f32) -> Vec<(f32, f32)> {
    let h = len * 0.32 + grow;
    let (s, c) = ang.sin_cos();
    let mut pts = Vec::new();
    for i in 0..=16 {
        let t = i as f32 / 16.0;
        pts.push((t * len, -h * (std::f32::consts::PI * t).sin()));
    }
    for i in (0..=16).rev() {
        let t = i as f32 / 16.0;
        pts.push((t * len, h * 0.7 * (std::f32::consts::PI * t).sin()));
    }
    pts.iter().map(|(px, py)| (x + px * c - py * s, y + px * s + py * c)).collect()
}

/// Значок батареи s×s. От сети — белая рамка и голубая заливка; от батареи — без рамки,
/// вся батарейка цветом заряда. eco — режим «Энергосбережение» (листик в углу).
fn draw_battery(s: usize, pct: u8, ac: bool, eco: bool) -> Canvas {
    let mut c = Canvas::new(s, s);
    let f = s as f32;
    let white = rgba(240, 242, 246, 1.0);
    let capw = f * 0.07;
    let (bw, bh) = (f - capw - f * 0.015, f * 0.78);
    let (x0, y0) = (0.0, (f - bh) / 2.0);
    let r = f * 0.13;
    let th = bh * 0.42;
    if ac {
        let t = (f * 0.065).max(1.0);
        let pad = t * 1.3;
        let full = bw - 2.0 * pad;
        c.rrect(x0 + pad, y0 + pad, x0 + pad + (full * pct as f32 / 100.0).max(r * 0.4), y0 + bh - pad, r * 0.5, 0.0, rgba(30, 170, 225, 1.0));
        c.rrect(x0, y0, x0 + bw, y0 + bh, r, t, white);
        c.rrect(x0 + bw + f * 0.015, f / 2.0 - th / 2.0, f, f / 2.0 + th / 2.0, capw * 0.35, 0.0, white);
    } else {
        let col = if pct <= 10 { [225u8, 60, 60] } else if pct <= 20 { [230, 150, 40] } else { [60, 175, 95] };
        let dark = rgba((col[0] as f32 * 0.42) as u8, (col[1] as f32 * 0.42) as u8, (col[2] as f32 * 0.42) as u8, 1.0);
        let bright = rgba(col[0], col[1], col[2], 1.0);
        // тёмная батарейка целиком, яркая — заряженная часть (обрезаем по ширине)
        c.rrect(x0, y0, x0 + bw, y0 + bh, r, 0.0, dark);
        let cut = x0 + bw * pct as f32 / 100.0;
        let mut part = Canvas::new(s, s);
        part.rrect(x0, y0, x0 + bw, y0 + bh, r, 0.0, bright);
        for y in 0..s {
            for x in 0..s {
                let cov = (cut - x as f32).clamp(0.0, 1.0);
                let p = part.px[y * s + x];
                if p[3] > 0.0 && cov > 0.0 {
                    c.blend(x as i32, y as i32, [p[0], p[1], p[2], p[3] * cov]);
                }
            }
        }
        c.rrect(x0 + bw + f * 0.015, f / 2.0 - th / 2.0, f, f / 2.0 + th / 2.0, capw * 0.35, 0.0, if pct >= 97 { bright } else { dark });
    }
    if let Some(font) = font_bold() {
        let txt = pct.to_string();
        let size = bh * if pct < 100 { 0.92 } else { 0.72 };
        let (cx, cy) = (x0 + bw / 2.0, f / 2.0);
        for (dx, dy, a) in [(0.0, f * 0.03, 0.55), (f * 0.02, 0.0, 0.3), (-f * 0.02, 0.0, 0.3)] {
            c.text_center(font, &txt, size, cx + dx, cy + dy, rgba(0, 0, 0, a));
        }
        c.text_center(font, &txt, size, cx, cy, rgba(255, 255, 255, 1.0));
    }
    if eco {
        // листик в правом верхнем углу с тёмной обводкой
        let (lx, ly, len, ang) = (f * 0.60, f * 0.30, f * 0.42, -0.62);
        c.polygon(&leaf_pts(lx - f * 0.03, ly + f * 0.02, len + f * 0.07, ang, f * 0.04), rgba(16, 18, 22, 1.0));
        c.polygon(&leaf_pts(lx, ly, len, ang, 0.0), rgba(150, 235, 120, 1.0));
        // прожилка
        let (s2, c2) = ang.sin_cos();
        let (ax, ay) = (lx + len * 0.12 * c2, ly + len * 0.12 * s2);
        let (bx, by) = (lx + len * 0.85 * c2, ly + len * 0.85 * s2);
        let nx = -s2 * f * 0.012;
        let ny = c2 * f * 0.012;
        c.polygon(&[(ax + nx, ay + ny), (bx + nx, by + ny), (bx - nx, by - ny), (ax - nx, ay - ny)], rgba(40, 110, 50, 1.0));
    }
    c
}

fn icon_size() -> usize {
    unsafe { GetSystemMetrics(49) }.clamp(16, 64) as usize // SM_CXSMICON
}

/// Значок Windows из картинки.
fn make_icon(pct: u8, ac: bool, eco: bool) -> isize {
    let s = icon_size();
    let img = draw_battery(s * 4, pct, ac, eco).down(4);
    let bgra = img.bgra(false);
    unsafe {
        let bi = BitmapInfoHeader { size: 40, width: s as i32, height: -(s as i32), planes: 1, bit_count: 32, compression: 0, size_image: 0, xppm: 0, yppm: 0, clr_used: 0, clr_important: 0 };
        let mut bits: *mut u8 = std::ptr::null_mut();
        let color = CreateDIBSection(0, &bi, 0, &mut bits, 0, 0);
        if color == 0 || bits.is_null() {
            return 0;
        }
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits, bgra.len());
        let zeros = vec![0u8; s * s / 8 + s * 4];
        let mask = CreateBitmap(s as i32, s as i32, 1, 1, zeros.as_ptr() as _);
        let ii = IconInfo { is_icon: 1, x: 0, y: 0, mask, color };
        let icon = CreateIconIndirect(&ii);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}

/// Значки режимов — как во вкладке программы.
fn mode_icon(c: &mut Canvas, code: usize, cx: f32, cy: f32, k: f32, col: Rgba, bg: Rgba) {
    let v = |x: f32, y: f32| (cx + x * k, cy + y * k);
    let t = (1.8 * k).max(1.0);
    match code {
        0 => {
            // листик — энергосбережение
            c.polygon(&leaf_pts(cx - 10.0 * k, cy + 8.0 * k, 24.0 * k, -0.7, 0.0), col);
            let (s2, c2) = (-0.7f32).sin_cos();
            let (ax, ay) = (cx - 10.0 * k, cy + 8.0 * k);
            let (bx, by) = (ax + 20.0 * k * c2, ay + 20.0 * k * s2);
            let (nx, ny) = (-s2 * 0.9 * k, c2 * 0.9 * k);
            c.polygon(&[(ax + nx, ay + ny), (bx + nx, by + ny), (bx - nx, by - ny), (ax - nx, ay - ny)], bg);
        }
        1 => {
            c.circle(cx, cy, 10.0 * k, 0.0, col);
            c.circle(cx + 5.0 * k, cy - 4.0 * k, 8.5 * k, 0.0, bg);
        }
        2 => {
            c.circle(cx, cy, 10.0 * k, t, col);
            let mut pts = vec![v(0.0, -10.0)];
            for i in 0..=20 {
                let a = std::f32::consts::PI * (-0.5 + i as f32 / 20.0);
                pts.push(v(a.cos() * 10.0, a.sin() * 10.0));
            }
            c.polygon(&pts, col);
        }
        3 => {
            let pts: Vec<(f32, f32)> = [(3.0, -12.0), (-7.0, 2.0), (-1.0, 2.0), (-4.0, 12.0), (7.0, -3.0), (1.0, -3.0)].iter().map(|(x, y)| v(*x, *y)).collect();
            c.polygon(&pts, col);
        }
        _ => {
            let pts: Vec<(f32, f32)> = [(0.0, -12.0), (5.0, -5.0), (8.0, 1.0), (8.0, 5.0), (5.0, 10.0), (0.0, 12.0), (-5.0, 10.0), (-8.0, 5.0), (-8.0, 1.0), (-5.0, -4.0), (-3.0, 0.0), (-1.0, -6.0)].iter().map(|(x, y)| v(*x, *y)).collect();
            c.polygon(&pts, col);
        }
    }
}

// ------------------------------------------------------------------ состояние
const SHORT: [&str; 5] = ["Энергосбережение", "Тихий", "Баланс", "Производительность", "Максимум"];
const DESC: [&str; 5] = ["Дольше без зарядки, мощность и шум — минимум", "Почти не слышно: браузер, кино, работа", "Обычный режим на каждый день", "Больше мощности для игр, вентиляторы громче", "Вся мощность, вентиляторы на полную"];

fn power() -> (u8, bool, Option<u32>) {
    let mut ps = PowerStatus { ac: 0, flag: 0, percent: 0, saver: 0, life: 0, full: 0 };
    unsafe { GetSystemPowerStatus(&mut ps) };
    let pct = if ps.percent > 100 { 100 } else { ps.percent };
    let life = if ps.life == u32::MAX || ps.ac == 1 { None } else { Some(ps.life) };
    (pct, ps.ac == 1, life)
}

fn mode() -> Option<u8> {
    CTX.get().and_then(|c| c.lock().ok()).and_then(|c| c.sensors.lock().ok().and_then(|s| s.mode))
}

fn set_mode(i: u8) {
    if let Some(c) = CTX.get().and_then(|c| c.lock().ok()) {
        let _ = c.ptx.send(Cmd::SetMode(i));
        if let Ok(mut s) = c.sensors.lock() {
            s.mode = Some(i);
        }
    }
    log::write(&format!("Трей: режим {}", SHORT[i as usize]));
}

struct Shown {
    on: bool,
    pct: u8,
    ac: bool,
    mode: Option<u8>,
    icon: isize,
}
static SHOWN: Mutex<Shown> = Mutex::new(Shown { on: false, pct: 255, ac: false, mode: None, icon: 0 });

fn data(hwnd: isize) -> NotifyIconData {
    let mut d: NotifyIconData = unsafe { std::mem::zeroed() };
    d.size = std::mem::size_of::<NotifyIconData>() as u32;
    d.hwnd = hwnd;
    d.id = 1;
    d.callback = WM_TRAY;
    d
}

/// Привести значок в трее к текущему состоянию (показ, заряд, режим).
fn refresh(hwnd: isize, force: bool) {
    let want = SHOW.load(Ordering::Relaxed);
    let Ok(mut sh) = SHOWN.lock() else { return };
    if !want {
        if sh.on {
            unsafe { Shell_NotifyIconW(NIM_DELETE, &data(hwnd)) };
            sh.on = false;
        }
        return;
    }
    let (pct, ac, _) = power();
    let m = mode();
    if sh.on && !force && sh.pct == pct && sh.ac == ac && sh.mode == m {
        return;
    }
    let mut d = data(hwnd);
    d.flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    let icon = make_icon(pct, ac, m == Some(0));
    d.icon = icon;
    let tip = format!("Заряд {pct}% · {} · режим «{}»", if ac { "от сети" } else { "от батареи" }, m.map(|m| SHORT[m as usize]).unwrap_or("—"));
    for (i, ch) in tip.encode_utf16().take(127).enumerate() {
        d.tip[i] = ch;
    }
    let ok = unsafe { Shell_NotifyIconW(if sh.on && !force { NIM_MODIFY } else { NIM_ADD }, &d) };
    if ok == 0 {
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &d) };
    }
    if sh.icon != 0 {
        unsafe { DestroyIcon(sh.icon) };
    }
    *sh = Shown { on: true, pct, ac, mode: m, icon };
}

// ------------------------------------------------------------------ окна-всплывашки
fn scale() -> f32 {
    unsafe { GetDpiForSystem() }.max(96) as f32 / 96.0
}

fn work_area() -> [i32; 4] {
    let mut r = [0i32; 4];
    unsafe { SystemParametersInfoW(0x30, 0, r.as_mut_ptr() as _, 0) }; // SPI_GETWORKAREA
    r
}

/// Показать картинку в прозрачном окне.
fn present(hwnd: isize, img: &Canvas, x: i32, y: i32) {
    let bgra = img.bgra(true);
    unsafe {
        let bi = BitmapInfoHeader { size: 40, width: img.w as i32, height: -(img.h as i32), planes: 1, bit_count: 32, compression: 0, size_image: 0, xppm: 0, yppm: 0, clr_used: 0, clr_important: 0 };
        let mut bits: *mut u8 = std::ptr::null_mut();
        let bmp = CreateDIBSection(0, &bi, 0, &mut bits, 0, 0);
        if bmp == 0 || bits.is_null() {
            return;
        }
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits, bgra.len());
        let dc = CreateCompatibleDC(0);
        let old = SelectObject(dc, bmp);
        let bl = Blend { op: 0, flags: 0, alpha: 255, format: 1 };
        UpdateLayeredWindow(hwnd, 0, &[x, y], &[img.w as i32, img.h as i32], dc, &[0, 0], 0, &bl, 2);
        SelectObject(dc, old);
        DeleteDC(dc);
        DeleteObject(bmp);
    }
}

struct MenuState {
    x: i32,
    y: i32,
    hover: Option<usize>,
    /// (y0, y1, действие) в пикселях окна; 0..4 режимы, 10 открыть, 11 выход
    rows: Vec<(f32, f32, usize)>,
}
static MENU: Mutex<MenuState> = Mutex::new(MenuState { x: 0, y: 0, hover: None, rows: Vec::new() });

fn menu_image(hover: Option<usize>, rows: &mut Vec<(f32, f32, usize)>) -> Canvas {
    let s = scale();
    let (wd, ih) = (420.0 * s, 58.0 * s);
    let hd = 16.0 * s + 44.0 * s + ih * 5.0 + 14.0 * s + 2.0 * 40.0 * s + 10.0 * s;
    let mut c = Canvas::new(wd.ceil() as usize, hd.ceil() as usize);
    rows.clear();
    c.rrect(0.5, 0.5, wd - 0.5, hd - 0.5, 12.0 * s, 0.0, PANEL);
    c.rrect(0.5, 0.5, wd - 0.5, hd - 0.5, 12.0 * s, 1.0 * s, LINE);
    let (pct, ac, life) = power();
    let cur = mode();
    let (Some(semi), Some(reg)) = (font_semi(), font_reg()) else { return c };
    c.text(semi, &format!("Заряд {pct}% · {}", if ac { "от сети" } else { "от батареи" }), 14.0 * s, 18.0 * s, 24.0 * s, TEXT);
    let sub = match life {
        Some(sec) => format!("Хватит примерно на {} ч {} мин", sec / 3600, sec % 3600 / 60),
        None if ac => "Питание подключено".to_string(),
        None => "Оцениваю время работы…".to_string(),
    };
    c.text(reg, &sub, 12.0 * s, 18.0 * s, 44.0 * s, DIM);
    let mut y = 60.0 * s;
    for i in 0..5 {
        let (y0, y1) = (y, y + ih - 6.0 * s);
        let on = cur == Some(i as u8);
        let hov = hover == Some(i);
        let bg = if on { rgba(17, 37, 44, 1.0) } else if hov { rgba(32, 36, 45, 1.0) } else { PANEL };
        if on || hov {
            c.rrect(8.0 * s, y0, wd - 8.0 * s, y1, 9.0 * s, 0.0, bg);
        }
        if on {
            c.rrect(8.0 * s, y0, wd - 8.0 * s, y1, 9.0 * s, 1.0 * s, ACC);
        }
        let col = if on { ACC } else if hov { TEXT } else { DIM };
        mode_icon(&mut c, i, 36.0 * s, (y0 + y1) / 2.0, 0.85 * s, col, bg);
        c.text(semi, SHORT[i], 14.5 * s, 64.0 * s, y0 + 16.0 * s, if on || hov { TEXT } else { rgba(205, 210, 218, 1.0) });
        c.text(reg, DESC[i], 11.5 * s, 64.0 * s, y0 + 35.0 * s, DIM);
        if on {
            let (bx0, bx1) = (wd - 82.0 * s, wd - 20.0 * s);
            let cy = (y0 + y1) / 2.0;
            c.rrect(bx0, cy - 10.0 * s, bx1, cy + 10.0 * s, 10.0 * s, 0.0, rgba(20, 70, 85, 1.0));
            c.text_center(reg, "сейчас", 11.5 * s, (bx0 + bx1) / 2.0, cy, ACC);
        }
        rows.push((y0, y1, i));
        y += ih;
    }
    c.rrect(16.0 * s, y + 4.0 * s, wd - 16.0 * s, y + 5.0 * s, 0.0, 0.0, LINE);
    y += 12.0 * s;
    for (n, act) in [("Открыть программу", 10usize), ("Выход", 11)] {
        let (y0, y1) = (y, y + 36.0 * s);
        if hover == Some(act) {
            c.rrect(8.0 * s, y0, wd - 8.0 * s, y1, 8.0 * s, 0.0, rgba(32, 36, 45, 1.0));
        }
        c.text(reg, n, 13.5 * s, 24.0 * s, (y0 + y1) / 2.0, rgba(215, 219, 226, 1.0));
        rows.push((y0, y1, act));
        y += 40.0 * s;
    }
    c
}

fn menu_redraw() {
    let h = HWND_MENU.load(Ordering::Relaxed);
    let Ok(mut m) = MENU.lock() else { return };
    let mut rows = Vec::new();
    let img = menu_image(m.hover, &mut rows);
    m.rows = rows;
    present(h, &img, m.x, m.y);
}

fn menu_show() {
    let h = HWND_MENU.load(Ordering::Relaxed);
    if h == 0 {
        return;
    }
    let mut rows = Vec::new();
    let img = menu_image(None, &mut rows);
    let mut p = [0i32; 2];
    unsafe { GetCursorPos(&mut p) };
    let wa = work_area();
    let x = (p[0] - img.w as i32).clamp(wa[0] + 8, wa[2] - img.w as i32 - 8);
    let y = (p[1] - img.h as i32 - 8).clamp(wa[1] + 8, wa[3] - img.h as i32 - 8);
    if let Ok(mut m) = MENU.lock() {
        *m = MenuState { x, y, hover: None, rows };
    }
    present(h, &img, x, y);
    unsafe {
        ShowWindow(h, 5); // SW_SHOW
        SetForegroundWindow(h);
    }
}

fn menu_hide() {
    let h = HWND_MENU.load(Ordering::Relaxed);
    unsafe { ShowWindow(h, 0) };
}

// ------------------------------------------------------------------ окна
fn main_window() -> isize {
    let n = w("Open Alienware Command Center");
    unsafe { FindWindowW(std::ptr::null(), n.as_ptr()) }
}

/// Открыть окно программы: если уже открыто — показать, иначе запустить.
pub fn open_window() {
    let h = main_window();
    if h != 0 {
        unsafe {
            ShowWindow(h, 5); // SW_SHOW
            ShowWindow(h, 9); // SW_RESTORE
            SetForegroundWindow(h);
        }
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--no-elevate").spawn();
    }
}

static TASKBAR_CREATED: AtomicIsize = AtomicIsize::new(0);

extern "system" fn tray_proc(h: isize, m: u32, wp: usize, lp: isize) -> isize {
    if m == WM_TRAY {
        match (lp & 0xFFFF) as u32 {
            WM_LBUTTONUP => {
                // «Энергосбережение» ↔ «Баланс»; режим видно по листику на значке
                let next = if mode() == Some(0) { 2u8 } else { 0 };
                set_mode(next);
                refresh(h, false);
            }
            WM_RBUTTONUP => menu_show(),
            _ => {}
        }
        return 0;
    }
    if m == WM_TIMER {
        refresh(h, false);
        return 0;
    }
    if m == 0x004A {
        // команда от окна программы
        if let Some(msg) = crate::ipc::parse(lp) {
            crate::service::dispatch(msg);
        }
        return 1;
    }
    if m as isize == TASKBAR_CREATED.load(Ordering::Relaxed) {
        // проводник перезапустился — добавляем значок заново
        if let Ok(mut sh) = SHOWN.lock() {
            sh.on = false;
        }
        refresh(h, true);
        return 0;
    }
    unsafe { DefWindowProcW(h, m, wp, lp) }
}

fn menu_hit(lp: isize) -> Option<usize> {
    let y = ((lp >> 16) & 0xFFFF) as i16 as f32;
    let x = (lp & 0xFFFF) as i16 as f32;
    let m = MENU.lock().ok()?;
    if x < 0.0 {
        return None;
    }
    m.rows.iter().find(|(y0, y1, _)| y >= *y0 && y <= *y1).map(|r| r.2)
}

extern "system" fn menu_proc(h: isize, m: u32, wp: usize, lp: isize) -> isize {
    match m {
        WM_MOUSEMOVE => {
            let hit = menu_hit(lp);
            let changed = MENU.lock().map(|mut s| {
                let c = s.hover != hit;
                s.hover = hit;
                c
            });
            if changed.unwrap_or(false) {
                menu_redraw();
            }
            let mut t = TrackMouse { size: std::mem::size_of::<TrackMouse>() as u32, flags: 2, hwnd: h, hover: 0 }; // TME_LEAVE
            unsafe { TrackMouseEvent(&mut t) };
            return 0;
        }
        WM_MOUSELEAVE => {
            if let Ok(mut s) = MENU.lock() {
                s.hover = None;
            }
            menu_redraw();
            return 0;
        }
        WM_LBUTTONUP => {
            let hit = menu_hit(lp);
            menu_hide();
            match hit {
                Some(i @ 0..=4) => {
                    set_mode(i as u8);
                    refresh(HWND_TRAY.load(Ordering::Relaxed), false);
                }
                Some(10) => open_window(),
                Some(11) => crate::service::quit(),
                _ => {}
            }
            return 0;
        }
        WM_ACTIVATE => {
            if wp & 0xFFFF == 0 {
                menu_hide();
            }
            return 0;
        }
        WM_SETCURSOR => {
            unsafe { SetCursor(LoadCursorW(0, 32512 as *const u16)) }; // стрелка
            return 1;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(h, m, wp, lp) }
}

fn class(name: &[u16], proc_: extern "system" fn(isize, u32, usize, isize) -> isize, inst: isize) {
    let wc = WndClass { style: 0, proc_, cls_extra: 0, wnd_extra: 0, instance: inst, icon: 0, cursor: 0, background: 0, menu_name: std::ptr::null(), class_name: name.as_ptr() };
    unsafe { RegisterClassW(&wc) };
}

pub fn start(ptx: Sender<Cmd>, sensors: Arc<Mutex<Sensors>>) {
    let _ = CTX.set(Mutex::new(Ctx { ptx, sensors }));
    std::thread::spawn(|| unsafe {
        let inst = GetModuleHandleW(std::ptr::null());
        let (ct, cm) = (w("OAWCC_Tray"), w("OAWCC_Menu"));
        class(&ct, tray_proc, inst);
        class(&cm, menu_proc, inst);
        let h = CreateWindowExW(0, ct.as_ptr(), ct.as_ptr(), 0, 0, 0, 0, 0, 0, 0, inst, std::ptr::null());
        // WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST, WS_POPUP
        let hm = CreateWindowExW(0x80000 | 0x80 | 0x8, cm.as_ptr(), cm.as_ptr(), 0x8000_0000, 0, 0, 10, 10, 0, 0, inst, std::ptr::null());
        if h == 0 {
            log::write("Трей: окно не создалось");
            return;
        }
        HWND_TRAY.store(h, Ordering::Relaxed);
        HWND_MENU.store(hm, Ordering::Relaxed);
        let tc = w("TaskbarCreated");
        TASKBAR_CREATED.store(RegisterWindowMessageW(tc.as_ptr()) as isize, Ordering::Relaxed);
        refresh(h, true);
        log::write(&format!("Трей: значок добавлен (размер {} точек)", icon_size()));
        SetTimer(h, 1, 2000, std::ptr::null());
        let mut msg: Msg = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

/// Сразу обновить значок (после смены режима или переключателя показа).
pub fn poke() {
    let h = HWND_TRAY.load(Ordering::Relaxed);
    if h != 0 {
        unsafe { PostMessageW(h, WM_TIMER, 1, 0) };
    }
}

/// Убрать значок при выходе из программы.
pub fn remove() {
    let h = HWND_TRAY.load(Ordering::Relaxed);
    if h != 0 {
        unsafe { Shell_NotifyIconW(NIM_DELETE, &data(h)) };
    }
}
