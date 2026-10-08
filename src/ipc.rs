//! Связь окна с фоновой частью программы.
//! Окно → фон: короткие сообщения окну трея (WM_COPYDATA).
//! Фон → окно: общий блок памяти, куда фон несколько раз в секунду пишет своё состояние.
use crate::keyboard::{Rgb, KEYS};
use serde::{Deserialize, Serialize};

pub const TRAY_CLASS: &str = "OAWCC_Tray";
const SNAP_NAME: &str = "Local\\OAWCC_Snapshot";
const SNAP_SIZE: usize = 256 * 1024;
const MAGIC: usize = 0x0A1E_C0DE;

pub type Keys = Vec<Option<Rgb>>;

pub fn pack(k: &[Option<Rgb>; KEYS]) -> Keys {
    k.to_vec()
}
pub fn unpack(k: &Keys) -> Box<[Option<Rgb>; KEYS]> {
    let mut a = Box::new([None; KEYS]);
    for (i, c) in k.iter().take(KEYS).enumerate() {
        a[i] = *c;
    }
    a
}

/// Команды окна фоновой части.
#[derive(Serialize, Deserialize)]
pub enum Msg {
    Keys(Keys, u8),
    Body(Rgb, Rgb, u8),
    Save(Option<Keys>, Rgb, Rgb, u8),
    Effect(u8, u8, Rgb, u8),
    Reactive(u8, u8, Rgb, Keys),
    Numpad(Keys),
    SetMode(u8),
    Watch(bool),
    Swapped(Vec<u8>),
    Tray(bool),
    Press(u8),
    Debug(bool),
    Lang(u8),
    /// окно закрылось, а фон не нужен (нет трея и автозапуска) — завершиться
    Quit,
}

/// Состояние фона для окна.
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Snapshot {
    pub started: bool,
    pub keyboard: bool,
    pub body: bool,
    pub fn_lock: bool,
    pub touchpad: bool,
    pub num: bool,
    pub caps: bool,
    pub last_log: String,
    pub power_ok: bool,
    pub mode: Option<u8>,
    pub cpu_temp: Option<u32>,
    pub gpu_temp: Option<u32>,
    pub cpu_load: Option<f32>,
    pub gpu_load: Option<u32>,
    pub fans: Vec<(u32, u32)>,
    pub history: Vec<(f32, f32)>,
    /// нажатия для предпросмотра «реакции»: (клавиша, сколько мс назад)
    pub hits: Vec<(u8, u32)>,
    pub heat: Vec<(u8, f32)>,
}

#[link(name = "user32")]
extern "system" {
    fn FindWindowW(c: *const u16, n: *const u16) -> isize;
    fn SendMessageTimeoutW(h: isize, m: u32, w: usize, l: isize, flags: u32, timeout: u32, res: *mut usize) -> isize;
}
#[link(name = "kernel32")]
extern "system" {
    fn CreateFileMappingW(file: isize, sec: *const std::ffi::c_void, prot: u32, hi: u32, lo: u32, name: *const u16) -> isize;
    fn OpenFileMappingW(access: u32, inherit: i32, name: *const u16) -> isize;
    fn MapViewOfFile(h: isize, access: u32, hi: u32, lo: u32, size: usize) -> *mut u8;
}

#[repr(C)]
pub struct CopyData {
    pub data: usize,
    pub len: u32,
    pub ptr: *const u8,
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Окно фоновой части (если фон запущен).
pub fn service_window() -> isize {
    let c = w(TRAY_CLASS);
    unsafe { FindWindowW(c.as_ptr(), std::ptr::null()) }
}

/// Отправить команду фону.
pub fn send(m: &Msg) -> bool {
    let h = service_window();
    if h == 0 {
        return false;
    }
    let Ok(body) = serde_json::to_vec(m) else { return false };
    let cd = CopyData { data: MAGIC, len: body.len() as u32, ptr: body.as_ptr() };
    let mut res = 0usize;
    // WM_COPYDATA, ждём не дольше секунды
    unsafe { SendMessageTimeoutW(h, 0x004A, 0, &cd as *const CopyData as isize, 0x0002, 1000, &mut res) != 0 }
}

/// Разобрать пришедшую команду (в фоновой части).
pub fn parse(lp: isize) -> Option<Msg> {
    let cd = unsafe { &*(lp as *const CopyData) };
    if cd.data != MAGIC || cd.ptr.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(cd.ptr, cd.len as usize) };
    serde_json::from_slice(bytes).ok()
}

