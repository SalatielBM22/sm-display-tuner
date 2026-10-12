//! Camada Windows: gamma ramp da GPU, alvo de exibição, associação de perfis e elevação.
//! Funções de gdi32/user32/mscms são carregadas dinamicamente (sem import libs extras).
#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub type Ramp = [[u16; 256]; 3];

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
    fn LocalFree(p: *mut c_void) -> *mut c_void;
}

pub fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

unsafe fn proc_addr(dll: &str, name: &str) -> usize {
    let m = LoadLibraryW(wide(dll).as_ptr());
    if m.is_null() {
        return 0;
    }
    let mut n = name.as_bytes().to_vec();
    n.push(0);
    GetProcAddress(m, n.as_ptr()) as usize
}

macro_rules! dynfn {
    ($fname:ident, $dll:expr, $sym:expr, ($($arg:ident : $t:ty),*) -> $ret:ty) => {
        #[allow(clippy::too_many_arguments)]
        unsafe fn $fname($($arg: $t),*) -> Option<$ret> {
            static P: OnceLock<usize> = OnceLock::new();
            let p = *P.get_or_init(|| proc_addr($dll, $sym));
            if p == 0 {
                return None;
            }
            let f: unsafe extern "system" fn($($t),*) -> $ret = std::mem::transmute(p);
            Some(f($($arg),*))
        }
    };
}

dynfn!(GetDC, "user32.dll", "GetDC", (hwnd: *mut c_void) -> *mut c_void);
dynfn!(ReleaseDC, "user32.dll", "ReleaseDC", (hwnd: *mut c_void, hdc: *mut c_void) -> i32);
dynfn!(GetDeviceGammaRamp, "gdi32.dll", "GetDeviceGammaRamp", (hdc: *mut c_void, ramp: *mut c_void) -> i32);
dynfn!(SetDeviceGammaRamp, "gdi32.dll", "SetDeviceGammaRamp", (hdc: *mut c_void, ramp: *const c_void) -> i32);
dynfn!(GetDisplayConfigBufferSizes, "user32.dll", "GetDisplayConfigBufferSizes", (flags: u32, np: *mut u32, nm: *mut u32) -> i32);
dynfn!(QueryDisplayConfig, "user32.dll", "QueryDisplayConfig",
    (flags: u32, np: *mut u32, p: *mut PathInfo, nm: *mut u32, m: *mut ModeInfo, topo: *mut c_void) -> i32);
dynfn!(ColorProfileAddDisplayAssociation, "mscms.dll", "ColorProfileAddDisplayAssociation",
    (scope: i32, name: *const u16, luid: Luid, src: u32, set_default: i32, adv: i32) -> i32);
dynfn!(ColorProfileSetDisplayDefaultAssociation, "mscms.dll", "ColorProfileSetDisplayDefaultAssociation",
    (scope: i32, name: *const u16, ptype: i32, psub: i32, luid: Luid, src: u32) -> i32);
dynfn!(ColorProfileGetDisplayDefault, "mscms.dll", "ColorProfileGetDisplayDefault",
    (scope: i32, luid: Luid, src: u32, ptype: i32, psub: i32, out: *mut *mut u16) -> i32);

// ---------------------------------------------------------------------------
// Gamma ramp
// ---------------------------------------------------------------------------
struct ScreenDc(*mut c_void);
impl ScreenDc {
    fn get() -> Result<Self, String> {
        let dc = unsafe { GetDC(std::ptr::null_mut()) }.ok_or("user32!GetDC indisponível")?;
        if dc.is_null() {
            return Err("GetDC(NULL) falhou".into());
        }
        Ok(Self(dc))
    }
}
impl Drop for ScreenDc {
    fn drop(&mut self) {
        unsafe {
            ReleaseDC(std::ptr::null_mut(), self.0);
        }
    }
}

pub fn read_ramp() -> Result<Ramp, String> {
    let dc = ScreenDc::get()?;
    let mut r: Ramp = [[0; 256]; 3];
    let ok = unsafe { GetDeviceGammaRamp(dc.0, r.as_mut_ptr() as *mut c_void) }.ok_or("gdi32 indisponível")?;
    if ok == 0 {
        return Err("GetDeviceGammaRamp falhou".into());
    }
    Ok(r)
}

pub fn write_ramp(r: &Ramp) -> Result<(), String> {
    let dc = ScreenDc::get()?;
    let ok = unsafe { SetDeviceGammaRamp(dc.0, r.as_ptr() as *const c_void) }.ok_or("gdi32 indisponível")?;
    if ok == 0 {
        return Err("Windows recusou a curva (fora da faixa permitida, ou ACM/HDR ligado)".into());
    }
    Ok(())
}

