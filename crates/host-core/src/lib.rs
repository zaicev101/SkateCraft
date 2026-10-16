//! SkateCraft host core: the logic that runs inside the WoW 3.3.5a client.
//!
//! Loaded (and reloaded) by the `dinput8.dll` proxy. `sc_start` spawns the worker, `sc_stop`
//! asks it to end and waits, so the proxy can unload this DLL and load a rebuilt one while the
//! client keeps running.

#![allow(clippy::missing_safety_doc, unsafe_op_in_unsafe_fn)]

mod wow;

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use skatecraft_protocol::*;
use std::io::Write;
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::{
    CreateThread, GetCurrentProcessId, Sleep, WaitForSingleObject,
};

static LOG_PATH: Mutex<String> = Mutex::new(String::new());
static STOP: AtomicBool = AtomicBool::new(false);
static THREAD: AtomicIsize = AtomicIsize::new(0);

fn log(msg: &str) {
    let path = LOG_PATH.lock().unwrap().clone();
    if path.is_empty() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {msg}", unsafe { GetTickCount64() });
    }
}

/// `dir` is the NUL-terminated UTF-16 game folder (where skatecraft.log lives).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sc_start(dir: *const u16) -> i32 {
    let mut n = 0;
    while *dir.add(n) != 0 {
        n += 1;
    }
    let dir = String::from_utf16_lossy(core::slice::from_raw_parts(dir, n));
    *LOG_PATH.lock().unwrap() = format!("{dir}\\skatecraft.log");
    STOP.store(false, Ordering::SeqCst);
    let h = CreateThread(ptr::null(), 0, Some(worker), ptr::null(), 0, ptr::null_mut());
    THREAD.store(h as isize, Ordering::SeqCst);
    (!h.is_null()) as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sc_stop() {
    STOP.store(true, Ordering::SeqCst);
    let h = THREAD.swap(0, Ordering::SeqCst);
    if h != 0 {
        WaitForSingleObject(h as _, 3000);
        CloseHandle(h as _);
    }
}

unsafe extern "system" fn worker(_: *mut c_void) -> u32 {
    log("core started");
    let shared = match Shared::create() {
        Ok(s) => s,
        Err(e) => {
            log(&format!("shared region failed: error {e}"));
            return 1;
        }
    };
    let pid = GetCurrentProcessId();
    let mut frame = 0u32;
    let mut last_log = 0u64;
    let mut guest_seen = false;
    let mut driving = false;
    while !STOP.load(Ordering::SeqCst) {
        let now = GetTickCount64();
        shared.set_wow_beat(pid, now);
        frame = frame.wrapping_add(1);
        let state = wow::read_player(frame).unwrap_or(WowState { frame, ..Default::default() });
        shared.write_wow(state);

        let hdr = shared.header();
        let guest_alive = hdr.guest_heartbeat_ms != 0 && now.saturating_sub(hdr.guest_heartbeat_ms) < 1000;
        if guest_alive && !guest_seen {
            log(&format!("guest attached (pid {})", hdr.guest_pid));
        }
        guest_seen = guest_alive;

        // the guest drives the player: puppet it
        let g = shared.read_guest();
        let want = guest_alive && g.flags & guest_flags::DRIVING != 0 && state.flags & wow_flags::IN_WORLD != 0;
        if want != driving {
            driving = want;
            log(if driving { "puppet on" } else { "puppet off" });
        }
        if driving {
            if let Some(p) = wow::local_player() {
                wow::puppet(p, g.x as f32, g.y as f32, g.z as f32, g.facing);
            }
        }

        if state.flags & wow_flags::IN_WORLD != 0 && now - last_log >= 1000 {
            last_log = now;
            let mv = wow::local_player().and_then(wow::read_movement);
            log(&format!(
                "player map {} pos ({:.2}, {:.2}, {:.2}) facing {:.3} | movement {:?}",
                state.map_id, state.x, state.y, state.z, state.facing, mv
            ));
        }
        Sleep(10);
    }
    log("core stopped");
    let _ = WAIT_OBJECT_0;
    0
}
