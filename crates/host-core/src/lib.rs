//! SkateCraft host core: the logic that runs inside the WoW 3.3.5a client.
//!
//! Loaded (and reloaded) by the `dinput8.dll` proxy. `sc_start` spawns the worker, `sc_stop`
//! asks it to end and waits, so the proxy can unload this DLL and load a rebuilt one while the
//! client keeps running.

#![allow(clippy::missing_safety_doc, unsafe_op_in_unsafe_fn)]

mod collide;
mod board;
mod camhook;
mod hook;
mod patch;
mod skel;
mod watch;
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
    hook::remove();
    board::remove();
    camhook::remove();
    for _ in 0..300 {
        if !patch::ALIVE.load(Ordering::SeqCst) {
            break;
        }
        Sleep(10);
    }
    let h = THREAD.swap(0, Ordering::SeqCst);
    if h != 0 {
        WaitForSingleObject(h as _, 3000);
        CloseHandle(h as _);
    }
}

/// Start the Skate engine (tools\run-engine.ps1) hidden. The project folder is the parent of the data folder.
fn b_ready(state: &WowState) -> bool {
    state.flags & wow_flags::IN_WORLD != 0 && hook::PLAYER_BONES.load(Ordering::Relaxed) != 0
}

fn spawn_engine(data: &str) {
    use std::os::windows::process::CommandExt;
    let root = data.rsplit_once('\\').map(|x| x.0).unwrap_or(data);
    let script = format!("{root}\\tools\\run-engine.ps1");
    match std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", &script])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
    {
        Ok(_) => log("skate mode: starting the engine"),
        Err(e) => log(&format!("skate mode: could not start the engine: {e}")),
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
    *patch::REQUEST.lock().unwrap() = None;
    patch::spawn_builder(data.clone());
    let mut patch_at: Option<(u32, f64, f64)> = None;
    let mut board_hooked = false;
    let nohook = std::path::Path::new(&format!("{}\\skatecraft_nohook.txt", GAME_DIR.lock().unwrap())).exists();
    if nohook {
        log("bone hook skipped (skatecraft_nohook.txt)");
    } else {
        log(if hook::install() { "bone hook installed" } else { "bone hook NOT installed (unknown client build?)" });
        log(if camhook::install() { "camera hooks installed" } else { "camera hooks NOT installed" });
        board_hooked = board::install() && board::install_vsc();
        log(if board_hooked { "board drawing hooked (EndScene, SetTransform)" } else { "board drawing not hooked yet (the device is created later: retrying)" });
    }
    let mut bones_checked = 0u64;
    let mut model_tried = false;
    let mut z_smooth: Option<f64> = None;
    let mut facing_s: Option<f32> = None;
    let (mut force, mut force_checked) = (false, 0u64);
    let mut watch_until: Option<Instant> = None;
    let mut vsc_until: Option<Instant> = None;
    let mut zf_checked = 0u64;
    let mut camlog_until: Option<Instant> = None;
    let mut viewlog_until: Option<Instant> = None;
    let mut applylog_until: Option<Instant> = None;
    let (mut last_cam_t, mut last_cam_ms) = (0u64, 0u64);
    let (mut cam_s0, mut cam_s1) = (camhook::CamSample::default(), camhook::CamSample::default());
    let mut cam_delay = 16_700.0f64;
    let mut orbit_polled = 0u64;
    let mut last_cursor: Option<(i32, i32)> = None;
    let mut zoom_ref: Option<f32> = None;
    let mut pin_checked = 0u64;
    let mut off_checked = 0u64;
    let mut board_retry = 0u64;
    let mut last_frames = 0u32;
    let mut last_gframe = u32::MAX;
    let mut last_gt = 0u64;
    let (mut gs0, mut gs1): (Option<(u64, [f64; 3])>, Option<(u64, [f64; 3])>) = (None, None);
    let mut gdelay = 16_700.0f64;
    let mut prev: Option<([f64; 3], Instant)> = None;
    let mut vel = [0.0f64; 2];
    let mut orig_speed = 0.0f32;
    let mut last_write: Option<(f32, f32, f32)> = None;
    let mut was_blocked = false;
    let mut block_seq = 0u32;
    let mut acc = (0.0f64, 0.0f64, 0.0f64);
    let mut last_iter = Instant::now();
    let mut last_spawn = 0u64;
    let (mut drift_sum, mut drift_max, mut drift_n) = (0.0f64, 0.0f32, 0u32);
    while !STOP.load(Ordering::SeqCst) {
        let now = GetTickCount64();
        shared.set_wow_beat(pid, now);
        frame = frame.wrapping_add(1);
        let mut state = wow::read_player(frame).unwrap_or(WowState { frame, ..Default::default() });
        let dt = {
            let n = Instant::now();
            let d = n.duration_since(last_iter).as_secs_f32().min(0.1);
            last_iter = n;
            d
        };

        let hdr = shared.header();
        // the addon's request (repeated every couple of seconds while skate mode is on)
        // (debug: a `skatecraft_force.txt` next to Wow.exe forces it on, for tests without the UI)
        if now.saturating_sub(force_checked) >= 1000 {
            force_checked = now;
            force = std::path::Path::new(&format!("{}\\skatecraft_force.txt", GAME_DIR.lock().unwrap())).exists();
        }
        let skate_requested = force || (hdr.skate_on != 0 && now.saturating_sub(hdr.skate_seen_ms) < 6000);
        if skate_requested {
            state.flags |= wow_flags::SKATE_ON;
        }
        let guest_alive = hdr.guest_heartbeat_ms != 0 && now.saturating_sub(hdr.guest_heartbeat_ms) < 1000;
        if guest_alive && !guest_seen {
            log(&format!("guest attached (pid {})", hdr.guest_pid));
        }
        guest_seen = guest_alive;
        // skate mode asked for but no Skate engine is running: start it (at most every 25 s)
        if skate_requested && !guest_alive && now.saturating_sub(last_spawn) > 25_000 {
            last_spawn = now;
            spawn_engine(&data);
        }

        // the guest drives the player: puppet it
        let g = shared.read_guest();
        // keep a collision patch cut around whoever is moving (the player, or the skater while driving)
        if skate_requested && state.flags & wow_flags::IN_WORLD != 0 {
            let (px, py) = if driving && g.flags & guest_flags::DRIVING != 0 { (g.x, g.y) } else { (state.x, state.y) };
            let stale = match patch_at {
                Some((m, ax, ay)) => m != state.map_id || (px - ax).hypot(py - ay) > 15.0,
                None => true,
            };
            if stale {
                patch_at = Some((state.map_id, px, py));
                *patch::REQUEST.lock().unwrap() = patch_at;
            }
        } else {
            patch_at = None;
        }
        let want = guest_alive
            && skate_requested
            && g.flags & guest_flags::DRIVING != 0
            && state.flags & wow_flags::IN_WORLD != 0;
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
            // guest samples carry their own timestamps (shared QPC clock). The character is shown one guest frame
            // late and interpolated between the last two samples: smooth at the render rate, no overshoot.
            if g.t_us != last_gt {
                last_gt = g.t_us;
                gs0 = gs1;
                gs1 = Some((g.t_us, [g.x, g.y, g.z]));
                if let (Some(a), Some(b)) = (gs0, gs1) {
                    let iv = b.0.saturating_sub(a.0) as f64;
                    if iv > 2000.0 && iv < 100_000.0 {
                        gdelay = 0.85 * gdelay + 0.15 * iv;
                    }
                }
            }
            if let (Some(b), Some(p)) = (gs1, wow::local_player()) {
                let tr = skatecraft_protocol::now_us() as f64 - gdelay;
                let pos = match gs0 {
                    Some(a) if b.0 > a.0 => {
                        let s = ((tr - a.0 as f64) / (b.0 - a.0) as f64).clamp(0.0, 1.25);
                        [a.1[0] + (b.1[0] - a.1[0]) * s, a.1[1] + (b.1[1] - a.1[1]) * s, a.1[2] + (b.1[2] - a.1[2]) * s]
                    }
                    _ => b.1,
                };
                let (tx, ty) = (pos[0] as f32, pos[1] as f32);
                let cur = last_write.or_else(|| wow::read_pos(p)).unwrap_or((tx, ty, pos[2] as f32));
                // how far did the client move the character on its own since our last write?
                if let (Some((cx, cy, _)), Some(lw)) = (wow::read_pos(p), last_write) {
                    let d = ((cx - lw.0).powi(2) + (cy - lw.1).powi(2)).sqrt();
                    drift_sum += d as f64;
                    drift_max = drift_max.max(d);
                    drift_n += 1;
                }
                // the Skate physics already runs inside WoW's geometry (the patch): accept its position
                let out = collide::Outcome { x: tx, y: ty, z: pos[2] as f32, blocked: false };
                let _ = (cur, dt, &mut was_blocked, &mut block_seq);
                acc = (out.x as f64, out.y as f64, out.z as f64);
                wow::set_speed(p, 0.0); // the client must not also integrate our movement
                // the unit's facing is low-passed: the client's camera follows it, and the board's heading is noisy
                let fs = facing_s.get_or_insert(g.facing);
                let mut d = g.facing - *fs;
                while d > core::f32::consts::PI { d -= 2.0 * core::f32::consts::PI; }
                while d < -core::f32::consts::PI { d += 2.0 * core::f32::consts::PI; }
                *fs += d * (1.0 - (-dt / 0.25).exp());
                wow::puppet(p, out.x, out.y, out.z, *fs);
                last_write = Some((out.x, out.y, out.z));
            }
        }

        if state.flags & wow_flags::IN_WORLD != 0 && now - last_log >= 1000 {
            last_log = now;
            if drift_n > 0 {
                log(&format!("client drift per write: avg {:.4} max {:.4} yd over {} writes", drift_sum / drift_n as f64, drift_max, drift_n));
                drift_sum = 0.0; drift_max = 0.0; drift_n = 0;
            }
            log(&format!("bone hook: {} entered, {} returned, {} on the player, {} retargeted, bones {:#x}, holder {:#x}, probe this {:#x} bones {:#x}", hook::ENTRY_CALLS.load(Ordering::Relaxed), hook::HOOK_CALLS.load(Ordering::Relaxed), hook::PLAYER_HITS.load(Ordering::Relaxed), skel::APPLIED.load(Ordering::Relaxed), hook::PLAYER_BONES.load(Ordering::Relaxed), hook::PLAYER_HOLDER.load(Ordering::Relaxed), hook::PROBE_THIS.load(Ordering::Relaxed), hook::PROBE_BONES.load(Ordering::Relaxed)));
            if let Ok(d) = skel::DEBUG.try_lock() { if !d.is_empty() { log(&format!("retarget: {}", *d)); } }
            { let f = board::FRAMES.load(Ordering::Relaxed); log(&format!("fps {}", f.wrapping_sub(last_frames))); last_frames = f; }
            log(&format!("retarget skips: stale pose {}, locked {}, no targets {}, bad array {}", skel::SKIPS[0].load(Ordering::Relaxed), skel::SKIPS[1].load(Ordering::Relaxed), skel::SKIPS[2].load(Ordering::Relaxed), skel::SKIPS[3].load(Ordering::Relaxed)));
            log(&format!("board draw stages: called {}, disabled {}, no pose {}, stale {}, no camera {}, state block failed {}, drawn {}", board::DRAW_STAGES[0].load(Ordering::Relaxed), board::DRAW_STAGES[1].load(Ordering::Relaxed), board::DRAW_STAGES[2].load(Ordering::Relaxed), board::DRAW_STAGES[3].load(Ordering::Relaxed), board::DRAW_STAGES[4].load(Ordering::Relaxed), board::DRAW_STAGES[5].load(Ordering::Relaxed), board::DRAW_STAGES[6].load(Ordering::Relaxed)));
            log(&format!("retarget safety: {} bad matrices, last bone {}", skel::BAD_COUNT.load(Ordering::Relaxed), skel::BAD_BONE.load(Ordering::Relaxed) as i32 - 1));
            if let Ok(d) = skel::DEBUG2.try_lock() { if !d.is_empty() { log(&format!("{}", *d)); } }
            if let (Ok(p), Ok(v)) = (board::DEV_PROJ.try_lock(), board::DEV_VIEW.try_lock()) { log(&format!("device proj {:.4?} view {:.3?}", *p, *v)); }
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
        if now.saturating_sub(bones_checked) >= 20 {
            bones_checked = now;
            // keep the last good values: a failed lookup must not make the hook skip frames (the model would flicker)
            if let Some(pl) = wow::local_player() {
                if let Some(b) = wow::bone_array(pl) {
                    hook::PLAYER_BONES.store(b as u32, Ordering::Relaxed);
                }
                if let Some(holder) = wow::read::<u32>(pl + wow::OBJ_MODEL).filter(|h| *h != 0) {
                    hook::PLAYER_HOLDER.store(holder, Ordering::Relaxed);
                }
            }
        }
        if !nohook && !model_tried && b_ready(&state) {
            model_tried = true;
            let t = Instant::now();
            let ok = skel::load_model();
            log(&format!("player model table: {} ({} ms)", if ok { "loaded" } else { "NOT found (unknown model: no retarget)" }, t.elapsed().as_millis()));
        }
        if !driving {
            gs0 = None;
            gs1 = None;
            cam_s0 = camhook::CamSample::default();
            cam_s1 = camhook::CamSample::default();
            last_cam_t = 0;
            last_gt = 0;
            z_smooth = None;
            facing_s = None;
            skel::CAM_DZ.store(0, Ordering::Relaxed);
        }
        if driving && now.saturating_sub(orbit_polled) >= 8 {
            orbit_polled = now;
            // detaching the camera: middle-mouse drag or numpad 4/6/8/2 orbit the skater, the wheel changes the distance, numpad 5 resets
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
            use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
            let down = |vk: i32| (GetAsyncKeyState(vk) as u16 & 0x8000) != 0;
            let (mut yaw, mut pitch) = (f32::from_bits(camhook::ORBIT_YAW.load(Ordering::Relaxed)), f32::from_bits(camhook::ORBIT_PITCH.load(Ordering::Relaxed)));
            let step = 0.02;
            if down(0x64) { yaw += step; }
            if down(0x66) { yaw -= step; }
            if down(0x68) { pitch += step; }
            if down(0x62) { pitch -= step; }
            let mut pt = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            if down(0x04) {
                if let Some((lx, ly)) = last_cursor {
                    yaw -= (pt.x - lx) as f32 * 0.006;
                    pitch += (pt.y - ly) as f32 * 0.006;
                }
                last_cursor = Some((pt.x, pt.y));
            } else {
                last_cursor = None;
            }
            if down(0x65) {
                yaw = 0.0;
                pitch = 0.0;
                zoom_ref = None;
            }
            yaw = yaw.clamp(-3.2, 3.2);
            pitch = pitch.clamp(-1.2, 1.2);
            camhook::ORBIT_YAW.store(yaw.to_bits(), Ordering::Relaxed);
            camhook::ORBIT_PITCH.store(pitch.to_bits(), Ordering::Relaxed);
            // wheel zoom (WoW's own zoom value) relative to what it was when skate mode started
            let cam = board::camera_ptr();
            if cam != 0 {
                if let Some(z) = wow::read::<f32>(cam + 0x118).filter(|z| z.is_finite()) {
                    // each wheel notch changes WoW's zoom value by about 1: 20% nearer / farther per notch
                    let r = *zoom_ref.get_or_insert(z);
                    camhook::ORBIT_MULT.store((1.0 + (z - r) * 0.2).clamp(0.15, 4.0).to_bits(), Ordering::Relaxed);
                }
            }
        }
        if !driving {
            zoom_ref = None;
            camhook::ORBIT_YAW.store(0, Ordering::Relaxed);
            camhook::ORBIT_PITCH.store(0, Ordering::Relaxed);
            camhook::ORBIT_MULT.store(0x3F80_0000, Ordering::Relaxed);
        }
        if driving {
            // the Skate camera drives the client's camera (so the view, and the stick-relative controls, match 1:1)
            let cf = shared.read_cam();
            if cf.t_us != 0 && cf.t_us != last_cam_t {
                last_cam_t = cf.t_us;
                last_cam_ms = now;
                if cf.fov_v > 0.05 {
                    cam_s0 = cam_s1;
                    cam_s1 = camhook::CamSample {
                        t_us: cf.t_us,
                        pos: [cf.pos[0] as f32, cf.pos[1] as f32, cf.pos[2] as f32],
                        fwd: cf.fwd,
                        left: cf.left,
                        up: cf.up,
                    };
                    if cam_s0.t_us != 0 {
                        let iv = cam_s1.t_us.saturating_sub(cam_s0.t_us) as f64;
                        if iv > 2000.0 && iv < 100_000.0 {
                            cam_delay = 0.85 * cam_delay + 0.15 * iv;
                        }
                    } else {
                        cam_s0 = cam_s1;
                    }
                }
            }
            if cf.fov_v > 0.05 && now.saturating_sub(last_cam_ms) < 250 {
                let cam = board::camera_ptr();
                if cam != 0 {
                    let aspect = wow::read::<f32>(cam + 0x44).filter(|a| a.is_finite() && *a > 0.5).unwrap_or(1.7778);
                    let fov_h = 2.0 * ((cf.fov_v * 0.5).tan() * aspect).atan();
                    *camhook::IMPOSED.lock().unwrap() = Some(camhook::Imposed {
                        s0: cam_s0,
                        s1: cam_s1,
                        delay_us: cam_delay,
                        fov_h,
                        at_ms: now,
                        target: [acc.0 as f32, acc.1 as f32, acc.2 as f32 + 1.0],
                    });
                    camhook::apply();
                }
            }
            let mut ps = shared.read_pose();
            if let Some(fs) = facing_s { ps.facing = fs; } // the model is posed relative to the unit's (smoothed) facing
            if ps.nparts > 0 {
                // camera: low-pass the vertical bounce of the unit (the camera bones are shifted by the difference)
                let zs = z_smooth.get_or_insert(acc.2);
                *zs += (acc.2 - *zs) * (1.0 - (-dt as f64 / 0.30).exp());
                skel::CAM_DZ.store(((*zs - acc.2) as f32).to_bits(), Ordering::Relaxed);
                // the board: where the Skate side says it is, around the character that stands at `acc`
                let bd = ps.parts[24];
                let r = skel::quat_to_r3(bd.rot);
                let col = |c: usize| [r[0][c], r[1][c], r[2][c]];
                let k = skel::model_scale();
                *board::BOARD.lock().unwrap() = Some(board::BoardPose {
                    pos: [acc.0 + (bd.pos[0] * k) as f64, acc.1 + (bd.pos[1] * k) as f64, acc.2 + (bd.pos[2] * k) as f64],
                    // Skate local z (deck length) = WoW-local x, Skate local x = WoW-local y, Skate local y (up) = WoW-local z
                    axes: [col(0), col(1), col(2)],
                    scale: k,
                    unit: [acc.0, acc.1, acc.2],
                    at_ms: now,
                });
                {
                    let mut g = skel::POSES.lock().unwrap();
                    if g[1].map_or(true, |p| p.t_us != ps.t_us) {
                        if let Some(prev) = g[1] {
                            let iv = ps.t_us.saturating_sub(prev.t_us) as f64;
                            if iv > 2000.0 && iv < 100_000.0 {
                                let old = f64::from_bits(skel::POSE_DELAY.load(Ordering::Relaxed));
                                skel::POSE_DELAY.store((0.85 * old + 0.15 * iv).to_bits(), Ordering::Relaxed);
                            }
                        }
                        g[0] = g[1].or(Some(ps));
                        g[1] = Some(ps);
                    }
                }
                skel::POSE_AT.store(now, Ordering::Relaxed);
            }
        }
        // debug: `skatecraft_watch.txt` arms a hardware write watchpoint on a bone matrix of the player's
        // model and logs, 3 s later, which client code wrote it
        if now.saturating_sub(zf_checked) >= 1000 {
            zf_checked = now;
            if let Ok(t) = std::fs::read_to_string(format!("{}\\skatecraft_zfunc.txt", GAME_DIR.lock().unwrap())) {
                if let Ok(v) = t.trim().parse::<u32>() {
                    board::ZFUNC.store(v, Ordering::Relaxed);
                    board::DEPTH_TEST.store(if v == 0 { 0 } else { 1 }, Ordering::Relaxed);
                }
            }
        }
        if !nohook && !board_hooked && now.saturating_sub(board_retry) >= 1000 {
            board_retry = now;
            board_hooked = board::install() && board::install_vsc();
            if board_hooked {
                log("board drawing hooked (EndScene, SetTransform)");
            }
        }
        if now.saturating_sub(off_checked) >= 1000 {
            off_checked = now;
            let t = std::fs::read_to_string(format!(r"{}\skatecraft_off.txt", GAME_DIR.lock().unwrap())).unwrap_or_default();
            camhook::DISABLED.store(t.contains("cam") as u32, Ordering::Relaxed);
            board::DISABLED.store(t.contains("board") as u32, Ordering::Relaxed);
            skel::DISABLED.store(t.contains("bones") as u32, Ordering::Relaxed);
        }
        if now.saturating_sub(pin_checked) >= 1000 {
            pin_checked = now;
            let v: Vec<usize> = std::fs::read_to_string(format!(r"{}\skatecraft_pin.txt", GAME_DIR.lock().unwrap()))
                .map(|t| t.split(',').filter_map(|x| x.trim().parse().ok()).collect())
                .unwrap_or_default();
            *skel::EXTRA_PIN.lock().unwrap() = v;
        }
        if applylog_until.is_some() && Instant::now() >= applylog_until.unwrap() {
            applylog_until = None;
            skel::APPLYLOG_ARM.store(0, Ordering::Relaxed);
            let rows = skel::APPLYLOG.lock().unwrap().clone();
            let mut csv = String::from("t_ms,pelvis_z,err,ankle0_now,ankle1_now,ankle0_target,ankle1_target,pose_age_ms,pose_frame,facing
");
            for r in &rows {
                csv += &r.iter().map(|v| format!("{v:.4}")).collect::<Vec<_>>().join(",");
                csv += "
";
            }
            let _ = std::fs::write(format!(r"{}\skatecraft_apply.csv", GAME_DIR.lock().unwrap()), csv);
            log(&format!("apply log: {} rows written", rows.len()));
            skel::APPLYLOG.lock().unwrap().clear();
        }
        if applylog_until.is_none() && std::path::Path::new(&format!(r"{}\skatecraft_applylog.txt", GAME_DIR.lock().unwrap())).exists() {
            let _ = std::fs::remove_file(format!(r"{}\skatecraft_applylog.txt", GAME_DIR.lock().unwrap()));
            skel::APPLYLOG.lock().unwrap().clear();
            skel::APPLYLOG_ARM.store(1, Ordering::Relaxed);
            applylog_until = Some(Instant::now() + std::time::Duration::from_secs(40));
        }
        if viewlog_until.is_some() && Instant::now() >= viewlog_until.unwrap() {
            viewlog_until = None;
            board::VIEWLOG_ARM.store(0, Ordering::Relaxed);
            let rows = board::VIEWLOG.lock().unwrap().clone();
            let mut csv = String::from("t,r0,r1,r2,u0,u1,u2,f0,f1,f2,tx,ty,tz
");
            for r in &rows {
                csv += &r.iter().map(|v| format!("{v:.5}")).collect::<Vec<_>>().join(",");
                csv += "
";
            }
            let _ = std::fs::write(format!(r"{}\skatecraft_view.csv", GAME_DIR.lock().unwrap()), csv);
            log(&format!("view log: {} frames written", rows.len()));
            board::VIEWLOG.lock().unwrap().clear();
        }
        if viewlog_until.is_none() && std::path::Path::new(&format!(r"{}\skatecraft_viewlog.txt", GAME_DIR.lock().unwrap())).exists() {
            let _ = std::fs::remove_file(format!(r"{}\skatecraft_viewlog.txt", GAME_DIR.lock().unwrap()));
            board::VIEWLOG.lock().unwrap().clear();
            board::VIEWLOG_ARM.store(1, Ordering::Relaxed);
            viewlog_until = Some(Instant::now() + std::time::Duration::from_millis(3000));
        }
        if camlog_until.is_some() && Instant::now() >= camlog_until.unwrap() {
            camlog_until = None;
            board::CAMLOG_ARM.store(0, Ordering::Relaxed);
            let rows = board::CAMLOG.lock().unwrap().clone();
            let mut csv = String::from("t,camx,camy,camz,unitx,unity,unitz,fwdz
");
            for r in &rows {
                csv += &format!("{:.0},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}
", r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7]);
            }
            let _ = std::fs::write(format!(r"{}\skatecraft_cam.csv", GAME_DIR.lock().unwrap()), csv);
            log(&format!("camera log: {} frames written", rows.len()));
            board::CAMLOG.lock().unwrap().clear();
        }
        if camlog_until.is_none() && std::path::Path::new(&format!(r"{}\skatecraft_camlog.txt", GAME_DIR.lock().unwrap())).exists() {
            let _ = std::fs::remove_file(format!(r"{}\skatecraft_camlog.txt", GAME_DIR.lock().unwrap()));
            board::CAMLOG.lock().unwrap().clear();
            board::CAMLOG_ARM.store(1, Ordering::Relaxed);
            camlog_until = Some(Instant::now() + std::time::Duration::from_millis(40000));
        }
        if vsc_until.is_some() && Instant::now() >= vsc_until.unwrap() {
            vsc_until = None;
            board::VSC_ARM.store(0, Ordering::Relaxed);
            let ev = board::EVENTS.lock().unwrap().clone();
            // run-length: P1/P2/P3 (perspective / ortho / other projection), Dn (n draws), E (EndScene)
            let mut out = String::new();
            let mut i = 0;
            while i < ev.len() {
                let (k, v) = ev[i];
                if k == b'D' {
                    let mut n = 0;
                    while i < ev.len() && ev[i].0 == b'D' { n += 1; i += 1; }
                    out += &format!("D{n} ");
                } else {
                    out += &format!("{}{} ", k as char, if k == b'P' { v } else { 0 });
                    i += 1;
                }
            }
            log(&format!("frame events: {out}"));
            board::EVENTS.lock().unwrap().clear();
        }
        if vsc_until.is_none() && state.flags & wow_flags::IN_WORLD != 0 {
            let flag = format!("{}\\skatecraft_vsc.txt", GAME_DIR.lock().unwrap());
            if std::path::Path::new(&flag).exists() {
                let _ = std::fs::remove_file(&flag);
                board::EVENTS.lock().unwrap().clear();
                board::VSC_ARM.store(1, Ordering::Relaxed);
                vsc_until = Some(Instant::now() + std::time::Duration::from_millis(12));
            }
        }
        if watch_until.is_none() && state.flags & wow_flags::IN_WORLD != 0 {
            let flagc = format!(r"{}\skatecraft_watchcam.txt", GAME_DIR.lock().unwrap());
            if std::path::Path::new(&flagc).exists() {
                let flagc_path = flagc.clone();
                let off_text = std::fs::read_to_string(&flagc_path).unwrap_or_default();
                let _ = std::fs::remove_file(&flagc);
                let cam = board::camera_ptr();
                if cam != 0 {
                    let rw = off_text.trim().starts_with("rw");
                    let off = usize::from_str_radix(off_text.trim().trim_start_matches("rw").trim().trim_start_matches("0x"), 16).unwrap_or(8);
                    log(&format!("watch: camera {cam:#x}, watching offset {off:#x}"));
                    watch::arm_mode(cam + off, rw);
                    watch_until = Some(Instant::now() + std::time::Duration::from_secs(3));
                }
            }
        }
        if watch_until.is_none() && state.flags & wow_flags::IN_WORLD != 0 {
            let game = GAME_DIR.lock().unwrap().clone();
            let flag = format!("{game}\\skatecraft_watch.txt");
            if std::path::Path::new(&flag).exists() {
                let _ = std::fs::remove_file(&flag);
                if let Some(arr) = wow::local_player().and_then(wow::bone_array) {
                    let target = arr + 5 * 64;
                    log(&format!("watch: bone array {arr:#x}, watching {target:#x}"));
                    watch::arm(target);
                    watch_until = Some(Instant::now() + std::time::Duration::from_secs(3));
                } else {
                    log("watch: no bone array found");
                }
            }
        }
        if let Some(t) = watch_until {
            if Instant::now() >= t {
                watch_until = None;
                for (eip, n, rets) in watch::disarm().iter().take(12) {
                    log(&format!("watch: write from {eip:#x} x{n}, callers {:x?}", rets));
                }
                log("watch: done");
            }
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
