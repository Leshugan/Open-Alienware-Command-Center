//! Окно программы.
use crate::keyboard::{Rgb, KEYS};
use crate::glyphs;
use crate::layout;
use crate::state::{self, Scheme, State};
use crate::worker::{Cmd, Status};
use eframe::egui::{self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Последняя строка журнала фоновой части (для строки состояния).
static LAST_LOG: Mutex<String> = Mutex::new(String::new());

const BG: Color32 = Color32::from_rgb(15, 17, 21);
const PANEL: Color32 = Color32::from_rgb(23, 26, 32);
const PANEL2: Color32 = Color32::from_rgb(29, 33, 41);
const LINE: Color32 = Color32::from_rgb(42, 47, 57);
const TEXT: Color32 = Color32::from_rgb(231, 234, 240);
const DIM: Color32 = Color32::from_rgb(139, 147, 163);
const ACC: Color32 = Color32::from_rgb(33, 212, 253);
const KEY_BG: Color32 = Color32::from_rgb(13, 15, 19);
const KEY_LINE: Color32 = Color32::from_rgb(35, 40, 51);
const OFF: Color32 = Color32::from_rgb(58, 63, 74);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Zone {
    Key(u8),
    Emblem,
    Contour,
}

pub struct App {
    st: State,
    keys: [Option<Rgb>; KEYS],
    /// цвета цифрового блока при выключенном Num Lock
    pad_off: [Option<Rgb>; KEYS],
    kbri: [u8; KEYS],
    emblem: Rgb,
    contour: Rgb,
    ebri: u8,
    cbri: u8,
    sel: BTreeSet<Zone>,
    hsv: [f32; 3],
    hex: String,
    drag_from: Option<Pos2>,
    tx: crate::ipc::Kb,
    status: Arc<Mutex<Status>>,
    kb_dirty: bool,
    pad_dirty: bool,
    body_dirty: bool,
    last_send: Instant,
    last_change: Option<Instant>,
    scheme_name: String,
    naming: bool,
    /// открыта вкладка «Реакция на нажатия»
    eff_tab: bool,
    /// открытая вкладка: 0 подсветка, 1 производительность (режим и датчики), 2 настройки
    tab: u8,
    /// выбранный режим питания (пока ноутбук не ответил)
    power: u8,
    ptx: crate::ipc::Pw,
    sensors: Arc<Mutex<crate::power::Sensors>>,
    watching: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        setup_style(&cc.egui_ctx);
        let st = State::load();
        crate::log::quiet(st.debug);
        let keys = st.key_colors();
        let mut pad_off = [None; KEYS];
        for (id, h) in &st.numpad_off {
            if (*id as usize) < KEYS && layout::is_numpad(*id) {
                pad_off[*id as usize] = state::from_hex(h);
            }
        }
        let kbri = st.key_bri();
        let (ebri, cbri) = (st.emblem_bri, st.contour_bri);
        let emblem = state::from_hex(&st.emblem).unwrap_or([0, 240, 240]);
        let contour = state::from_hex(&st.contour).unwrap_or([0, 240, 240]);
        // подсветкой, режимами и треем занимается фоновая часть — окно только посылает ей команды
        let (tx, ptx) = (crate::ipc::Kb, crate::ipc::Pw);
        let status = Arc::new(Mutex::new(Status::default()));
        let sensors = Arc::new(Mutex::new(crate::power::Sensors::default()));
        crate::hook::set(&st.swapped);
        let mut app = App {
            st,
            keys,
            pad_off,
            kbri,
            emblem,
            contour,
            ebri,
            cbri,
            sel: BTreeSet::new(),
            hsv: [0.5, 1.0, 1.0],
            hex: String::new(),
            drag_from: None,
            tx,
            status,
            kb_dirty: false,
            pad_dirty: false,
            body_dirty: false,
            last_send: Instant::now(),
            last_change: None,
            scheme_name: String::new(),
            naming: false,
            eff_tab: false,
            tab: 0,
            power: 2,
            ptx,
            sensors,
            watching: false,
        };
        app.hex = "00F0F0".into();
        app.eff_tab = crate::reactive::is_reactive(app.st.effect.kind);
        app.tab = app.st.tab.min(2);
        crate::autostart::refresh_path();
        cc.egui_ctx.request_repaint();
        // при запуске один раз включаем свои цвета — клавиатура приходит в известное состояние


        app
    }

    fn num_off(&self) -> bool {
        !crate::hook::NUM.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Цвет клавиши, как он горит сейчас (у цифрового блока зависит от Num Lock).
    fn key_now(&self, id: u8) -> Option<Rgb> {
        let i = id as usize;
        if self.num_off() && layout::is_numpad(id) {
            if let Some(c) = self.pad_off[i] {
                return self.keys[i].map(|_| c);
            }
        }
        self.keys[i]
    }

    fn zone_color(&self, z: Zone) -> Option<Rgb> {
        match z {
            Zone::Key(id) => self.key_now(id),
            Zone::Emblem => Some(self.emblem),
            Zone::Contour => Some(self.contour),
        }
    }

    fn select(&mut self, zones: &[Zone], add: bool) {
        if !add {
            self.sel.clear();
        }
        for z in zones {
            // при эффекте клавиши не выбираются — красится вся клавиатура
            if self.st.effect.kind != 0 && matches!(z, Zone::Key(_)) {
                continue;
            }
            if add && self.sel.contains(z) && zones.len() == 1 {
                self.sel.remove(z);
            } else {
                self.sel.insert(*z);
            }
        }
        if let Some(c) = self.sel.iter().find_map(|z| self.zone_color(*z)) {
            self.hsv = rgb_to_hsv(c);
            self.hex = state::to_hex(c);
        }
    }

    fn set_color(&mut self, c: Rgb) {
        self.hex = state::to_hex(c);
        // при эффекте с цветом и без выбранной эмблемы/контура — меняем цвет эффекта
        if effect_has_color(self.st.effect.kind) && self.sel.is_empty() {
            self.st.effect.color = state::to_hex(c);
            self.push_effect();
            return;
        }
        let sel: Vec<Zone> = self.sel.iter().copied().collect();
        for z in sel {
            match z {
                Zone::Key(id) => {
                    // Num Lock выключен — красим «второй» цвет цифрового блока
                    if self.num_off() && layout::is_numpad(id) {
                        self.pad_off[id as usize] = Some(c);
                        if self.keys[id as usize].is_none() {
                            self.keys[id as usize] = Some(c);
                        }
                        self.pad_dirty = true;
                    } else {
                        self.keys[id as usize] = Some(c);
                    }
                    self.kb_dirty = true;
                }
                Zone::Emblem => {
                    self.emblem = c;
                    self.body_dirty = true;
                }
                Zone::Contour => {
                    self.contour = c;
                    self.body_dirty = true;
                }
            }
        }
        self.last_change = Some(Instant::now());
    }

    /// Отправить эффект на клавиатуру (или вернуть свои цвета).
    fn push_effect(&mut self) {
        let e = &self.st.effect;
        crate::hook::REACTIVE.store(crate::reactive::is_reactive(e.kind), std::sync::atomic::Ordering::Relaxed);
        if e.kind == 0 {
            self.kb_dirty = true;
        } else if crate::reactive::is_reactive(e.kind) {
            let c = state::from_hex(&e.color).unwrap_or([0, 240, 240]);
            let bg = if e.bg { self.keys_out() } else { [None; KEYS] };
            let _ = self.tx.send(Cmd::Reactive(e.kind, e.speed, c, Box::new(bg)));
        } else {
            let c = state::from_hex(&e.color).unwrap_or([0, 240, 240]);
            let _ = self.tx.send(Cmd::Effect(e.kind, e.speed, c, e.dir));
        }
        self.last_change = Some(Instant::now());
    }

    fn zone_bri(&self, z: Zone) -> u8 {
        match z {
            Zone::Key(id) => self.kbri[id as usize],
            Zone::Emblem => self.ebri,
            Zone::Contour => self.cbri,
        }
    }

    /// Цвета клавиш с учётом яркости каждой клавиши — то, что уходит на ноутбук.
    fn keys_out(&self) -> [Option<Rgb>; KEYS] {
        let mut o = self.keys;
        for (i, c) in o.iter_mut().enumerate() {
            if let Some(c) = c {
                *c = shade(*c, self.kbri[i] as f32 / 100.0);
            }
        }
        o
    }

    /// Цвета цифрового блока при выключенном Num Lock — с яркостью клавиш.
    fn pad_out(&self) -> [Option<Rgb>; KEYS] {
        let mut o = self.pad_off;
        for (i, c) in o.iter_mut().enumerate() {
            if let Some(c) = c {
                *c = shade(*c, self.kbri[i] as f32 / 100.0);
            }
        }
        o
    }

    fn body_out(&self) -> (Rgb, Rgb) {
        (shade(self.emblem, self.ebri as f32 / 100.0), shade(self.contour, self.cbri as f32 / 100.0))
    }

    fn set_bri(&mut self, b: u8) {
        let sel: Vec<Zone> = self.sel.iter().copied().collect();
        for z in sel {
            match z {
                Zone::Key(id) => {
                    self.kbri[id as usize] = b;
                    self.kb_dirty = true;
                }
                Zone::Emblem => {
                    self.ebri = b;
                    self.body_dirty = true;
                }
                Zone::Contour => {
                    self.cbri = b;
                    self.body_dirty = true;
                }
            }
        }
        self.last_change = Some(Instant::now());
    }

    fn push(&mut self, force: bool) {
        if !force && self.last_send.elapsed() < Duration::from_millis(70) {
            return;
        }
        if self.pad_dirty {
            let _ = self.tx.send(Cmd::Numpad(Box::new(self.pad_out())));
            self.pad_dirty = false;
        }
        if self.kb_dirty {
            let _ = self.tx.send(Cmd::Keys(Box::new(self.keys_out()), 100));
            self.kb_dirty = false;
        }
        if self.body_dirty {
            let (e, c) = self.body_out();
            let _ = self.tx.send(Cmd::Body(e, c, 100));
            self.body_dirty = false;
        }
        self.last_send = Instant::now();
    }

    /// Забрать состояние у фоновой части (датчики, Fn Lock, Num Lock, нажатия).
    fn sync(&mut self) {
        let Some(s) = crate::ipc::read() else {
            if let Ok(mut st) = self.status.lock() {
                st.started = false;
            }
            return;
        };
        if let Ok(mut st) = self.status.lock() {
            *st = Status { started: s.started, keyboard: s.keyboard, body: s.body, fn_lock: s.fn_lock, touchpad: s.touchpad };
        }
        if let Ok(mut p) = self.sensors.lock() {
            *p = crate::power::Sensors { ok: s.power_ok, mode: s.mode, cpu_temp: s.cpu_temp, gpu_temp: s.gpu_temp, cpu_load: s.cpu_load, gpu_load: s.gpu_load, fans: s.fans, history: s.history };
        }
        crate::hook::NUM.store(s.num, std::sync::atomic::Ordering::Relaxed);
        crate::hook::CAPS.store(s.caps, std::sync::atomic::Ordering::Relaxed);
        crate::reactive::import(&s.hits, &s.heat);
        if let Ok(mut l) = LAST_LOG.lock() {
            *l = s.last_log;
        }
    }

    /// Сохранить сразу (при закрытии окна).
    fn persist_now(&mut self) {
        if self.last_change.is_some() {
            self.push(true);
            self.last_change = Some(Instant::now() - Duration::from_secs(5));
            self.persist();
        }
    }

    fn persist(&mut self) {
        if let Some(t) = self.last_change {
            if t.elapsed() > Duration::from_millis(1500) && !self.kb_dirty && !self.body_dirty {
                self.last_change = None;
                self.st.set_key_colors(&self.keys);
                self.st.numpad_off = self.pad_off.iter().enumerate().filter_map(|(i, c)| c.map(|c| (i as u8, state::to_hex(c)))).collect();
                self.st.set_key_bri(&self.kbri);
                self.st.emblem_bri = self.ebri;
                self.st.contour_bri = self.cbri;
                self.st.emblem = state::to_hex(self.emblem);
                self.st.contour = state::to_hex(self.contour);
                self.st.save();
                let (e, c) = self.body_out();
                // при эффекте свои цвета на клавиатуру не шлём — иначе они мелькают поверх эффекта
                let keys = if self.st.effect.kind != 0 { None } else { Some(Box::new(self.keys_out())) };
                let _ = self.tx.send(Cmd::Save(keys, e, c, 100));
            }
        }
    }
}

impl eframe::App for App {
    /// При выходе «реакция на нажатия» останавливается — возвращаем свои цвета.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist_now();
        let _ = self.ptx.send(crate::power::Cmd::Watch(false));
        // без значка в трее и автозапуска фон никому не нужен — закрываем и его
        if !self.st.tray && crate::autostart::is_on() != Some(true) {
            let _ = crate::ipc::send(&crate::ipc::Msg::Quit);
        }
    }


    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync();
        egui::TopBottomPanel::top("tabs")
            .show_separator_line(false)
            .frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin { left: 18, right: 18, top: 12, bottom: 0 }))
            .show(ctx, |ui| self.tabs(ui));
        if self.watching != (self.tab == 1) {
            self.watching = self.tab == 1;
            let _ = self.ptx.send(crate::power::Cmd::Watch(self.watching));
        }
        if self.tab == 2 {
            egui::CentralPanel::default()
                .frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin::same(18)))
                .show(ctx, |ui| self.settings_page(ui));
            ctx.request_repaint_after(Duration::from_millis(500));
            return;
        }
        if self.tab == 1 {
            egui::CentralPanel::default()
                .frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin::same(18)))
                .show(ctx, |ui| self.power_page(ui));
            ctx.request_repaint_after(Duration::from_millis(500));
            return;
        }
        egui::SidePanel::right("right")
            .exact_width(350.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin { left: 0, right: 18, top: 18, bottom: 18 }))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.right_panel(ui));
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin::same(18)))
            .show(ctx, |ui| self.left_panel(ui));
        self.push(false);
        self.persist();
        if self.kb_dirty || self.body_dirty || self.last_change.is_some() || self.st.effect.kind != 0 {
            ctx.request_repaint_after(Duration::from_millis(if self.st.effect.kind != 0 { 33 } else { 80 }));
        } else {
            ctx.request_repaint_after(Duration::from_millis(66));
        }
    }
}

