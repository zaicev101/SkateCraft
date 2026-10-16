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
    note(&dir, if hook_send_import() { "network hook installed" } else { "network hook NOT installed" });
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

// ---------------------------------------------------------------- addon channel
// The in-game addon cannot call into this DLL, so it talks through the network: it sends the
// addon message "SKATECRAFT\tON" / "SKATECRAFT\tOFF" to the player's own name. The text travels in
// plain bytes inside the outgoing packet (only the 6-byte packet header is encrypted), so this DLL
// looks for it in `send` and records the request in the shared region for the core. Wow.exe
// imports ws2_32 by ordinal (send = 19): its import table slot is replaced with `hook_send`.

use skatecraft_protocol::Shared;
use std::sync::Mutex;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_READWRITE};

static SHARED: Mutex<Option<Shared>> = Mutex::new(None);
static REAL_SEND: AtomicIsize = AtomicIsize::new(0);

type FnSend = unsafe extern "system" fn(usize, *const u8, i32, i32) -> i32;

fn find(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

unsafe extern "system" fn hook_send(s: usize, buf: *const u8, len: i32, flags: i32) -> i32 {
    if !buf.is_null() && len > 12 && len < 512 {
        let data = core::slice::from_raw_parts(buf, len as usize);
        let on = find(data, b"SKATECRAFT\tON");
        let off = !on && find(data, b"SKATECRAFT\tOFF");
        if on || off {
            if let Ok(mut g) = SHARED.lock() {
                if g.is_none() {
                    *g = Shared::create().ok();
                }
                if let Some(sh) = g.as_ref() {
                    sh.set_skate(on, skatecraft_protocol::tick_ms());
                }
            }
        }
    }
    let real = REAL_SEND.load(Ordering::Acquire);
    if real == 0 {
        return -1;
    }
    core::mem::transmute::<_, FnSend>(real)(s, buf, len, flags)
}

/// Replace the ws2_32 `send` slot (ordinal 19) in Wow.exe's import table.
unsafe fn hook_send_import() -> bool {
    let base = GetModuleHandleW(ptr::null()) as usize;
    if base == 0 {
        return false;
    }
    let rd32 = |a: usize| *(a as *const u32);
    let nt = base + rd32(base + 0x3C) as usize;
    let imports = base + rd32(nt + 0x80) as usize; // IMAGE_DIRECTORY_ENTRY_IMPORT (32-bit)
    let mut d = imports;
    while rd32(d + 12) != 0 {
        let name = core::ffi::CStr::from_ptr((base + rd32(d + 12) as usize) as *const i8).to_string_lossy().to_ascii_lowercase();
        if name == "ws2_32.dll" {
            let mut orig = base + rd32(d) as usize; // OriginalFirstThunk
            let mut slot = base + rd32(d + 16) as usize; // FirstThunk
            while rd32(orig) != 0 {
                let v = rd32(orig);
                if v & 0x8000_0000 != 0 && (v & 0xFFFF) == 19 {
                    let mut old = 0u32;
                    VirtualProtect(slot as _, 4, PAGE_READWRITE, &mut old);
                    REAL_SEND.store(rd32(slot) as isize, Ordering::Release);
                    *(slot as *mut u32) = hook_send as usize as u32;
                    VirtualProtect(slot as _, 4, old, &mut old);
                    return true;
                }
                orig += 4;
                slot += 4;
            }
        }
        d += 20;
    }
    false
}
