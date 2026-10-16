//! SkateCraft host plugin for the WoW 3.3.5a (build 12340) client.
//!
//! Delivered as a `dinput8.dll` proxy: Wow.exe imports DirectInput8Create from DINPUT8.dll, so a
//! DLL of that name next to the exe is loaded at start-up. It forwards that call to
//! the real system DLL, and starts a worker thread that talks to the Skate guest through the
//! shared region defined in `skatecraft-protocol`.

#![allow(non_snake_case, clippy::missing_safety_doc, unsafe_op_in_unsafe_fn)]

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use skatecraft_protocol::*;
use std::io::Write;
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HINSTANCE, TRUE};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetProcAddress, LoadLibraryW,
};
use windows_sys::Win32::System::SystemInformation::{GetSystemDirectoryW, GetTickCount64};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::{CreateThread, GetCurrentProcessId, Sleep};

// ---------------------------------------------------------------- real dinput8.dll
// Wow.exe imports a single function from DINPUT8.dll, so a dinput8.dll next to the exe is
// loaded at start-up (version.dll does not work: Windows 11 maps the system copy earlier).

static REAL: AtomicIsize = AtomicIsize::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe fn real_fn(name: &[u8]) -> *const c_void {
    let mut h = REAL.load(Ordering::Acquire);
    if h == 0 {
        let mut buf = [0u16; 260];
        let n = GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) as usize;
        let mut path = String::from_utf16_lossy(&buf[..n]);
        path.push_str("\\dinput8.dll");
        h = LoadLibraryW(wide(&path).as_ptr()) as isize;
        REAL.store(h, Ordering::Release);
    }
    match GetProcAddress(h as _, name.as_ptr()) {
        Some(f) => f as *const c_void,
        None => ptr::null(),
    }
}

type FnCreate = unsafe extern "system" fn(HINSTANCE, u32, *const c_void, *mut *mut c_void, *mut c_void) -> i32;

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DirectInput8Create(
    hinst: HINSTANCE,
    version: u32,
    riid: *const c_void,
    out: *mut *mut c_void,
    outer: *mut c_void,
) -> i32 {
    let p = real_fn(b"DirectInput8Create\0");
    if p.is_null() {
        return 0x8007007Eu32 as i32; // HRESULT_FROM_WIN32(ERROR_MOD_NOT_FOUND)
    }
    core::mem::transmute::<_, FnCreate>(p)(hinst, version, riid, out, outer)
}

// ---------------------------------------------------------------- plugin

static LOG_PATH: AtomicIsize = AtomicIsize::new(0);

fn log(msg: &str) {
    let p = LOG_PATH.load(Ordering::Relaxed);
    if p == 0 { return; }
    let path = unsafe { &*(p as *const String) };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let t = unsafe { GetTickCount64() };
        let _ = writeln!(f, "[{t}] {msg}");
    }
}

fn exe_path() -> String {
    let mut buf = [0u16; 520];
    let n = unsafe { GetModuleFileNameW(ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

static FRAME: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn worker(_: *mut c_void) -> u32 {
    log(&format!("host plugin started in {}", exe_path()));
    let shared = match Shared::create() {
        Ok(s) => s,
        Err(e) => {
            log(&format!("shared region failed: error {e}"));
            return 1;
        }
    };
    log("shared region ready");
    let pid = GetCurrentProcessId();
    let mut last_guest_beat = 0u64;
    loop {
        let now = GetTickCount64();
        shared.set_wow_beat(pid, now);
        let frame = FRAME.fetch_add(1, Ordering::Relaxed);
        // E1 will fill the real player state here; E0 only proves the link.
        shared.write_wow(WowState { frame, ..Default::default() });
        let g = shared.header().guest_heartbeat_ms;
        if g != last_guest_beat && last_guest_beat == 0 {
            log(&format!("guest attached (pid {})", shared.header().guest_pid));
        }
        last_guest_beat = g;
        Sleep(10);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(_h: HINSTANCE, reason: u32, _: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        let exe = exe_path().to_ascii_lowercase();
        if exe.ends_with("wow.exe") {
            let dir = exe_path();
            let dir = dir.rsplit_once('\\').map(|x| x.0.to_string()).unwrap_or_default();
            let path = Box::new(format!("{dir}\\skatecraft.log"));
            LOG_PATH.store(Box::into_raw(path) as isize, Ordering::Relaxed);
            CreateThread(ptr::null(), 0, Some(worker), ptr::null(), 0, ptr::null_mut());
        }
    }
    TRUE
}