// ---------------------------------------------------------------- вкладки и питание

impl App {
    fn tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (i, n) in ["Подсветка", "Производительность", "Настройки"].iter().enumerate() {
                let i = i as u8;
                let on = self.tab == i;
                let ready = true;
                let g = ui.painter().layout_no_wrap(n.to_string(), FontId::proportional(14.0), TEXT);
                let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 32.0, 34.0), Sense::click());
                let fill = if on { with_alpha(ACC, 38) } else if resp.hovered() && ready { PANEL2 } else { BG };
                ui.painter().rect_filled(r, CornerRadius::same(9), fill);
                if on {
                    ui.painter().rect_stroke(r, CornerRadius::same(9), Stroke::new(1.0_f32, with_alpha(ACC, 150)), StrokeKind::Inside);
                }
                let col = if on { TEXT } else if ready { DIM } else { OFF };
                ui.painter().text(r.center(), Align2::CENTER_CENTER, *n, FontId::proportional(14.0), col);
                if ready && resp.clicked() && self.tab != i {
                    self.tab = i;
                    self.st.tab = i;
                    self.st.save();
                }
                if !ready {
                    resp.on_hover_text("Скоро");
                }
            }
        });
    }

    fn power_page(&mut self, ui: &mut egui::Ui) {
        let sn = self.sensors.lock().map(|s| s.clone()).unwrap_or_default();
        if let Some(m) = sn.mode {
            self.power = m;
        }
        // (код, название, описание, мощность 1..5, шум 1..5)
        let modes: [(u8, &str, &str, u8, u8); 5] = [
            (0, "Энергосбережение", "Дольше без зарядки. Мощность и шум — минимум.", 1, 1),
            (1, "Тихий", "Почти не слышно. Браузер, кино, работа.", 2, 1),
            (2, "Баланс", "Обычный режим на каждый день.", 3, 3),
            (3, "Производительность", "Больше мощности для игр. Вентиляторы громче.", 4, 4),
            (4, "Максимум", "Вся мощность. Вентиляторы на полную.", 5, 5),
        ];
        // режимы — пять компактных карточек с описанием и шкалами
        card_fit(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Режим").size(17.0).strong().color(TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = self.st.tray;
                    if ui.checkbox(&mut on, "Значок батареи в трее").on_hover_text("Левый клик по значку — «Энергосбережение» и обратно «Баланс» (в этом режиме на значке листик). Правый — все режимы. Крестик закрывает окно, а программа продолжает работать в трее.").changed() {
                        self.st.tray = on;
                        self.st.save();
                        let _ = crate::ipc::send(&crate::ipc::Msg::Tray(on));
                    }
                });
            });
            ui.add_space(8.0);
            let gap = 10.0;
            let w = (ui.available_width() - gap * 4.0) / 5.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (code, name, desc, pw, noise) in modes {
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 132.0), Sense::click());
                    let on = self.power == code;
                    let p = ui.painter();
                    p.rect_filled(r, CornerRadius::same(10), if on { Color32::from_rgb(17, 37, 44) } else if resp.hovered() { PANEL2 } else { Color32::from_rgb(26, 30, 38) });
                    p.rect_stroke(r, CornerRadius::same(10), Stroke::new(if on { 1.8_f32 } else { 1.0 }, if on { ACC } else { LINE }), StrokeKind::Inside);
                    let col = if on { ACC } else { DIM };
                    power_icon(p, code, r.left_top() + vec2(24.0, 24.0), col, 0.8);
                    p.text(r.left_top() + vec2(44.0, 24.0), Align2::LEFT_CENTER, name, FontId::proportional(14.5), if on { TEXT } else { Color32::from_rgb(200, 205, 214) });
                    let g = p.layout(desc.to_string(), FontId::proportional(11.5), DIM, w - 28.0);
                    p.galley(r.left_top() + vec2(14.0, 44.0), g, DIM);
                    for (row, (label, v)) in [("Мощность", pw), ("Шум", noise)].iter().enumerate() {
                        let y = r.bottom() - 34.0 + row as f32 * 18.0;
                        p.text(pos2(r.left() + 14.0, y), Align2::LEFT_CENTER, *label, FontId::proportional(11.0), DIM);
                        for i in 0..5u8 {
                            let b = Rect::from_min_size(pos2(r.right() - 14.0 - (5 - i) as f32 * 13.0, y - 3.5), vec2(10.0, 7.0));
                            p.rect_filled(b, CornerRadius::same(2), if i < *v { if on { ACC } else { Color32::from_rgb(120, 128, 142) } } else { Color32::from_rgb(40, 45, 55) });
                        }
                    }
                    if resp.clicked() && self.power != code {
                        self.power = code;
                        let _ = self.ptx.send(crate::power::Cmd::SetMode(code));
                        if let Ok(mut s) = self.sensors.lock() {
                            s.mode = Some(code);
                        }
                    }
                }
            });
            if !sn.ok {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Нет связи с ноутбуком: программе нужны права администратора.").size(12.0).color(Color32::from_rgb(255, 120, 100)));
            }
        });
        ui.add_space(14.0);
        // мониторинг: плитки и график
        let opt = |v: Option<u32>, s: &str| v.map(|v| format!("{v}{s}")).unwrap_or("—".into());
        let orange = Color32::from_rgb(255, 140, 60);
        let green = Color32::from_rgb(120, 220, 90);
        let mut tiles: Vec<(String, String, String, f32, Color32)> = vec![
            ("Процессор".into(), opt(sn.cpu_temp, "°"), sn.cpu_load.map(|l| format!("нагрузка {l:.0}%")).unwrap_or_default(), sn.cpu_temp.unwrap_or(0) as f32 / 100.0, orange),
            ("Видеокарта".into(), opt(sn.gpu_temp, "°"), sn.gpu_load.map(|l| format!("нагрузка {l}%")).unwrap_or_default(), sn.gpu_temp.unwrap_or(0) as f32 / 100.0, green),
        ];
        for (i, (rpm, max)) in sn.fans.iter().enumerate() {
            let small = if *rpm == 0 { "об/мин · стоит".to_string() } else { "об/мин".to_string() };
            tiles.push((format!("Вентилятор {}", i + 1), rpm.to_string(), small, *rpm as f32 / (*max).max(1) as f32, ACC));
        }
        let gap = 14.0;
        let n = tiles.len() as f32;
        let tw = (ui.available_width() - gap * (n - 1.0)) / n;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (name, big, small, frac, col) in &tiles {
                let (r, _) = ui.allocate_exact_size(vec2(tw, 132.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(r, CornerRadius::same(14), PANEL);
                p.rect_stroke(r, CornerRadius::same(14), Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
                p.text(r.left_top() + vec2(16.0, 14.0), Align2::LEFT_TOP, name, FontId::proportional(13.0), DIM);
                p.text(r.left_top() + vec2(16.0, 38.0), Align2::LEFT_TOP, big, FontId::proportional(34.0), TEXT);
                p.text(r.left_top() + vec2(16.0, 84.0), Align2::LEFT_TOP, small, FontId::proportional(12.5), DIM);
                let track = Rect::from_min_size(r.left_bottom() + vec2(16.0, -20.0), vec2(r.width() - 32.0, 5.0));
                p.rect_filled(track, CornerRadius::same(3), Color32::from_rgb(40, 45, 55));
                p.rect_filled(Rect::from_min_size(track.min, vec2(track.width() * frac.clamp(0.0, 1.0), 5.0)), CornerRadius::same(3), *col);
            }
        });
        ui.add_space(14.0);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Температура за 2 минуты").size(15.0).strong().color(TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("● видеокарта").size(12.0).color(Color32::from_rgb(120, 220, 90)));
                    ui.label(egui::RichText::new("● процессор").size(12.0).color(Color32::from_rgb(255, 140, 60)));
                });
            });
            ui.add_space(6.0);
            let h = (ui.available_height() - 8.0).max(120.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
            let p = ui.painter();
            for i in 0..=4 {
                let y = r.top() + r.height() * i as f32 / 4.0;
                p.line_segment([pos2(r.left() + 34.0, y), pos2(r.right(), y)], Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 47)));
                p.text(pos2(r.left(), y), Align2::LEFT_CENTER, format!("{}°", 100 - i * 25), FontId::proportional(11.0), DIM);
            }
            let n = 120usize;
            let x = |i: usize| r.left() + 34.0 + (r.width() - 34.0) * i as f32 / (n - 1) as f32;
            let y = |v: f32| r.bottom() - r.height() * v.clamp(0.0, 100.0) / 100.0;
            let off = n.saturating_sub(sn.history.len());
            for (k, col) in [(0usize, orange), (1, green)] {
                let pts: Vec<Pos2> = sn.history.iter().enumerate().map(|(i, h)| pos2(x(off + i), y(if k == 0 { h.0 } else { h.1 }))).collect();
                if pts.len() > 1 {
                    p.add(Shape::line(pts, Stroke::new(2.0_f32, col)));
                }
            }
            if sn.history.len() < 2 {
                p.text(r.center(), Align2::CENTER_CENTER, "собираю данные…", FontId::proportional(13.0), DIM);
            }
        });
    }
}