pub fn identity_ramp() -> Ramp {
    let mut r: Ramp = [[0; 256]; 3];
    for ch in r.iter_mut() {
        for (i, v) in ch.iter_mut().enumerate() {
            *v = (i as u16) * 257;
        }
    }
    r
}

// ---------------------------------------------------------------------------
// Alvo de exibição (adapter LUID + source id) para a API de associação
// ---------------------------------------------------------------------------
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Luid {
    pub low: u32,
    pub high: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SrcInfo {
    adapter: Luid,
    id: u32,
    mode_idx: u32,
    status: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TgtInfo {
    adapter: Luid,
    id: u32,
    mode_idx: u32,
    tech: u32,
    rot: u32,
    scal: u32,
    refresh_n: u32,
    refresh_d: u32,
    scan: u32,
    avail: i32,
    status: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PathInfo {
    src: SrcInfo,
    tgt: TgtInfo,
    flags: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ModeInfo {
    _b: [u8; 64],
}

#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub adapter: Luid,
    pub source: u32,
}

pub fn primary_target() -> Result<Target, String> {
    const QDC_ONLY_ACTIVE_PATHS: u32 = 2;
    let (mut np, mut nm) = (0u32, 0u32);
    let hr = unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) }.ok_or("user32 indisponível")?;
    if hr != 0 || np == 0 {
        return Err(format!("GetDisplayConfigBufferSizes = {hr}"));
    }
    let mut paths = vec![PathInfo::default(); np as usize];
    let mut modes = vec![ModeInfo { _b: [0; 64] }; nm as usize];
    let hr = unsafe {
        QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), std::ptr::null_mut())
    }
    .ok_or("user32 indisponível")?;
    if hr != 0 || np == 0 {
        return Err(format!("QueryDisplayConfig = {hr}"));
    }
    Ok(Target { adapter: paths[0].src.adapter, source: paths[0].src.id })
}

const USER_SCOPE: i32 = 1;
const CPT_ICC: i32 = 0;
const CPST_STANDARD: i32 = 7;

pub fn default_profile(t: Target) -> Option<String> {
    let mut out: *mut u16 = std::ptr::null_mut();
    let hr = unsafe { ColorProfileGetDisplayDefault(USER_SCOPE, t.adapter, t.source, CPT_ICC, CPST_STANDARD, &mut out) }?;
    if hr < 0 || out.is_null() {
        return None;
    }
    let mut n = 0usize;
    let s = unsafe {
        while *out.add(n) != 0 {
            n += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(out, n))
    };
    unsafe {
        LocalFree(out as *mut c_void);
    }
    Some(s)
}

pub fn set_default_profile(t: Target, file_name: &str) -> Result<(), String> {
    let w = wide(file_name);
    let hr = unsafe { ColorProfileAddDisplayAssociation(USER_SCOPE, w.as_ptr(), t.adapter, t.source, 1, 0) }.ok_or("mscms indisponível")?;
    if hr < 0 {
        return Err(format!("ColorProfileAddDisplayAssociation 0x{hr:08X}"));
    }
    let hr = unsafe { ColorProfileSetDisplayDefaultAssociation(USER_SCOPE, w.as_ptr(), CPT_ICC, CPST_STANDARD, t.adapter, t.source) }
        .ok_or("mscms indisponível")?;
    if hr < 0 {
        return Err(format!("ColorProfileSetDisplayDefaultAssociation 0x{hr:08X}"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Sistema
// ---------------------------------------------------------------------------
pub fn color_dir() -> PathBuf {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    Path::new(&windir).join(r"System32\spool\drivers\color")
}

pub fn reload_calibration() -> bool {
    Command::new("schtasks.exe")
        .args(["/run", "/tn", r"\Microsoft\Windows\WindowsColorSystem\Calibration Loader"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

const ICM_KEY: &str = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ICM";

pub fn gamma_range_unlocked() -> bool {
    Command::new("reg.exe")
        .args(["query", ICM_KEY, "/v", "GdiIcmGammaRange"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("0x100"))
        .unwrap_or(false)
}

/// Executado pelo processo elevado.
pub fn unlock_gamma_range() -> bool {
    Command::new("reg.exe")
        .args(["add", ICM_KEY, "/v", "GdiIcmGammaRange", "/t", "REG_DWORD", "/d", "256", "/f"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Relança este executável elevado (UAC) com `args` e espera terminar.
pub fn run_elevated(args: &[&str]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let quoted: Vec<String> = args.iter().map(|a| format!("\"{}\"", a.replace('\'', "''"))).collect();
    let ps = format!(
        "Start-Process -FilePath '{}' -ArgumentList '{}' -Verb RunAs -Wait",
        exe.display().to_string().replace('\'', "''"),
        quoted.join(" ")
    );
    let st = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    if st.success() {
        Ok(())
    } else {
        Err("elevação cancelada ou falhou".into())
    }
}
