//! `dinput8.dll` proxy for the WoW 3.3.5a (build 12340) client, and hot loader for the plugin core.
//!
//! Wow.exe imports DirectInput8Create from DINPUT8.dll, so a dinput8.dll next to the exe is loaded
//! at start-up (version.dll does not work: Windows 11 maps the system copy earlier). This DLL only
//! forwards that one call and runs a watcher thread: it loads `skatecraft_core.dll` from the game
//! folder (as a private copy under `sc_live\`), and when that file changes it unloads the running
//! core and loads the new one. Rebuilding the core therefore never needs a client restart.

#![allow(non_snake_case, clippy::missing_safety_doc, unsafe_op_in_unsafe_fn)]

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicIsize, Ordering};
use windows_sys::Win32::Foundation::{FreeLibrary, HINSTANCE, TRUE};
use windows_sys::core::BOOL;
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetProcAddress, LoadLibraryW,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::{CreateThread, Sleep};

// ---------------------------------------------------------------- real dinput8.dll

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

// ---------------------------------------------------------------- hot loader

type FnStart = unsafe extern "C" fn(*const u16) -> i32;
type FnStop = unsafe extern "C" fn();

fn game_dir() -> String {
    let mut buf = [0u16; 520];
    let n = unsafe { GetModuleFileNameW(ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let exe = String::from_utf16_lossy(&buf[..n]);
    exe.rsplit_once('\\').map(|x| x.0.to_string()).unwrap_or_default()
}

fn mtime(path: &str) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn note(dir: &str, msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(format!("{dir}\\skatecraft.log")) {
        let _ = writeln!(f, "[loader] {msg}");
    }
}

unsafe extern "system" fn watcher(_: *mut c_void) -> u32 {
    let dir = game_dir();
    let src = format!("{dir}\\skatecraft_core.dll");
    let live_dir = format!("{dir}\\sc_live");
    let _ = std::fs::create_dir_all(&live_dir);
    let mut loaded: isize = 0;
    let mut stop: Option<FnStop> = None;
    let mut seen = None;
    let mut generation = 0u32;
    note(&dir, "loader started");
    loop {
        let m = mtime(&src);
        if m.is_some() && m != seen {
            // give the build a moment to finish writing the file
            Sleep(300);
            seen = mtime(&src);
            if loaded != 0 {
                if let Some(s) = stop {
                    s();
                }
                FreeLibrary(loaded as _);
                loaded = 0;
                note(&dir, "core unloaded");
            }
            generation += 1;
            let live = format!("{live_dir}\\core_{generation}.dll");
            if std::fs::copy(&src, &live).is_ok() {
                let h = LoadLibraryW(wide(&live).as_ptr());
                if h.is_null() {
                    note(&dir, "LoadLibrary failed for the new core");
                } else {
                    let start = GetProcAddress(h, b"sc_start\0".as_ptr());
                    let stopf = GetProcAddress(h, b"sc_stop\0".as_ptr());
                    if let (Some(start), Some(stopf)) = (start, stopf) {
                        stop = Some(core::mem::transmute::<_, FnStop>(stopf));
                        loaded = h as isize;
                        let ok = core::mem::transmute::<_, FnStart>(start)(wide(&dir).as_ptr());
                        note(&dir, &format!("core generation {generation} started: {ok}"));
                    } else {
                        FreeLibrary(h);
                        note(&dir, "core is missing sc_start/sc_stop");
                    }
                }
            } else {
                note(&dir, "could not copy the core");
            }
        }
        Sleep(500);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(_h: HINSTANCE, reason: u32, _: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        let mut buf = [0u16; 520];
        let n = GetModuleFileNameW(ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32) as usize;
        let exe = String::from_utf16_lossy(&buf[..n]).to_ascii_lowercase();
        if exe.ends_with("wow.exe") {
            CreateThread(ptr::null(), 0, Some(watcher), ptr::null(), 0, ptr::null_mut());
        }
    }
    TRUE
}