impl App {
    /// Предупреждение: функция работает, только пока программа запущена.
    fn autostart_note(&mut self, ui: &mut egui::Ui, why: &str) {
        let on = crate::autostart::is_on();
        if on == Some(true) {
            ui.label(egui::RichText::new(format!("{why} Автозапуск включён — работает всегда.")).size(11.5).color(Color32::from_rgb(120, 220, 140)));
            return;
        }
        egui::Frame::default().fill(Color32::from_rgb(48, 38, 20)).stroke(Stroke::new(1.0_f32, Color32::from_rgb(120, 90, 30))).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(format!("{why} Работает, только пока программа запущена — после перезагрузки сам не включится.")).size(11.5).color(Color32::from_rgb(240, 200, 120)));
            if ui.button("Включить автозапуск").clicked() {
                crate::autostart::set(true);
            }
        });
    }

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        card_fit(ui, |ui| {
            ui.label(egui::RichText::new("Автозапуск").size(17.0).strong().color(TEXT));
            ui.add_space(6.0);
            let state = crate::autostart::is_on();
            let mut on = state.unwrap_or(false);
            ui.add_enabled_ui(state.is_some(), |ui| {
                if ui.checkbox(&mut on, egui::RichText::new("Запускать вместе с Windows — тихо, в фоне").size(14.0)).changed() {
                    crate::autostart::set(on);
                }
            });
            ui.add_space(8.0);
            let p = |ui: &mut egui::Ui, t: &str, c: Color32| {
                ui.label(egui::RichText::new(t).size(12.5).color(c));
            };
            p(ui, "Зачем это нужно. Свои цвета, обычные эффекты (дыхание, спектр, волна, сканер) и режим питания хранятся в самом ноутбуке — они работают и без программы, даже если её удалить.", DIM);
            ui.add_space(4.0);
            p(ui, "А вот это делает сама программа, и работает оно, только пока она запущена:", DIM);
            for t in [
                "•  эффекты «На нажатия» (круг, крест, брызги, тепловая карта…);",
                "•  Fn-выключение подсветки гасит заодно эмблему, контур и кнопку питания;",
                "•  поменянные местами функции HOME / END / DEL (правый клик по клавише);",
                "•  значок батареи в трее и переключение режимов из него.",
            ] {
                p(ui, t, Color32::from_rgb(205, 210, 218));
            }
            ui.add_space(4.0);
            p(ui, "С автозапуском программа сама стартует при входе в Windows, окно не открывается. Если значок в трее выключен, она просто тихо работает в фоне — чтобы открыть окно, запусти программу ещё раз.", DIM);
        });
        ui.add_space(14.0);
        card_fit(ui, |ui| {
            ui.label(egui::RichText::new("Отладка").size(17.0).strong().color(TEXT));
            ui.add_space(6.0);
            let mut on = self.st.debug;
            if ui.checkbox(&mut on, egui::RichText::new("Записывать отладку в файл на рабочем столе").size(14.0)).changed() {
                self.st.debug = on;
                self.st.save();
                crate::log::quiet(on);
                let _ = crate::ipc::send(&crate::ipc::Msg::Debug(on));
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Нужна, только если что-то работает не так: файл «Open Alienware Command Center_debug.txt» покажет, где проблема.").size(12.5).color(DIM));
        });
        ui.add_space(14.0);
        card_fit(ui, |ui| {
            ui.label(egui::RichText::new("О программе").size(17.0).strong().color(TEXT));
            ui.add_space(6.0);
            ui.label(egui::RichText::new(format!("Open Alienware Command Center {}", env!("CARGO_PKG_VERSION"))).size(13.5).color(TEXT));
            ui.label(egui::RichText::new("Лёгкая замена Alienware Command Center без телеметрии и лишних служб.").size(12.5).color(DIM));
        });
    }
}

