//! Режимы питания и датчики ноутбука через WMI (класс AWCCWmiMethodFunction, как в AWCC).
//! Всё общение с WMI идёт в своём потоке: окно не подвисает.
use crate::log;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{BSTR, VARIANT};
use windows::Win32::System::Com::*;
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Wmi::*;

/// Коды режимов в ноутбуке: Батарея, Тихий, Баланс, Производительность, Максимум.
pub const MODES: [u32; 5] = [162, 163, 160, 161, 164];

#[derive(Default, Clone)]
pub struct Sensors {
    /// подключено ли (нужны права администратора)
    pub ok: bool,
    /// номер режима 0..4 (как в MODES), если узнали
    pub mode: Option<u8>,
    pub cpu_temp: Option<u32>,
    pub gpu_temp: Option<u32>,
    pub cpu_load: Option<f32>,
    pub gpu_load: Option<u32>,
    /// обороты вентиляторов и их максимум
    pub fans: Vec<(u32, u32)>,
    /// история температур (процессор, видеокарта) — раз в секунду, до 120 точек
    pub history: Vec<(f32, f32)>,
}

pub enum Cmd {
    SetMode(u8),
    /// смотрят ли сейчас на датчики (иначе опрашиваем редко)
    Watch(bool),
}

pub fn start() -> (Sender<Cmd>, Arc<Mutex<Sensors>>) {
    let (tx, rx) = channel();
    let s = Arc::new(Mutex::new(Sensors::default()));
    let s2 = s.clone();
    std::thread::spawn(move || run(rx, s2));
    (tx, s)
}

struct Wmi {
    svc: IWbemServices,
    path: BSTR,
}

impl Wmi {
    fn open() -> Option<Wmi> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let _ = CoInitializeSecurity(None, -1, None, None, RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE, None);
            let loc: IWbemLocator = match CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER) {
                Ok(l) => l,
                Err(e) => {
                    log::write(&format!("Питание: WMI не открылся ({e})"));
                    return None;
                }
            };
            let svc = match loc.ConnectServer(&BSTR::from("ROOT\\WMI"), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None) {
                Ok(s) => s,
                Err(e) => {
                    log::write(&format!("Питание: нет доступа к WMI ({e})"));
                    return None;
                }
            };
            // права на вызовы: без этого WMI отвечает пусто (безопасность COM уже выставило окно)
            let _ = CoSetProxyBlanket(&svc, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, windows::core::PCWSTR::null(), RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE);
            let q = match svc.ExecQuery(&BSTR::from("WQL"), &BSTR::from("SELECT * FROM AWCCWmiMethodFunction"), WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY, None) {
                Ok(q) => q,
                Err(e) => {
                    log::write(&format!("Питание: класс AWCC не найден ({e}) — нужны права администратора"));
                    return None;
                }
            };
            let mut objs = [None];
            let mut n = 0u32;
            let hr = q.Next(WBEM_INFINITE, &mut objs, &mut n);
            log::write(&format!("Питание: поиск устройства — {hr:?}, найдено {n}"));
            let Some(obj) = objs[0].take() else {
                log::write("Питание: устройство AWCC не найдено (нужны права администратора?)");
                return None;
            };
            let mut v = VARIANT::default();
            if obj.Get(windows::core::w!("__PATH"), 0, &mut v, None, None).is_err() {
                return None;
            }
            let path = BSTR::try_from(&v).unwrap_or_default();
            log::write(&format!("Питание: подключено ({path})"));
            Some(Wmi { svc, path })
        }
    }

    /// Вызвать метод с одним числом и получить ответ.
    fn call(&self, method: &str, arg: u32) -> Option<u32> {
        unsafe {
            let mut class = None;
            self.svc.GetObject(&BSTR::from("AWCCWmiMethodFunction"), WBEM_FLAG_RETURN_WBEM_COMPLETE, None, Some(&mut class), None).ok()?;
            let class: IWbemClassObject = class?;
            let m: Vec<u16> = method.encode_utf16().chain(Some(0)).collect();
            let mut insig = None;
            class.GetMethod(windows::core::PCWSTR(m.as_ptr()), 0, &mut insig, std::ptr::null_mut()).ok()?;
            let inp = insig?.SpawnInstance(0).ok()?;
            let val = VARIANT::from(arg as i32);
            inp.Put(windows::core::w!("arg2"), 0, &val, 0).ok()?;
            let mut out = None;
            self.svc.ExecMethod(&self.path, &BSTR::from(method), WBEM_GENERIC_FLAG_TYPE(0), None, &inp, Some(&mut out), None).ok()?;
            let out: IWbemClassObject = out?;
            let mut v = VARIANT::default();
            out.Get(windows::core::w!("argr"), 0, &mut v, None, None).ok()?;
            i32::try_from(&v).map(|x| x as u32).ok().or_else(|| u32::try_from(&v).ok())
        }
    }

    fn info(&self, cmd: u32, arg: u32) -> Option<u32> {
        self.call("Thermal_Information", cmd | (arg << 8)).filter(|v| *v != u32::MAX && *v != u32::MAX - 1)
    }
}

