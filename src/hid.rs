//! Работа с HID-устройствами Windows напрямую через hid.dll / setupapi.dll.
#![allow(non_snake_case, non_camel_case_types, clippy::upper_case_acronyms)]

use std::ffi::c_void;
use std::ptr::{null, null_mut};

type HANDLE = *mut c_void;
type BOOL = i32;
type BOOLEAN = u8;

#[repr(C)]
#[derive(Clone, Copy)]
struct GUID {
    d1: u32,
    d2: u16,
    d3: u16,
    d4: [u8; 8],
}

#[repr(C)]
struct SP_DEVICE_INTERFACE_DATA {
    cbSize: u32,
    InterfaceClassGuid: GUID,
    Flags: u32,
    Reserved: usize,
}

#[repr(C)]
struct HIDD_ATTRIBUTES {
    Size: u32,
    VendorID: u16,
    ProductID: u16,
    VersionNumber: u16,
}

#[repr(C)]
#[derive(Default)]
struct HIDP_CAPS {
    Usage: u16,
    UsagePage: u16,
    InputReportByteLength: u16,
    OutputReportByteLength: u16,
    FeatureReportByteLength: u16,
    Reserved: [u16; 17],
    NumberLinkCollectionNodes: u16,
    NumberInputButtonCaps: u16,
    NumberInputValueCaps: u16,
    NumberInputDataIndices: u16,
    NumberOutputButtonCaps: u16,
    NumberOutputValueCaps: u16,
    NumberOutputDataIndices: u16,
    NumberFeatureButtonCaps: u16,
    NumberFeatureValueCaps: u16,
    NumberFeatureDataIndices: u16,
}

#[link(name = "hid")]
extern "system" {
    fn HidD_GetHidGuid(guid: *mut GUID);
    fn HidD_GetAttributes(h: HANDLE, a: *mut HIDD_ATTRIBUTES) -> BOOLEAN;
    fn HidD_GetPreparsedData(h: HANDLE, pp: *mut isize) -> BOOLEAN;
    fn HidD_FreePreparsedData(pp: isize) -> BOOLEAN;
    fn HidP_GetCaps(pp: isize, caps: *mut HIDP_CAPS) -> i32;
    fn HidD_SetFeature(h: HANDLE, buf: *const c_void, len: u32) -> BOOLEAN;
    fn HidD_GetFeature(h: HANDLE, buf: *mut c_void, len: u32) -> BOOLEAN;
    fn HidD_SetOutputReport(h: HANDLE, buf: *const c_void, len: u32) -> BOOLEAN;
    fn HidD_GetInputReport(h: HANDLE, buf: *mut c_void, len: u32) -> BOOLEAN;
}

#[link(name = "setupapi")]
extern "system" {
    fn SetupDiGetClassDevsW(guid: *const GUID, enumerator: *const u16, hwnd: *mut c_void, flags: u32) -> isize;
    fn SetupDiEnumDeviceInterfaces(set: isize, info: *mut c_void, guid: *const GUID, idx: u32, data: *mut SP_DEVICE_INTERFACE_DATA) -> BOOL;
    fn SetupDiGetDeviceInterfaceDetailW(set: isize, data: *const SP_DEVICE_INTERFACE_DATA, detail: *mut c_void, size: u32, required: *mut u32, info: *mut c_void) -> BOOL;
    fn SetupDiDestroyDeviceInfoList(set: isize) -> BOOL;
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(name: *const u16, access: u32, share: u32, sec: *const c_void, disp: u32, flags: u32, tmpl: HANDLE) -> HANDLE;
    fn CloseHandle(h: HANDLE) -> BOOL;
    fn GetLastError() -> u32;
}

const DIGCF_PRESENT: u32 = 0x2;
const DIGCF_DEVICEINTERFACE: u32 = 0x10;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_SHARE_READ: u32 = 1;
const FILE_SHARE_WRITE: u32 = 2;
const OPEN_EXISTING: u32 = 3;
const INVALID: isize = -1;

/// Одна HID-коллекция устройства.
pub struct HidDevice {
    h: HANDLE,
    pub path: String,
    pub usage_page: u16,
    pub usage: u16,
    pub input_len: usize,
    pub output_len: usize,
    pub feature_len: usize,
}