/// Значки режимов питания (k — масштаб).
fn power_icon(p: &egui::Painter, code: u8, c: Pos2, col: Color32, k: f32) {
    let st = Stroke::new(1.8_f32, col);
    let v = |x: f32, y: f32| c + vec2(x, y) * k;
    match code {
        0 => {
            // листик — энергосбережение
            let (s, co) = (-0.7f32).sin_cos();
            let start = c + vec2(-10.0, 8.0) * k;
            let len = 24.0 * k;
            let mut pts = Vec::new();
            for i in 0..=16 {
                let t = i as f32 / 16.0;
                pts.push((t * len, -len * 0.32 * (std::f32::consts::PI * t).sin()));
            }
            for i in (0..=16).rev() {
                let t = i as f32 / 16.0;
                pts.push((t * len, len * 0.22 * (std::f32::consts::PI * t).sin()));
            }
            let pts: Vec<Pos2> = pts.iter().map(|(x, y)| start + vec2(x * co - y * s, x * s + y * co)).collect();
            p.add(Shape::closed_line(pts, st));
            p.line_segment([start, start + vec2(co, s) * len * 0.85], st);
        }
        1 => {
            // луна
            let mut pts = Vec::new();
            for i in 0..=24 {
                let a = std::f32::consts::PI * (0.25 + 1.5 * i as f32 / 24.0);
                pts.push(v(a.cos() * 11.0, a.sin() * 11.0));
            }
            for i in 0..=24 {
                let a = std::f32::consts::PI * (1.75 - 1.5 * i as f32 / 24.0);
                pts.push(v(5.0 + a.cos() * 8.5, -2.0 + a.sin() * 8.5));
            }
            p.add(Shape::closed_line(pts, st));
        }
        2 => {
            // круг, закрашенный наполовину
            p.circle_stroke(c, 11.0 * k, st);
            let mut pts = vec![v(0.0, -11.0)];
            for i in 0..=16 {
                let a = std::f32::consts::PI * (-0.5 + i as f32 / 16.0);
                pts.push(v(a.cos() * 11.0, a.sin() * 11.0));
            }
            p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
        }
        3 => {
            // молния
            let pts = vec![v(3.0, -13.0), v(-8.0, 2.0), v(-1.0, 2.0), v(-4.0, 13.0), v(8.0, -3.0), v(1.0, -3.0)];
            p.add(Shape::closed_line(pts, st));
        }
        _ => {
            // пламя
            let pts: Vec<Pos2> = [(0.0, -13.0), (5.0, -6.0), (8.0, 0.0), (8.0, 5.0), (5.0, 10.0), (0.0, 12.0), (-5.0, 10.0), (-8.0, 5.0), (-8.0, 0.0), (-5.0, -4.0), (-3.0, 0.0), (-1.0, -6.0)]
                .iter()
                .map(|(x, y)| v(*x, *y))
                .collect();
            p.add(Shape::closed_line(pts, st));
        }
    }
}

// ---------------------------------------------------------------- левая часть

