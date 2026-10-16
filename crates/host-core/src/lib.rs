//! SkateCraft host core: the logic that runs inside the WoW 3.3.5a client.
//!
//! Loaded (and reloaded) by the `dinput8.dll` proxy. `sc_start` spawns the worker, `sc_stop`
//! asks it to end and waits, so the proxy can unload this DLL and load a rebuilt one while the
//! client keeps running.

#![allow(clippy::missing_safety_doc, unsafe_op_in_unsafe_fn)]

mod collide;
mod terrain;
mod vmap;
mod wow;

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use skatecraft_protocol::*;
use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;
use windows_sys::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::{
    CreateThread, GetCurrentProcessId, Sleep, WaitForSingleObject,
};

static LOG_PATH: Mutex<String> = Mutex::new(String::new());
static STOP: AtomicBool = AtomicBool::new(false);
static THREAD: AtomicIsize = AtomicIsize::new(0);
static GAME_DIR: Mutex<String> = Mutex::new(String::new());

/// Folder with the server's `maps\` (heightmaps). Default: the project's data folder; a different
/// path can be put in `skatecraft_data.txt` next to Wow.exe.
fn data_dir() -> String {
    let game = GAME_DIR.lock().unwrap().clone();
    std::fs::read_to_string(format!("{game}\\skatecraft_data.txt"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| String::from("C:\\WOW_335_ONPREM\\skatecraft\\data"))
}

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
    *GAME_DIR.lock().unwrap() = dir.clone();
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
    let data = data_dir();
    timeBeginPeriod(1);
    let mut last_gframe = u32::MAX;
    let mut prev: Option<([f64; 3], Instant)> = None;
    let mut vel = [0.0f64; 2];
    let mut orig_speed = 0.0f32;
    let mut last_write: Option<(f32, f32, f32)> = None;
    let mut was_blocked = false;
    let mut block_seq = 0u32;
    let mut acc = (0.0f64, 0.0f64, 0.0f64);
    let mut last_iter = Instant::now();
    let (mut drift_sum, mut drift_max, mut drift_n) = (0.0f64, 0.0f32, 0u32);
    while !STOP.load(Ordering::SeqCst) {
        let now = GetTickCount64();
        shared.set_wow_beat(pid, now);
        frame = frame.wrapping_add(1);
        let state = wow::read_player(frame).unwrap_or(WowState { frame, ..Default::default() });
        let dt = {
            let n = Instant::now();
            let d = n.duration_since(last_iter).as_secs_f32().min(0.1);
            last_iter = n;
            d
        };

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
            prev = None;
            last_write = None;
            vel = [0.0; 2];
            if !driving {
                if let Some(p) = wow::local_player() {
                    wow::release(p);
                    if orig_speed > 0.0 { wow::set_speed(p, orig_speed); }
                }
            } else if let Some(p) = wow::local_player() {
                orig_speed = wow::get_speed(p).unwrap_or(7.0);
            }
        }
        if driving {
            // a new guest sample: remember it and estimate the velocity (low-pass filtered)
            let tnow = Instant::now();
            if g.frame != last_gframe {
                last_gframe = g.frame;
                if let Some((pp, pt)) = prev {
                    let dt = tnow.duration_since(pt).as_secs_f64();
                    if dt > 0.002 {
                        let nv = [(g.x - pp[0]) / dt, (g.y - pp[1]) / dt];
                        vel = [0.6 * vel[0] + 0.4 * nv[0], 0.6 * vel[1] + 0.4 * nv[1]];
                    }
                }
                prev = Some(([g.x, g.y, g.z], tnow));
            }
            if let (Some((pp, pt)), Some(p)) = (prev, wow::local_player()) {
                // extrapolate from the last sample so the character glides between guest frames
                let ahead = tnow.duration_since(pt).as_secs_f64().min(0.08);
                let (tx, ty) = ((pp[0] + vel[0] * ahead) as f32, (pp[1] + vel[1] * ahead) as f32);
                let cur = last_write.or_else(|| wow::read_pos(p)).unwrap_or((tx, ty, pp[2] as f32));
                // how far did the client move the character on its own since our last write?
                if let (Some((cx, cy, _)), Some(lw)) = (wow::read_pos(p), last_write) {
                    let d = ((cx - lw.0).powi(2) + (cy - lw.1).powi(2)).sqrt();
                    drift_sum += d as f64;
                    drift_max = drift_max.max(d);
                    drift_n += 1;
                }
                // WoW geometry decides where the character can be (walls, floors, terrain)
                let out = collide::step(&data, state.map_id, cur, (tx, ty), dt);
                if out.blocked && !was_blocked {
                    block_seq = block_seq.wrapping_add(1);
                    log(&format!("blocked #{block_seq} at ({:.2}, {:.2}, {:.2}) wanted ({tx:.2}, {ty:.2})", out.x, out.y, out.z));
                }
                was_blocked = out.blocked;
                acc = (out.x as f64, out.y as f64, out.z as f64);
                wow::set_speed(p, 0.0); // the client must not also integrate our movement
                wow::puppet(p, out.x, out.y, out.z, g.facing);
                last_write = Some((out.x, out.y, out.z));
            }
        }

        if state.flags & wow_flags::IN_WORLD != 0 && now - last_log >= 1000 {
            last_log = now;
            if drift_n > 0 {
                log(&format!("client drift per write: avg {:.4} max {:.4} yd over {} writes", drift_sum / drift_n as f64, drift_max, drift_n));
                drift_sum = 0.0; drift_max = 0.0; drift_n = 0;
            }
            let mv = wow::local_player().and_then(wow::read_movement);
            if let Some(p) = wow::local_player() {
                log(&format!("run speed {:?}", wow::get_speed(p)));
            }
            if let Some(d) = wow::local_player().and_then(wow::dump_movement) { log(&format!("cmove {d}")); }
            log(&format!(
                "player map {} pos ({:.2}, {:.2}, {:.2}) facing {:.3} ground {:?} vmap floor {:?} (tile tris {:?}) | movement {:?}",
                state.map_id, state.x, state.y, state.z, state.facing,
                terrain::height(&data, state.map_id, state.x as f32, state.y as f32),
                vmap::floor(&data, state.map_id, state.x as f32, state.y as f32, state.z as f32, 1.5, 30.0),
                vmap::tile_triangles(&data, state.map_id, state.x as f32, state.y as f32), mv
            ));
        }
        // never leave the character frozen: when nobody drives, a zero run speed (left behind by a
        // puppet session that ended abruptly, e.g. a core reload) is put back to the normal run speed
        if !driving && state.flags & wow_flags::IN_WORLD != 0 {
            if let Some(p) = wow::local_player() {
                if wow::get_speed(p) == Some(0.0) {
                    wow::set_speed(p, if orig_speed > 0.0 { orig_speed } else { 7.0 });
                    wow::release(p);
                    log("restored the character's run speed");
                }
            }
        }
        // debug: `skatecraft_scan.txt` next to Wow.exe makes the core list where buildings are
        // around the player (vmap floor clearly above the terrain), once per file creation
        if state.flags & wow_flags::IN_WORLD != 0 && !driving {
            let game = GAME_DIR.lock().unwrap().clone();
            let flag = format!("{game}\\skatecraft_scan.txt");
            if std::path::Path::new(&flag).exists() {
                let _ = std::fs::remove_file(&flag);
                let mut found = Vec::new();
                let (px, py, pz) = (state.x as f32, state.y as f32, state.z as f32);
                let mut dx = -70.0f32;
                while dx <= 70.0 {
                    let mut dy = -70.0f32;
                    while dy <= 70.0 {
                        let (x, y) = (px + dx, py + dy);
                        if let (Some(g), Some(f)) = (
                            terrain::height(&data, state.map_id, x, y),
                            vmap::floor(&data, state.map_id, x, y, pz + 40.0, 0.0, 80.0),
                        ) {
                            if f > g + 1.0 {
                                found.push((x, y, g, f));
                            }
                        }
                        dy += 3.0;
                    }
                    dx += 3.0;
                }
                log(&format!("scan: {} points with a vmap floor clearly above the terrain", found.len()));
                for (x, y, g, f) in found.iter().step_by((found.len() / 12).max(1)).take(12) {
                    log(&format!("scan: ({x:.1}, {y:.1}) terrain {g:.2} vmap floor {f:.2}"));
                }
            }
        }
        let mut st = state;
        st.ax = acc.0;
        st.ay = acc.1;
        st.az = acc.2;
        st.block_seq = block_seq;
        shared.write_wow(st);
        Sleep(if driving { 1 } else { 10 });
    }
    timeEndPeriod(1);
    log("core stopped");
    let _ = WAIT_OBJECT_0;
    0
}