unsafe impl Send for HidDevice {}

impl Drop for HidDevice {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.h);
        }
    }
}

impl HidDevice {
    /// Найти все коллекции с нужным VID/PID.
    pub fn find(vid: u16, pid: u16) -> Vec<HidDevice> {
        let mut out = Vec::new();
        unsafe {
            let mut guid = GUID { d1: 0, d2: 0, d3: 0, d4: [0; 8] };
            HidD_GetHidGuid(&mut guid);
            let set = SetupDiGetClassDevsW(&guid, null(), null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
            if set == INVALID {
                return out;
            }
            let mut idx = 0u32;
            loop {
                let mut data = SP_DEVICE_INTERFACE_DATA {
                    cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                    InterfaceClassGuid: guid,
                    Flags: 0,
                    Reserved: 0,
                };
                if SetupDiEnumDeviceInterfaces(set, null_mut(), &guid, idx, &mut data) == 0 {
                    break;
                }
                idx += 1;
                let mut req = 0u32;
                SetupDiGetDeviceInterfaceDetailW(set, &data, null_mut(), 0, &mut req, null_mut());
                if req < 8 {
                    continue;
                }
                let mut buf = vec![0u8; req as usize + 4];
                *(buf.as_mut_ptr() as *mut u32) = 8; // cbSize для x64
                if SetupDiGetDeviceInterfaceDetailW(set, &data, buf.as_mut_ptr() as *mut c_void, req, null_mut(), null_mut()) == 0 {
                    continue;
                }
                let wptr = buf.as_ptr().add(4) as *const u16;
                let mut len = 0;
                while *wptr.add(len) != 0 {
                    len += 1;
                }
                let wpath: Vec<u16> = std::slice::from_raw_parts(wptr, len + 1).to_vec();
                let path = String::from_utf16_lossy(&wpath[..len]);
                let low = path.to_lowercase();
                if !low.contains(&format!("vid_{:04x}", vid)) || !low.contains(&format!("pid_{:04x}", pid)) {
                    continue;
                }
                let h = CreateFileW(wpath.as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut());
                let h = if h as isize == INVALID {
                    // некоторые коллекции открываются только без прав доступа (для запросов)
                    let h2 = CreateFileW(wpath.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut());
                    if h2 as isize == INVALID {
                        crate::log::write(&format!("HID: не открылось {path} (ошибка {})", GetLastError()));
                        continue;
                    }
                    h2
                } else {
                    h
                };
                let mut attr = HIDD_ATTRIBUTES { Size: std::mem::size_of::<HIDD_ATTRIBUTES>() as u32, VendorID: 0, ProductID: 0, VersionNumber: 0 };
                HidD_GetAttributes(h, &mut attr);
                let mut caps = HIDP_CAPS::default();
                let mut pp = 0isize;
                if HidD_GetPreparsedData(h, &mut pp) != 0 {
                    HidP_GetCaps(pp, &mut caps);
                    HidD_FreePreparsedData(pp);
                }
                out.push(HidDevice {
                    h,
                    path,
                    usage_page: caps.UsagePage,
                    usage: caps.Usage,
                    input_len: caps.InputReportByteLength as usize,
                    output_len: caps.OutputReportByteLength as usize,
                    feature_len: caps.FeatureReportByteLength as usize,
                });
            }
            SetupDiDestroyDeviceInfoList(set);
        }
        out
    }

    pub fn set_feature(&self, buf: &[u8]) -> bool {
        unsafe { HidD_SetFeature(self.h, buf.as_ptr() as *const c_void, buf.len() as u32) != 0 }
    }
    pub fn get_feature(&self, buf: &mut [u8]) -> bool {
        unsafe { HidD_GetFeature(self.h, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) != 0 }
    }
    pub fn set_output(&self, buf: &[u8]) -> bool {
        unsafe { HidD_SetOutputReport(self.h, buf.as_ptr() as *const c_void, buf.len() as u32) != 0 }
    }
    pub fn get_input(&self, buf: &mut [u8]) -> bool {
        unsafe { HidD_GetInputReport(self.h, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) != 0 }
    }
    pub fn last_error() -> u32 {
        unsafe { GetLastError() }
    }
}