impl App {
    fn left_panel(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Подсветка").size(17.0).strong().color(TEXT));
                ui.add_space(10.0);
                ui.label(egui::RichText::new("нажми на клавишу или зону · Ctrl — несколько · протяни мышью — область · правый клик по HOME, END, DEL — поменять их функции").size(12.5).color(DIM));
            });
            if self.num_off() {
                ui.label(egui::RichText::new("Num Lock выключен — цифровой блок красится своим вторым цветом").size(12.0).color(ACC));
            } else {
                ui.label(egui::RichText::new("Выключи Num Lock, чтобы задать цифровому блоку второй цвет").size(12.0).color(DIM));
            }
            if !self.st.swapped.is_empty() && crate::autostart::is_on() != Some(true) {
                self.autostart_note(ui, "Поменянные местами HOME / END / DEL делает сама программа.");
            }
            ui.add_space(6.0);
            self.keyboard(ui);
            ui.add_space(16.0);
            ui.horizontal_top(|ui| {
                self.rear(ui);
                ui.add_space(20.0);
                ui.vertical(|ui| {
                    caption(ui, "Быстрый выбор");
                    self.chips(ui);
                    ui.add_space(14.0);
                    caption(ui, "Выбрано");
                    self.selection_line(ui);
                    ui.add_space((ui.available_height() - 40.0).max(0.0));
                    self.status_line(ui);
                });
            });
        });
    }

    fn keyboard(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_width();
        let span = layout::RIGHT - layout::LEFT;
        let sc = ((avail - 28.0) / span).min(0.95);
        let h = 74.0 * sc; // высота обычного ряда
        let h0 = 52.0 * sc; // верхний ряд ниже
        let g = 8.0 * sc;
        let width = span * sc + 28.0;
        let height = h0 + 5.0 * (h + g) + g + 28.0;
        let (resp, painter) = ui.allocate_painter(vec2(avail, height), Sense::click_and_drag());
        let deck = Rect::from_min_size(pos2(resp.rect.center().x - width / 2.0, resp.rect.top()), vec2(width, height));
        painter.rect_filled(deck, CornerRadius::same(14), Color32::from_rgb(22, 25, 32));
        painter.rect_stroke(deck, CornerRadius::same(14), Stroke::new(1.0_f32, Color32::from_rgb(43, 49, 60)), StrokeKind::Inside);
        let row_y = |r: u8| if r == 0 { 0.0 } else { h0 + g + (r as f32 - 1.0) * (h + g) };
        let mut rects: Vec<(u8, Rect, &'static str)> = Vec::new();
        for k in layout::keys() {
            let x = deck.left() + 14.0 + (k.x0 - layout::LEFT) * sc;
            let w = (k.x1 - k.x0) * sc;
            let y = deck.top() + 14.0 + row_y(k.row);
            let hh = if k.row == 0 { h0 } else if k.tall { 2.0 * h + g } else { h };
            rects.push((k.id, Rect::from_min_size(pos2(x, y), vec2(w, hh)), k.label));
        }

        let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.shift);
        let hover = resp.hover_pos();
        // выбор
        if resp.drag_started() {
            self.drag_from = resp.interact_pointer_pos();
        }
        if resp.drag_stopped() {
            if let (Some(a), Some(b)) = (self.drag_from, resp.interact_pointer_pos()) {
                let area = Rect::from_two_pos(a, b);
                let hit: Vec<Zone> = rects.iter().filter(|(_, r, _)| r.intersects(area)).map(|(id, _, _)| Zone::Key(*id)).collect();
                if !hit.is_empty() && self.st.effect.kind == 0 {
                    let add = ctrl;
                    if !add {
                        self.sel.clear();
                    }
                    for z in &hit {
                        self.sel.insert(*z);
                    }
                    self.select(&[], true);
                }
            }
            self.drag_from = None;
        }
        // правый клик по клавише с двумя функциями — поменять их местами
        if resp.secondary_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                if let Some((id, _, _)) = rects.iter().find(|(_, r, _)| r.contains(p)) {
                    if crate::hook::can_swap(*id) {
                        let on = !crate::hook::is_swapped(*id);
                        self.st.swapped.retain(|x| x != id);
                        if on {
                            self.st.swapped.push(*id);
                        }
                        crate::hook::set(&self.st.swapped);
                        let _ = crate::ipc::send(&crate::ipc::Msg::Swapped(self.st.swapped.clone()));
                        self.st.save();
                        crate::log::write(&format!("Клавиша {}: функции {}", layout::label(*id), if on { "поменяны местами" } else { "как обычно" }));
                    }
                }
            }
        }
        if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                if let Some((id, _, _)) = rects.iter().find(|(_, r, _)| r.contains(p)) {
                    if self.st.effect.kind == 0 {
                        self.select(&[Zone::Key(*id)], ctrl);
                    } else if crate::reactive::is_reactive(self.st.effect.kind) {
                        let _ = crate::ipc::send(&crate::ipc::Msg::Press(*id));
                    }
                } else if !ctrl {
                    self.sel.clear();
                }
            }
        }

        // рисование
        let locks = Locks::now(&self.status);
        let t = ui.input(|i| i.time) as f32;
        let eff = self.st.effect.clone();
        let ecol = state::from_hex(&eff.color).unwrap_or([0, 240, 240]);
        let rframe = if crate::reactive::is_reactive(eff.kind) {
            let bg = if eff.bg { self.keys_out() } else { [None; KEYS] };
            Some(crate::reactive::frame(eff.kind, ecol, eff.speed, &bg))
        } else {
            None
        };
        for (id, r, label) in &rects {
            let bri = self.kbri[*id as usize] as f32 / 100.0;
            let col = if let Some(fr) = &rframe {
                // тёмная клавиша в окне всё равно видна контуром надписи
                fr[*id as usize].map(|c| if c == [0, 0, 0] { [40, 44, 52] } else { c })
            } else if eff.kind != 0 {
                let x = (r.center().x - deck.left()) / deck.width();
                let y = (r.center().y - deck.top()) / deck.height();
                Some(effect_preview(&eff, ecol, x, y, t))
            } else {
                self.key_now(*id).map(|c| shade(c, 0.35 + 0.65 * bri))
            };
            let sel = self.sel.contains(&Zone::Key(*id));
            let hov = hover.map(|p| r.contains(p)).unwrap_or(false);
            let rr = *r;
            painter.rect_filled(rr, CornerRadius::same(6), if sel { Color32::from_rgb(17, 37, 44) } else { KEY_BG });
            let border = if sel { Stroke::new(2.0_f32, ACC) } else if hov { Stroke::new(1.0_f32, Color32::from_rgb(80, 90, 105)) } else { Stroke::new(1.0_f32, KEY_LINE) };
            painter.rect_stroke(rr, CornerRadius::same(6), border, StrokeKind::Inside);
            let tc = col.map(lift).unwrap_or(OFF);
            let dim = tc.gamma_multiply(0.62);
            let faint = tc.gamma_multiply(0.28);
            let k = (h / 52.0).clamp(0.75, 1.4) * if rr.height() < h * 0.85 { 0.92 } else { 1.0 };
            let f = |s: f32| FontId::proportional(s * k);
            let sec = layout::secondary(*id);
            let ins = rr.shrink2(vec2(6.0, 4.0) * k);
            match *id {
                // верхний ряд как на клавиатуре; замок на ESC горит, когда включён Fn Lock
                0 => {
                    painter.text(ins.left_top(), Align2::LEFT_TOP, *label, f(13.0), tc);
                    glyphs::image(&painter, "lock", rr, glyphs::At::Corner, if locks.fn_lock { tc } else { dim });
                }
                // обе надписи на месте; ярко та, что работает сейчас (зависит от Fn Lock)
                1..=15 => {
                    // что работает: зависит от Fn Lock, а у поменянных правым кликом — наоборот
                    let second = locks.fn_lock != crate::hook::is_swapped(*id);
                    let (c1, c2) = if second { (faint, tc) } else { (tc, faint) };
                    painter.text(ins.left_top(), Align2::LEFT_TOP, *label, f(if label.len() > 3 { 11.5 } else { 13.0 }), c1);
                    if let Some(g) = glyphs::icon_for(*id) {
                        glyphs::image(&painter, g, rr, glyphs::At::Dell, c2);
                    } else {
                        painter.text(ins.right_bottom(), Align2::RIGHT_BOTTOM, sec, f(8.5), c2);
                    }
                }
                16 => glyphs::image(&painter, "mute", rr, glyphs::At::Center(1.35), tc),
                17 => glyphs::image(&painter, "voldn", rr, glyphs::At::Center(1.35), tc),
                18 => glyphs::image(&painter, "volup", rr, glyphs::At::Center(1.35), tc),
                19 => glyphs::image(&painter, "mic", rr, glyphs::At::Center(1.35), tc),
                35 => glyphs::draw(&painter, "back", rr.center(), k * 1.2, tc),
                36 => glyphs::image(&painter, "lock", rr, glyphs::At::Center(1.8), if locks.num { tc } else { dim }),
                74 | 119 => glyphs::draw(&painter, "enter", rr.center(), k * 1.1, tc),
                103 => glyphs::image(&painter, "win", rr, glyphs::At::Center(1.0), tc),
                109 => glyphs::image(&painter, "winlock", rr, glyphs::At::Center(1.0), tc),
                107 => {}
                // цифры: цифра, рядом мелко русский символ, снизу символ при Shift
                20..=32 => {
                    painter.text(ins.left_top(), Align2::LEFT_TOP, *label, f(14.0), tc);
                    let cr = layout::corner(*id);
                    if !cr.is_empty() {
                        painter.text(ins.left_top() + vec2(13.0, 1.0) * k, Align2::LEFT_TOP, cr, f(9.5), tc);
                    }
                    painter.text(ins.left_bottom(), Align2::LEFT_BOTTOM, sec, f(11.0), dim);
                }
                // стрелки вверх/вниз с PAGE UP / PAGE DN
                114 | 134 => {
                    painter.text(rr.center() - vec2(0.0, 6.0) * k, Align2::CENTER_CENTER, *label, f(13.0), tc);
                    painter.text(rr.center() + vec2(0.0, 11.0) * k, Align2::CENTER_CENTER, sec, f(7.0), dim);
                }
                // цифровой блок: зависит от Num Lock
                _ if layout::numpad_nav(*id) => {
                    // что сейчас работает — крупно сверху, вторая функция — мелко снизу
                    let (top, bottom) = if locks.num { (*label, sec) } else { (sec, *label) };
                    let big = if top.chars().count() > 2 { 11.0 } else { 15.0 };
                    let small = if bottom.chars().count() > 2 { 7.5 } else { 10.0 };
                    painter.text(rr.center() - vec2(0.0, 6.0) * k, Align2::CENTER_CENTER, top, f(big), tc);
                    painter.text(rr.center() + vec2(0.0, 12.0) * k, Align2::CENTER_CENTER, bottom, f(small), faint);
                }
                _ if sec.is_empty() => {
                    let n = label.chars().count();
                    let size = if n > 4 { 10.0 } else if n > 1 { 11.0 } else { 14.0 };
                    painter.text(rr.center(), Align2::CENTER_CENTER, *label, f(size), tc);
                }
                // буквы и знаки: латиница сверху слева, русская снизу справа
                _ => {
                    painter.text(ins.left_top(), Align2::LEFT_TOP, *label, f(14.0), tc);
                    painter.text(ins.right_bottom(), Align2::RIGHT_BOTTOM, sec, f(11.0), dim);
                }
            }
            let ind = match *id {
                layout::CAPS => Some(locks.caps),
                layout::NUMLOCK => Some(locks.num),
                0 => Some(locks.fn_lock),
                12 => Some(locks.touchpad),
                _ => None,
            };
            if let Some(on) = ind {
                let c = pos2(rr.right() - 7.0, rr.top() + 7.0);
                if on {
                    painter.circle_filled(c, 2.8, Color32::WHITE);
                } else {
                    painter.circle_stroke(c, 2.5, Stroke::new(1.0_f32, Color32::from_rgb(70, 76, 88)));
                }
            }
        }
        if let (Some(a), Some(b)) = (self.drag_from, hover) {
            let area = Rect::from_two_pos(a, b);
            painter.rect_filled(area, CornerRadius::same(4), with_alpha(ACC, 25));
            painter.rect_stroke(area, CornerRadius::same(4), Stroke::new(1.0_f32, with_alpha(ACC, 160)), StrokeKind::Inside);
        }
    }

    fn rear(&mut self, ui: &mut egui::Ui) {
        let size = vec2(470.0, 268.0);
        let (outer, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(outer);
        painter.rect_filled(outer, CornerRadius::same(10), PANEL2);
        painter.rect_stroke(outer, CornerRadius::same(10), Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
        painter.text(outer.left_top() + vec2(14.0, 12.0), Align2::LEFT_TOP, "КОРПУС — ВИД СЗАДИ", FontId::proportional(11.0), DIM);
        let o = outer.left_top() + vec2(15.0, 36.0);
        let p = |x: f32, y: f32| o + vec2(x, y);

        // крышка
        let lid = Rect::from_min_max(p(62.0, 6.0), p(378.0, 146.0));
        painter.rect_filled(lid, CornerRadius::same(6), Color32::from_rgb(44, 48, 57));
        painter.rect_stroke(lid, CornerRadius::same(6), Stroke::new(1.0_f32, Color32::from_rgb(58, 64, 75)), StrokeKind::Inside);
        painter.rect_filled(Rect::from_min_max(p(80.0, 146.0), p(360.0, 154.0)), CornerRadius::ZERO, Color32::from_rgb(12, 14, 17));

        // эмблема
        let center = p(220.0, 46.0);
        let head = alien_head(center, 0.08);
        let ec = shade(self.emblem, 0.25 + 0.75 * self.ebri as f32 / 100.0);
        let hb = bbox(&head);
        painter.add(egui::epaint::Shadow { offset: [0, 0], blur: 22, spread: 2, color: with_alpha(ec, 90) }.as_shape(hb.shrink(4.0), CornerRadius::same(14)));
        painter.add(Shape::convex_polygon(head, rgb(ec), Stroke::NONE));
        for eye in alien_eyes(center, 0.08) {
            painter.add(Shape::convex_polygon(eye, Color32::from_rgb(44, 48, 57), Stroke::NONE));
        }
        let emb_sel = self.sel.contains(&Zone::Emblem);
        if emb_sel {
            painter.rect_stroke(hb.expand(6.0), CornerRadius::same(8), Stroke::new(2.0_f32, ACC), StrokeKind::Outside);
        }
        painter.line_segment([p(246.0, 46.0), p(296.0, 46.0)], Stroke::new(1.0_f32, Color32::from_rgb(74, 81, 96)));
        painter.text(p(300.0, 46.0), Align2::LEFT_CENTER, "Эмблема", FontId::proportional(12.0), DIM);

        // световой контур
        let bar = Rect::from_min_max(p(30.0, 155.0), p(410.0, 193.0));
        let cc = shade(self.contour, 0.25 + 0.75 * self.cbri as f32 / 100.0);
        painter.add(egui::epaint::Shadow { offset: [0, 0], blur: 16, spread: 1, color: with_alpha(cc, 110) }.as_shape(bar, CornerRadius::same(19)));
        painter.rect_filled(bar, CornerRadius::same(19), Color32::from_rgb(18, 20, 24));
        painter.rect_stroke(bar, CornerRadius::same(19), Stroke::new(3.0_f32, rgb(cc)), StrokeKind::Inside);
        let vents = bar.shrink2(vec2(10.0, 7.0));
        for (xs, xe) in [(vents.left() + 4.0, o.x + 150.0), (o.x + 290.0, vents.right() - 4.0)] {
            for (row, dy) in [-8.0f32, 0.0, 8.0].iter().enumerate() {
                let mut x = xs + if row % 2 == 1 { 4.3 } else { 0.0 };
                while x < xe {
                    let hx = hexagon(pos2(x, bar.center().y + dy), 3.8);
                    if hx.iter().all(|q| vents.expand(2.0).contains(*q)) {
                        painter.add(Shape::convex_polygon(hx, Color32::from_rgb(6, 7, 10), Stroke::new(0.8_f32, Color32::from_rgb(44, 49, 58))));
                    }
                    x += 8.6;
                }
            }
        }
        let ports = Rect::from_min_max(p(152.0, 163.0), p(286.0, 186.0));
        painter.rect_filled(ports, CornerRadius::same(5), Color32::from_rgb(29, 32, 39));
        painter.rect_stroke(ports, CornerRadius::same(5), Stroke::new(1.0_f32, Color32::from_rgb(47, 52, 61)), StrokeKind::Inside);
        for (x, w, h) in [(162.0, 10.0, 5.0), (178.0, 10.0, 5.0), (195.0, 13.0, 8.0), (214.0, 15.0, 8.0), (235.0, 7.0, 6.0), (248.0, 18.0, 4.0)] {
            let r = Rect::from_center_size(p(x + w / 2.0, 174.0), vec2(w, h));
            painter.rect_filled(r, CornerRadius::same(2), Color32::from_rgb(7, 8, 10));
        }
        painter.circle_filled(p(275.0, 174.0), 4.0, Color32::from_rgb(7, 8, 10));
        let con_sel = self.sel.contains(&Zone::Contour);
        if con_sel {
            painter.rect_stroke(bar.expand(5.0), CornerRadius::same(23), Stroke::new(2.0_f32, ACC), StrokeKind::Outside);
        }
        painter.text(p(410.0, 214.0), Align2::RIGHT_CENTER, "Световой контур", FontId::proportional(12.0), DIM);

        // нажатия
        let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.shift);
        let er = ui.interact(hb.expand(6.0), ui.id().with("emblem"), Sense::click());
        if er.clicked() {
            self.select(&[Zone::Emblem], ctrl);
        }
        let cr = ui.interact(bar.expand(4.0), ui.id().with("contour"), Sense::click());
        if cr.clicked() {
            self.select(&[Zone::Contour], ctrl);
        }
        if er.hovered() || cr.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    fn chips(&mut self, ui: &mut egui::Ui) {
        let all: Vec<Zone> = layout::all_ids().into_iter().map(Zone::Key).collect();
        let keys = |ids: &[u8]| ids.iter().map(|i| Zone::Key(*i)).collect::<Vec<_>>();
        let mut everything = all.clone();
        everything.push(Zone::Emblem);
        everything.push(Zone::Contour);
        let groups: Vec<(&str, Vec<Zone>)> = vec![
            ("Вся клавиатура", all),
            ("F1–F12", keys(layout::F_KEYS)),
            ("Цифры", keys(layout::NUMBERS)),
            ("QWER", keys(layout::QWER)),
            ("WASD", keys(layout::WASD)),
            ("Эмблема", vec![Zone::Emblem]),
            ("Световой контур", vec![Zone::Contour]),
            ("Всё сразу", everything),
        ];
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for (name, zones) in groups {
                let on = !self.sel.is_empty() && zones.len() == self.sel.len() && zones.iter().all(|z| self.sel.contains(z));
                let dot = zones.iter().find_map(|z| self.zone_color(*z)).unwrap_or([60, 60, 60]);
                if chip(ui, name, rgb(dot), on).clicked() {
                    self.select(&zones, false);
                }
            }
        });
    }

    fn status_line(&mut self, ui: &mut egui::Ui) {
        let s = self.status.lock().map(|s| s.clone()).unwrap_or_default();
        let dev = if !s.started {
            "Поиск устройств…".to_string()
        } else {
            format!(
                "Клавиатура: {} · Корпус: {}",
                if s.keyboard { "подключена" } else { "не найдена" },
                if s.body { "подключён" } else { "не найден" }
            )
        };
        let text = format!("{dev}  ·  {}", LAST_LOG.lock().map(|l| l.clone()).unwrap_or_default());
        let r = ui.add(egui::Label::new(egui::RichText::new(&text).size(12.0).color(DIM)).truncate().sense(Sense::click()));
        if r.clicked() {
            ui.ctx().copy_text(text.clone());
        }
        r.on_hover_text("Нажми, чтобы скопировать");
    }

    fn selection_line(&mut self, ui: &mut egui::Ui) {
        let sel = match self.sel.len() {
            0 => "ничего — нажми на клавишу или зону".to_string(),
            1..=6 => self.sel.iter().map(|z| match z {
                Zone::Key(id) => layout::label(*id),
                Zone::Emblem => "Эмблема".into(),
                Zone::Contour => "Световой контур".into(),
            }).collect::<Vec<_>>().join(", "),
            n => format!("зон: {n}"),
        };
        ui.label(egui::RichText::new(sel).size(14.0).color(TEXT));
    }

    // ---------------------------------------------------------------- правая часть

    fn right_panel(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            caption(ui, "Эффект");
            let before_eff = self.st.effect.clone();
            // две вкладки: обычные эффекты и реакция на нажатия; эффекты — плитками
            let t = ui.input(|i| i.time) as f32;
            let full = ui.available_width();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (v, n) in [(false, "Подсветка"), (true, "На нажатия")] {
                    let (r, resp) = ui.allocate_exact_size(vec2(full / 2.0, 30.0), Sense::click());
                    let on = self.eff_tab == v;
                    let cr = if v { CornerRadius { nw: 0, sw: 0, ne: 8, se: 8 } } else { CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 } };
                    ui.painter().rect_filled(r, cr, if on { with_alpha(ACC, 45) } else { Color32::from_rgb(28, 32, 40) });
                    ui.painter().rect_stroke(r, cr, Stroke::new(1.0_f32, if on { ACC } else { LINE }), StrokeKind::Inside);
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, n, FontId::proportional(13.0), if on { TEXT } else { DIM });
                    if resp.clicked() {
                        self.eff_tab = v;
                    }
                }
            });
            ui.add_space(2.0);
            let list: &[u8] = if self.eff_tab { &[100, 101, 102, 103, 104, 105, 106] } else { &[0, 7, 8, 16, 17] };
            let tw = (full - 8.0) / 2.0;
            for row in list.chunks(2) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for &k in row {
                        let (r, resp) = ui.allocate_exact_size(vec2(tw, 40.0), Sense::click());
                        let on = self.st.effect.kind == k;
                        let hov = resp.hovered();
                        ui.painter().rect_filled(r, CornerRadius::same(8), if on { with_alpha(ACC, 40) } else if hov { Color32::from_rgb(34, 39, 49) } else { Color32::from_rgb(26, 30, 38) });
                        ui.painter().rect_stroke(r, CornerRadius::same(8), Stroke::new(if on { 1.5_f32 } else { 1.0 }, if on { ACC } else { LINE }), StrokeKind::Inside);
                        ui.painter().text(r.left_top() + vec2(10.0, 7.0), Align2::LEFT_TOP, effect_name(k), FontId::proportional(12.5), if on { TEXT } else { DIM });
                        // маленькая живая полоска — как выглядит эффект
                        let strip = Rect::from_min_size(r.left_bottom() + vec2(10.0, -10.0), vec2(r.width() - 20.0, 3.0));
                        let n = 14;
                        let demo = state::Effect { kind: k, speed: 1, color: self.st.effect.color.clone(), dir: 2, bg: false };
                        let dc = state::from_hex(&demo.color).unwrap_or([0, 240, 240]);
                        for i in 0..n {
                            let x = i as f32 / (n - 1) as f32;
                            let c = match k {
                                0 => self.keys.iter().flatten().nth(i * 7).copied().unwrap_or([0, 240, 240]),
                                100..=106 => {
                                    // вспышка в середине полоски, повторяется раз в 2 секунды
                                    let age = (t % 2.0) as f32;
                                    let d = (x - 0.5).abs() * 8.0;
                                    let v = match k {
                                        100 => if d < 0.6 { 1.0 - age / 1.6 } else { 0.0 },
                                        104 | 105 => (1.0 - (d - age * 6.0).abs() * 0.8).max(0.0) * (1.0 - age / 1.6),
                                        106 => (1.0 - d * 0.25).max(0.0),
                                        _ => (1.0 - age / 1.6) - d * 0.3,
                                    }
                                    .clamp(0.0, 1.0);
                                    let col = match k {
                                        104 => hsv_to_rgb([(d * 0.12 + age).rem_euclid(1.0), 1.0, 1.0]),
                                        106 => hsv_to_rgb([0.66 * d / 4.0, 1.0, 1.0]),
                                        _ => dc,
                                    };
                                    shade(col, v.max(0.06))
                                }
                                _ => effect_preview(&demo, dc, x, 0.5, t),
                            };
                            let seg = Rect::from_min_size(strip.min + vec2(strip.width() * i as f32 / n as f32, 0.0), vec2(strip.width() / n as f32 - 1.5, 3.0));
                            ui.painter().rect_filled(seg, CornerRadius::same(1), rgb(c));
                        }
                        if resp.clicked() {
                            self.st.effect.kind = k;
                        }
                    }
                });
            }
            if self.st.effect.kind != 0 {
                if crate::reactive::is_reactive(self.st.effect.kind) {
                    ui.label(egui::RichText::new("Клавиши загораются, когда нажимаешь.").size(11.5).color(DIM));
                    self.autostart_note(ui, "Этот эффект рисует сама программа, а не клавиатура.");
                    ui.checkbox(&mut self.st.effect.bg, "Под эффектом — свои цвета");
                } else {
                    ui.label(egui::RichText::new("На всю клавиатуру. Свои цвета не пропадут.").size(11.5).color(DIM));
                }
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Скорость").color(DIM));
                    for (v, n) in [(0u8, "Медленно"), (1, "Средне"), (2, "Быстро")] {
                        ui.selectable_value(&mut self.st.effect.speed, v, n);
                    }
                });
                if self.st.effect.kind == 16 {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Направление").color(DIM));
                        for (v, n) in [(2u8, "→"), (1, "←"), (4, "↓"), (3, "↑")] {
                            ui.selectable_value(&mut self.st.effect.dir, v, egui::RichText::new(n).size(15.0));
                        }
                    });
                }
            }
            if self.st.effect != before_eff {
                if self.st.effect.kind != 0 {
                    self.sel.retain(|z| !matches!(z, Zone::Key(_)));
                    if let Some(c) = state::from_hex(&self.st.effect.color) {
                        if effect_has_color(self.st.effect.kind) && self.sel.is_empty() {
                            self.hsv = rgb_to_hsv(c);
                            self.hex = state::to_hex(c);
                        }
                    }
                }
                self.push_effect();
                self.st.save();
            }
            // у спектра и радужной волны своего цвета нет — палитру прячем, пока не выбрана эмблема или контур
            let show_color = !(self.st.effect.kind != 0 && !effect_has_color(self.st.effect.kind) && self.sel.is_empty());
            if show_color {
            ui.add_space(8.0);
            caption(ui, if effect_has_color(self.st.effect.kind) && self.sel.is_empty() { "Цвет эффекта" } else { "Цвет" });
            let before = self.hsv;
            sv_square(ui, &mut self.hsv);
            ui.add_space(4.0);
            hue_bar(ui, &mut self.hsv);
            if self.hsv != before {
                let c = hsv_to_rgb(self.hsv);
                self.set_color(c);
            }
            ui.add_space(4.0);
            let cur = hsv_to_rgb(self.hsv);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("HEX").color(DIM));
                let r = ui.add(egui::TextEdit::singleline(&mut self.hex).desired_width(72.0).char_limit(7));
                if r.changed() {
                    if let Some(c) = state::from_hex(&self.hex) {
                        self.hsv = rgb_to_hsv(c);
                        self.set_color(c);
                        self.hex = self.hex.trim_start_matches('#').to_uppercase();
                    }
                }
                let mut c = cur;
                let mut changed = false;
                for (i, n) in ["R", "G", "B"].iter().enumerate() {
                    ui.label(egui::RichText::new(*n).color(DIM));
                    changed |= ui.add(egui::DragValue::new(&mut c[i]).range(0..=255).speed(1.0)).changed();
                }
                if changed {
                    self.hsv = rgb_to_hsv(c);
                    self.set_color(c);
                }
            });
            ui.add_space(10.0);
            caption(ui, "Мои цвета");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(7.0, 7.0);
                let mut remove = None;
                for (i, h) in self.st.my_colors.clone().iter().enumerate() {
                    if let Some(c) = state::from_hex(h) {
                        let (r, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                        ui.painter().rect_filled(r, CornerRadius::same(7), rgb(c));
                        if resp.hovered() {
                            ui.painter().rect_stroke(r, CornerRadius::same(7), Stroke::new(2.0_f32, TEXT), StrokeKind::Inside);
                        }
                        if resp.clicked() {
                            self.hsv = rgb_to_hsv(c);
                            self.set_color(c);
                        }
                        if resp.secondary_clicked() {
                            remove = Some(i);
                        }
                        resp.on_hover_text("ПКМ — убрать");
                    }
                }
                let (r, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                ui.painter().rect_stroke(r, CornerRadius::same(7), Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
                ui.painter().text(r.center(), Align2::CENTER_CENTER, "+", FontId::proportional(17.0), DIM);
                if resp.on_hover_text("Добавить текущий цвет").clicked() {
                    self.st.my_colors.push(state::to_hex(cur));
                    self.st.save();
                }
                if let Some(i) = remove {
                    self.st.my_colors.remove(i);
                    self.st.save();
                }
            });
            ui.add_space(10.0);
            let cur_b = self.sel.iter().next().map(|z| self.zone_bri(*z));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Яркость выбранного").color(DIM));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(cur_b.map(|b| format!("{b}%")).unwrap_or("—".into())).color(TEXT));
                });
            });
            ui.spacing_mut().slider_width = ui.available_width();
            let mut b = cur_b.unwrap_or(100);
            let r = ui.add_enabled(cur_b.is_some(), egui::Slider::new(&mut b, 0..=100).show_value(false));
            if r.changed() {
                self.set_bri(b);
            }
            ui.add_space(10.0);
            } else {
                ui.add_space(8.0);
                ui.label(egui::RichText::new("У этого эффекта цвета меняются сами. Нажми на эмблему или контур, чтобы задать их цвет.").size(12.0).color(DIM));
            }
            ui.add_space(12.0);
            if self.naming {
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut self.scheme_name).hint_text("Название набора").desired_width(180.0));
                    r.request_focus();
                    if ui.button("Сохранить").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let name = if self.scheme_name.trim().is_empty() { format!("Набор {}", self.st.schemes.len() + 1) } else { self.scheme_name.trim().to_string() };
                        self.st.schemes.retain(|s| s.name != name);
                        self.st.schemes.push(Scheme {
                            name,
                            keys: self.keys_out().iter().enumerate().filter_map(|(i, c)| c.map(|c| (i as u8, state::to_hex(c)))).collect(),
                            emblem: state::to_hex(self.body_out().0),
                            contour: state::to_hex(self.body_out().1),
                        });
                        self.st.save();
                        self.naming = false;
                        self.scheme_name.clear();
                    }
                    if ui.button("✕").clicked() {
                        self.naming = false;
                    }
                });
            }
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 8.0) / 2.0;
                let mut load = None;
                egui::ComboBox::from_id_salt("schemes").width(w - 8.0).selected_text("Наборы").show_ui(ui, |ui| {
                    if self.st.schemes.is_empty() {
                        ui.label(egui::RichText::new("Пока нет сохранённых наборов").color(DIM));
                    }
                    for s in &self.st.schemes {
                        if ui.selectable_label(false, &s.name).clicked() {
                            load = Some(s.clone());
                        }
                    }
                });
                if let Some(s) = load {
                    let mut k = [None; KEYS];
                    for (id, h) in &s.keys {
                        if (*id as usize) < KEYS {
                            k[*id as usize] = state::from_hex(h);
                        }
                    }
                    self.keys = k;
                    self.kbri = [100; KEYS];
                    self.ebri = 100;
                    self.cbri = 100;
                    self.emblem = state::from_hex(&s.emblem).unwrap_or(self.emblem);
                    self.contour = state::from_hex(&s.contour).unwrap_or(self.contour);
                    self.kb_dirty = true;
                    self.body_dirty = true;
                    self.last_change = Some(Instant::now());
                    let sel: Vec<Zone> = self.sel.iter().copied().collect();
                    self.select(&sel, false);
                }
                let btn = egui::Button::new(egui::RichText::new("Сохранить набор").color(Color32::from_rgb(4, 19, 26)).strong()).fill(ACC).min_size(vec2(w, 34.0)).corner_radius(CornerRadius::same(8));
                if ui.add(btn).clicked() {
                    self.naming = true;
                }
            });
        });
    }
}