struct View(*mut u8);
unsafe impl Send for View {}
unsafe impl Sync for View {}

/// Общий блок памяти: создаёт фон, открывает окно.
fn view(create: bool) -> Option<&'static View> {
    static V: std::sync::OnceLock<Option<View>> = std::sync::OnceLock::new();
    let v = V.get_or_init(|| unsafe {
        let n = w(SNAP_NAME);
        let h = if create { CreateFileMappingW(-1, std::ptr::null(), 0x04, 0, SNAP_SIZE as u32, n.as_ptr()) } else { OpenFileMappingW(0x0004 | 0x0002, 0, n.as_ptr()) };
        if h == 0 {
            return None;
        }
        let p = MapViewOfFile(h, 0x0004 | 0x0002, 0, 0, SNAP_SIZE);
        if p.is_null() {
            None
        } else {
            Some(View(p))
        }
    });
    v.as_ref()
}

/// Записать состояние (фоновая часть).
pub fn publish(s: &Snapshot) {
    let Some(v) = view(true) else { return };
    let Ok(body) = serde_json::to_vec(s) else { return };
    if body.len() + 8 > SNAP_SIZE {
        return;
    }
    unsafe {
        // сначала обнуляем длину, потом пишем данные, потом длину — окно не прочитает половину
        std::ptr::write_volatile(v.0 as *mut u32, 0);
        std::ptr::copy_nonoverlapping(body.as_ptr(), v.0.add(8), body.len());
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        std::ptr::write_volatile(v.0 as *mut u32, body.len() as u32);
    }
}

/// Прочитать состояние (окно).
pub fn read() -> Option<Snapshot> {
    let v = view(false)?;
    unsafe {
        let len = std::ptr::read_volatile(v.0 as *const u32) as usize;
        if len == 0 || len + 8 > SNAP_SIZE {
            return None;
        }
        let mut buf = vec![0u8; len];
        std::ptr::copy_nonoverlapping(v.0.add(8), buf.as_mut_ptr(), len);
        if std::ptr::read_volatile(v.0 as *const u32) as usize != len {
            return None;
        }
        serde_json::from_slice(&buf).ok()
    }
}

// ------------------------------------------------------------------ для окна: те же команды, что раньше
/// Подсветка — команды уходят в фоновую часть.
pub struct Kb;
impl Kb {
    pub fn send(&self, c: crate::worker::Cmd) -> Result<(), ()> {
        use crate::worker::Cmd;
        let m = match c {
            Cmd::Keys(k, b) => Msg::Keys(pack(&k), b),
            Cmd::Body(e, c, b) => Msg::Body(e, c, b),
            Cmd::Save(k, e, c, b) => Msg::Save(k.map(|k| pack(&k)), e, c, b),
            Cmd::Effect(k, s, c, d) => Msg::Effect(k, s, c, d),
            Cmd::Reactive(k, s, c, bg) => Msg::Reactive(k, s, c, pack(&bg)),
            Cmd::Numpad(p) => Msg::Numpad(pack(&p)),
        };
        if send(&m) { Ok(()) } else { Err(()) }
    }
}

/// Режимы питания и датчики — команды уходят в фоновую часть.
pub struct Pw;
impl Pw {
    pub fn send(&self, c: crate::power::Cmd) -> Result<(), ()> {
        let m = match c {
            crate::power::Cmd::SetMode(i) => Msg::SetMode(i),
            crate::power::Cmd::Watch(v) => Msg::Watch(v),
        };
        if send(&m) { Ok(()) } else { Err(()) }
    }
}