// ---- загрузка процессора (по времени простоя системы)
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct FileTime(u32, u32);
#[link(name = "kernel32")]
extern "system" {
    fn GetSystemTimes(idle: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
    fn LoadLibraryW(name: *const u16) -> isize;
    fn GetProcAddress(m: isize, name: *const u8) -> isize;
    fn FreeLibrary(m: isize) -> i32;
}
fn ft(t: FileTime) -> u64 {
    ((t.1 as u64) << 32) | t.0 as u64
}

/// Загрузка видеокарты NVIDIA через nvml.dll (идёт вместе с драйвером).
struct Nvml {
    m: isize,
    shutdown: isize,
    dev: *mut std::ffi::c_void,
    util: extern "C" fn(*mut std::ffi::c_void, *mut [u32; 2]) -> i32,
}
impl Nvml {
    fn open() -> Option<Nvml> {
        unsafe {
            let name: Vec<u16> = "nvml.dll".encode_utf16().chain(Some(0)).collect();
            let m = LoadLibraryW(name.as_ptr());
            if m == 0 {
                return None;
            }
            let init = GetProcAddress(m, b"nvmlInit_v2\0".as_ptr());
            let handle = GetProcAddress(m, b"nvmlDeviceGetHandleByIndex_v2\0".as_ptr());
            let util = GetProcAddress(m, b"nvmlDeviceGetUtilizationRates\0".as_ptr());
            if init == 0 || handle == 0 || util == 0 {
                return None;
            }
            let init: extern "C" fn() -> i32 = std::mem::transmute(init);
            let handle: extern "C" fn(u32, *mut *mut std::ffi::c_void) -> i32 = std::mem::transmute(handle);
            if init() != 0 {
                return None;
            }
            let mut dev = std::ptr::null_mut();
            if handle(0, &mut dev) != 0 {
                return None;
            }
            let shutdown = GetProcAddress(m, b"nvmlShutdown\0".as_ptr());
            Some(Nvml { m, shutdown, dev, util: std::mem::transmute(util) })
        }
    }
    fn load(&self) -> Option<u32> {
        let mut u = [0u32; 2];
        if (self.util)(self.dev, &mut u) == 0 { Some(u[0]) } else { None }
    }
}
/// Окно закрыли — выгружаем nvml, чтобы фон не держал её в памяти.
impl Drop for Nvml {
    fn drop(&mut self) {
        unsafe {
            if self.shutdown != 0 {
                let f: extern "C" fn() -> i32 = std::mem::transmute(self.shutdown);
                f();
            }
            FreeLibrary(self.m);
        }
    }
}

fn run(rx: Receiver<Cmd>, s: Arc<Mutex<Sensors>>) {
    let w = Wmi::open();
    let Some(w) = w else { return };
    // какие датчики и вентиляторы есть у ноутбука
    let mut fans = Vec::new();
    let mut temps = Vec::new();
    for i in 0..48 {
        let Some(id) = w.info(0x03, i) else { break };
        match id {
            0x30..=0x3F => fans.push(id),
            0x100..=0x1FF => temps.push(id & 0xFF),
            _ => {}
        }
    }
    let fan_max: Vec<u32> = fans.iter().map(|f| w.info(0x09, *f).unwrap_or(5000)).collect();
    log::write(&format!("Питание: вентиляторы {:X?}, датчики {:X?}", fans, temps));
    // nvml открываем только пока открыто окно (график)
    let mut nv: Option<Nvml> = None;
    let mut watch = false;
    let mut last = Instant::now() - Duration::from_secs(10);
    let mut prev_cpu: Option<(u64, u64)> = None;
    if let Ok(mut st) = s.lock() {
        st.ok = true;
    }
    loop {
        let wait = if watch { Duration::from_millis(1000) } else { Duration::from_millis(5000) };
        match rx.recv_timeout(wait.saturating_sub(last.elapsed())) {
            Ok(Cmd::SetMode(i)) => {
                let code = MODES[(i as usize).min(4)];
                let r = w.call("Thermal_Control", (code << 8) | 1);
                log::write(&format!("Питание: режим {code} — {}", if r == Some(0) { "ок" } else { "ошибка" }));
                last = Instant::now() - Duration::from_secs(10);
                continue;
            }
            Ok(Cmd::Watch(v)) => {
                watch = v;
                if v && nv.is_none() {
                    nv = Nvml::open();
                } else if !v {
                    nv = None;
                    crate::service::trim();
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(_) => {}
        }
        last = Instant::now();
        let mode = w.info(0x0B, 0).and_then(|c| MODES.iter().position(|m| *m == c)).map(|p| p as u8);
        let mut out = Sensors { ok: true, mode, ..Default::default() };
        if watch {
            out.cpu_temp = temps.first().and_then(|t| w.info(0x04, *t));
            out.gpu_temp = temps.get(1).and_then(|t| w.info(0x04, *t));
            out.fans = fans.iter().zip(&fan_max).map(|(f, m)| (w.info(0x05, *f).unwrap_or(0), *m)).collect();
            out.gpu_load = nv.as_ref().and_then(|n| n.load());
            let (mut i, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default());
            if unsafe { GetSystemTimes(&mut i, &mut k, &mut u) } != 0 {
                let idle = ft(i);
                let total = ft(k) + ft(u);
                if let Some((pi, pt)) = prev_cpu {
                    let dt = total.saturating_sub(pt).max(1);
                    out.cpu_load = Some(100.0 * (1.0 - idle.saturating_sub(pi) as f32 / dt as f32));
                }
                prev_cpu = Some((idle, total));
            }
        }
        if let Ok(mut st) = s.lock() {
            let mut hist = std::mem::take(&mut st.history);
            if let (Some(c), Some(g)) = (out.cpu_temp, out.gpu_temp) {
                hist.push((c as f32, g as f32));
                if hist.len() > 120 {
                    hist.remove(0);
                }
            }
            out.history = hist;
            *st = out;
        }
    }
}