// ---------------------------------------------------------------- состояние Caps / Num / Fn Lock

struct Locks {
    caps: bool,
    num: bool,
    fn_lock: bool,
    touchpad: bool,
}

impl Locks {
    fn now(status: &Arc<Mutex<Status>>) -> Locks {
        let caps = crate::hook::CAPS.load(std::sync::atomic::Ordering::Relaxed);
        let num = crate::hook::NUM.load(std::sync::atomic::Ordering::Relaxed);
        let (fn_lock, touchpad) = status.lock().map(|s| (s.fn_lock, s.touchpad)).unwrap_or((false, true));
        Locks { caps, num, fn_lock, touchpad }
    }
}

// ---------------------------------------------------------------- элементы

fn setup_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    for (i, (name, file)) in [("segoe", "segoeui.ttf"), ("segoe_sym", "seguisym.ttf")].into_iter().enumerate() {
        if let Ok(bytes) = std::fs::read(std::path::Path::new(&windir).join("Fonts").join(file)) {
            fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
            fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(i, name.into());
        }
    }
    ctx.set_fonts(fonts);
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = PANEL2;
    v.selection.bg_fill = ACC.gamma_multiply(0.6);
    v.selection.stroke = Stroke::new(1.0_f32, ACC);
    v.widgets.inactive.bg_fill = PANEL2;
    v.widgets.inactive.weak_bg_fill = PANEL2;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
    v.widgets.inactive.corner_radius = CornerRadius::same(7);
    v.widgets.hovered.corner_radius = CornerRadius::same(7);
    v.widgets.active.corner_radius = CornerRadius::same(7);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(36, 41, 51);
    v.widgets.hovered.bg_fill = Color32::from_rgb(36, 41, 51);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.slider_trailing_fill = true;
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.0));
        s.spacing.button_padding = vec2(10.0, 6.0);
        s.spacing.interact_size.y = 28.0;
    });
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::default()
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, LINE))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            add(ui)
        });
}

/// Карточка по высоте содержимого (не растягивается на всё место).
fn card_fit(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::default()
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, LINE))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        });
}

fn caption(ui: &mut egui::Ui, t: &str) {
    ui.label(egui::RichText::new(t.to_uppercase()).size(11.5).color(DIM).extra_letter_spacing(1.0));
}

fn chip(ui: &mut egui::Ui, text: &str, dot: Color32, on: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(13.0), if on { TEXT } else { DIM });
    let size = vec2(galley.size().x + 38.0, 32.0);
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same(16), PANEL2);
    let stroke = if on { Stroke::new(1.5_f32, ACC) } else if resp.hovered() { Stroke::new(1.0_f32, Color32::from_rgb(80, 90, 105)) } else { Stroke::new(1.0_f32, LINE) };
    p.rect_stroke(r, CornerRadius::same(16), stroke, StrokeKind::Inside);
    p.circle_filled(pos2(r.left() + 16.0, r.center().y), 5.0, dot);
    p.galley(pos2(r.left() + 28.0, r.center().y - galley.size().y / 2.0), galley, TEXT);
    resp
}

fn sv_square(ui: &mut egui::Ui, hsv: &mut [f32; 3]) {
    let w = ui.available_width();
    let h = (ui.available_height() - 330.0).clamp(170.0, 300.0);
    let (r, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
    let n = 24;
    let mut mesh = Mesh::default();
    for j in 0..=n {
        for i in 0..=n {
            let s = i as f32 / n as f32;
            let v = 1.0 - j as f32 / n as f32;
            let c = hsv_to_rgb([hsv[0], s, v]);
            mesh.colored_vertex(pos2(r.left() + s * r.width(), r.top() + (1.0 - v) * r.height()), rgb(c));
        }
    }
    for j in 0..n {
        for i in 0..n {
            let a = (j * (n + 1) + i) as u32;
            mesh.add_triangle(a, a + 1, a + n as u32 + 1);
            mesh.add_triangle(a + 1, a + n as u32 + 2, a + n as u32 + 1);
        }
    }
    ui.painter().add(Shape::mesh(mesh));
    if let Some(p) = resp.interact_pointer_pos() {
        if resp.is_pointer_button_down_on() || resp.clicked() {
            hsv[1] = ((p.x - r.left()) / r.width()).clamp(0.0, 1.0);
            hsv[2] = 1.0 - ((p.y - r.top()) / r.height()).clamp(0.0, 1.0);
        }
    }
    let c = pos2(r.left() + hsv[1] * r.width(), r.top() + (1.0 - hsv[2]) * r.height());
    ui.painter().circle_stroke(c, 8.0, Stroke::new(2.5_f32, Color32::WHITE));
    ui.painter().circle_stroke(c, 9.5, Stroke::new(1.0_f32, Color32::from_black_alpha(120)));
}

fn hue_bar(ui: &mut egui::Ui, hsv: &mut [f32; 3]) {
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(vec2(w, 14.0), Sense::click_and_drag());
    let n = 36;
    let mut mesh = Mesh::default();
    for i in 0..=n {
        let h = i as f32 / n as f32;
        let c = rgb(hsv_to_rgb([h, 1.0, 1.0]));
        let x = r.left() + h * r.width();
        mesh.colored_vertex(pos2(x, r.top()), c);
        mesh.colored_vertex(pos2(x, r.bottom()), c);
    }
    for i in 0..n as u32 {
        let a = i * 2;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    ui.painter().add(Shape::mesh(mesh));
    if let Some(p) = resp.interact_pointer_pos() {
        if resp.is_pointer_button_down_on() || resp.clicked() {
            hsv[0] = ((p.x - r.left()) / r.width()).clamp(0.0, 0.999);
        }
    }
    let x = r.left() + hsv[0] * r.width();
    let k = Rect::from_center_size(pos2(x, r.center().y), vec2(8.0, 20.0));
    ui.painter().rect_filled(k, CornerRadius::same(4), Color32::WHITE);
    ui.painter().rect_stroke(k, CornerRadius::same(4), Stroke::new(1.0_f32, Color32::from_black_alpha(120)), StrokeKind::Outside);
}

// ---------------------------------------------------------------- рисунки

fn cubic(out: &mut Vec<Pos2>, p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2) {
    for i in 1..=12 {
        let t = i as f32 / 12.0;
        let u = 1.0 - t;
        let v = p0 * u * u * u + p1 * 3.0 * u * u * t + p2 * 3.0 * u * t * t + p3 * t * t * t;
        out.push(v.to_pos2());
    }
}

fn quad(out: &mut Vec<Pos2>, p0: Vec2, p1: Vec2, p2: Vec2) {
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        let u = 1.0 - t;
        let v = p0 * u * u + p1 * 2.0 * u * t + p2 * t * t;
        out.push(v.to_pos2());
    }
}

/// Голова инопланетянина (своя форма), точки в координатах 0..458.
fn head_raw() -> Vec<Pos2> {
    let s = 1.06;
    let t = |x: f32, y: f32| vec2(229.0 + (x - 229.0) * s, 230.0 + (y - 230.0) * s);
    let mut v = Vec::new();
    cubic(&mut v, t(229.0, 14.0), t(318.0, 14.0), t(400.0, 72.0), t(408.0, 178.0));
    cubic(&mut v, t(408.0, 178.0), t(412.0, 250.0), t(340.0, 340.0), t(270.0, 418.0));
    quad(&mut v, t(270.0, 418.0), t(229.0, 456.0), t(188.0, 418.0));
    cubic(&mut v, t(188.0, 418.0), t(118.0, 340.0), t(46.0, 250.0), t(50.0, 178.0));
    cubic(&mut v, t(50.0, 178.0), t(58.0, 72.0), t(140.0, 14.0), t(229.0, 14.0));
    v
}

fn eye_raw(mirror: bool) -> Vec<Pos2> {
    let m = |x: f32, y: f32| if mirror { vec2(458.0 - x, y) } else { vec2(x, y) };
    let mut v = Vec::new();
    cubic(&mut v, m(64.0, 200.0), m(66.0, 185.0), m(100.0, 183.0), m(140.0, 198.0));
    cubic(&mut v, m(140.0, 198.0), m(185.0, 215.0), m(210.0, 260.0), m(215.0, 322.0));
    cubic(&mut v, m(215.0, 322.0), m(216.0, 333.0), m(205.0, 334.0), m(190.0, 332.0));
    cubic(&mut v, m(190.0, 332.0), m(130.0, 322.0), m(70.0, 280.0), m(64.0, 230.0));
    cubic(&mut v, m(64.0, 230.0), m(63.0, 218.0), m(63.0, 208.0), m(64.0, 200.0));
    if mirror {
        v.reverse();
    }
    v
}

fn place(pts: Vec<Pos2>, center: Pos2, scale: f32) -> Vec<Pos2> {
    pts.into_iter().map(|p| center + (p.to_vec2() - vec2(229.0, 235.0)) * scale).collect()
}

fn alien_head(center: Pos2, scale: f32) -> Vec<Pos2> {
    place(head_raw(), center, scale)
}

fn alien_eyes(center: Pos2, scale: f32) -> [Vec<Pos2>; 2] {
    [place(eye_raw(false), center, scale), place(eye_raw(true), center, scale)]
}

fn bbox(pts: &[Pos2]) -> Rect {
    let mut r = Rect::NOTHING;
    for p in pts {
        r.extend_with(*p);
    }
    r
}

fn hexagon(c: Pos2, r: f32) -> Vec<Pos2> {
    (0..6).map(|i| {
        let a = (60.0 * i as f32 + 30.0).to_radians();
        pos2(c.x + r * a.cos(), c.y + r * a.sin())
    }).collect()
}

pub fn app_icon() -> egui::IconData {
    egui::IconData { rgba: include_bytes!("icon64.rgba").to_vec(), width: 64, height: 64 }
}

// ---------------------------------------------------------------- цвета

fn rgb(c: Rgb) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn with_alpha(c: impl Into<Color32Like>, a: u8) -> Color32 {
    let c = c.into().0;
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], a)
}

struct Color32Like([u8; 3]);
impl From<Rgb> for Color32Like {
    fn from(c: Rgb) -> Self {
        Color32Like(c)
    }
}
impl From<Color32> for Color32Like {
    fn from(c: Color32) -> Self {
        Color32Like([c.r(), c.g(), c.b()])
    }
}

fn shade(c: Rgb, k: f32) -> Rgb {
    [(c[0] as f32 * k) as u8, (c[1] as f32 * k) as u8, (c[2] as f32 * k) as u8]
}

/// Цвет надписи на клавише: тёмные цвета чуть подсвечиваем, чтобы читались.
fn lift(c: Rgb) -> Color32 {
    let m = c[0].max(c[1]).max(c[2]) as f32;
    if m < 1.0 {
        return OFF;
    }
    let k = (200.0 / m).max(1.0);
    rgb([(c[0] as f32 * k).min(255.0) as u8, (c[1] as f32 * k).min(255.0) as u8, (c[2] as f32 * k).min(255.0) as u8])
}

fn hsv_to_rgb(hsv: [f32; 3]) -> Rgb {
    let [h, s, v] = hsv;
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [(r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8]
}

fn rgb_to_hsv(c: Rgb) -> [f32; 3] {
    let (r, g, b) = (c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    [h, s, max]
}

fn effect_name(k: u8) -> &'static str {
    match k {
        100 => "Нажатая клавиша",
        101 => "Круг от нажатия",
        102 => "Крест",
        103 => "Лучи",
        104 => "Брызги радугой",
        105 => "Брызги цветом",
        106 => "Тепловая карта",
        7 => "Дыхание",
        8 => "Спектр",
        16 => "Радужная волна",
        17 => "Сканер",
        _ => "Свои цвета",
    }
}

fn effect_has_color(k: u8) -> bool {
    matches!(k, 7 | 17 | 100..=103 | 105)
}

/// Примерный вид эффекта в окне программы (на ноутбуке его рисует сама клавиатура).
fn effect_preview(e: &state::Effect, c: Rgb, x: f32, y: f32, t: f32) -> Rgb {
    let period = [3.0_f32, 2.0, 1.0][e.speed.min(2) as usize];
    let hue = |h: f32| hsv_to_rgb([h.rem_euclid(1.0), 1.0, 1.0]);
    match e.kind {
        7 => {
            let v = (t / period * std::f32::consts::PI).sin().powi(2);
            shade(c, 0.08 + 0.92 * v)
        }
        8 => hue(t / (period * 3.0)),
        16 => {
            let p = match e.dir { 1 => x, 3 => y, 4 => -y, _ => -x };
            hue(p + t / (period * 1.5))
        }
        17 => {
            let ph = (t / period).rem_euclid(2.0);
            let pos = if ph < 1.0 { ph } else { 2.0 - ph };
            let d = (x - pos).abs();
            shade(c, (1.0 - d * 7.0).clamp(0.05, 1.0))
        }
        _ => c,
    }
}
