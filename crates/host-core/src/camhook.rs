//! Hooks on the two client functions that write the camera (Wow.exe 3.3.5a build 12340):
//! - 0x607B00: the per-frame camera update (position), called from the world-view setup that reads the
//!   camera right after it returns;
//! - 0x607BD0: builds the camera's basis matrix (`this + 0x14`) from three floats.
//!
//! Each runs the original, then puts the Skate camera into the camera object before anything reads it, so
//! the client's own camera never shows up on screen (a writer thread alone loses that race and flickers).
//! Return-address hijack with a per-thread shadow stack, same scheme as `hook.rs`.

use core::arch::naked_asm;
use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use windows_sys::Win32::System::Memory::{VirtualAlloc, VirtualProtect, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE};

/// The camera to impose, in WoW coordinates, set by the worker thread; `at_ms` = when it was last refreshed.
#[derive(Clone, Copy, Default)]
pub struct CamSample {
    pub t_us: u64,
    pub pos: [f32; 3],
    pub fwd: [f32; 3],
    pub left: [f32; 3],
    pub up: [f32; 3],
}

#[derive(Clone, Copy, Default)]
pub struct Imposed {
    /// the last two guest samples (older, newer); the camera is shown one guest frame late, interpolated between them
    pub s0: CamSample,
    pub s1: CamSample,
    pub delay_us: f64,
    pub fov_h: f32,
    pub at_ms: u64,
    /// what the camera orbits when the player detaches it (the skater, WoW coordinates)
    pub target: [f32; 3],
}

/// Player-controlled offset of the camera around the skater: yaw, pitch (radians) and distance multiplier (f32 bits).
pub static ORBIT_YAW: AtomicU32 = AtomicU32::new(0);
pub static ORBIT_PITCH: AtomicU32 = AtomicU32::new(0);
pub static ORBIT_MULT: AtomicU32 = AtomicU32::new(0x3F80_0000); // 1.0

fn windows_us() -> u64 {
    skatecraft_protocol::now_us()
}

/// Seconds since the first call (monotonic, sub-millisecond).
pub fn clock() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

pub static IMPOSED: Mutex<Option<Imposed>> = Mutex::new(None);
pub static APPLIED: AtomicU32 = AtomicU32::new(0);

/// Write the imposed camera into the camera object (same-process pointers, game or worker thread).
pub unsafe fn apply() {
    let cam = crate::board::camera_ptr();
    if cam == 0 {
        return;
    }
    let Ok(g) = IMPOSED.try_lock() else { return };
    let Some(c) = *g else { return };
    drop(g);
    let now = windows_sys::Win32::System::SystemInformation::GetTickCount64();
    if now.saturating_sub(c.at_ms) > 250 {
        return;
    }
    // interpolate between the two newest guest samples at (now - one guest frame)
    let tr = windows_us() as f64 - c.delay_us;
    let (a, b) = (c.s0, c.s1);
    let s = if b.t_us > a.t_us { ((tr - a.t_us as f64) / (b.t_us - a.t_us) as f64).clamp(0.0, 1.25) as f32 } else { 1.0 };
    let lerp = |x: [f32; 3], y: [f32; 3]| [x[0] + (y[0] - x[0]) * s, x[1] + (y[1] - x[1]) * s, x[2] + (y[2] - x[2]) * s];
    let norm = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let mut pos = lerp(a.pos, b.pos);
    let mut fwd = norm(lerp(a.fwd, b.fwd));
    let mut left = norm(lerp(a.left, b.left));
    let mut up = norm(lerp(a.up, b.up));
    let (yaw, pitch, mult) = (
        f32::from_bits(ORBIT_YAW.load(Ordering::Relaxed)),
        f32::from_bits(ORBIT_PITCH.load(Ordering::Relaxed)),
        f32::from_bits(ORBIT_MULT.load(Ordering::Relaxed)),
    );
    if yaw.abs() > 1e-4 || pitch.abs() > 1e-4 || (mult - 1.0).abs() > 1e-3 {
        // detached: orbit the skater (yaw about the world up axis, pitch about the sideways axis, distance scaled)
        let t = c.target;
        let mut v = [pos[0] - t[0], pos[1] - t[1], pos[2] - t[2]];
        let (sy, cy) = yaw.sin_cos();
        v = [v[0] * cy - v[1] * sy, v[0] * sy + v[1] * cy, v[2]];
        let side = norm([-v[1], v[0], 0.0]); // horizontal, perpendicular to the view line
        let (sp, cp) = pitch.sin_cos();
        let dotv = side[0] * v[0] + side[1] * v[1] + side[2] * v[2];
        let crs = [side[1] * v[2] - side[2] * v[1], side[2] * v[0] - side[0] * v[2], side[0] * v[1] - side[1] * v[0]];
        v = [
            v[0] * cp + crs[0] * sp + side[0] * dotv * (1.0 - cp),
            v[1] * cp + crs[1] * sp + side[1] * dotv * (1.0 - cp),
            v[2] * cp + crs[2] * sp + side[2] * dotv * (1.0 - cp),
        ];
        v = [v[0] * mult, v[1] * mult, v[2] * mult];
        pos = [t[0] + v[0], t[1] + v[1], t[2] + v[2]];
        fwd = norm([t[0] - pos[0], t[1] - pos[1], t[2] - pos[2]]);
        left = norm([-fwd[1], fwd[0], 0.0]); // up x forward with up = Z
        up = [fwd[1] * left[2] - fwd[2] * left[1], fwd[2] * left[0] - fwd[0] * left[2], fwd[0] * left[1] - fwd[1] * left[0]];
    }
    *((cam + 0x08) as *mut [f32; 3]) = pos;
    *((cam + 0x14) as *mut [f32; 3]) = fwd;
    *((cam + 0x20) as *mut [f32; 3]) = left;
    *((cam + 0x2C) as *mut [f32; 3]) = up;
    *((cam + 0x40) as *mut f32) = c.fov_h;
    APPLIED.fetch_add(1, Ordering::Relaxed);
}

macro_rules! define_hook {
    ($m:ident, $target:expr, $orig:expr) => {
        pub mod $m {
            use super::*;
            pub const TARGET: usize = $target;
            const ORIGINAL: &[u8] = &$orig;
            static TRAMPOLINE: AtomicU32 = AtomicU32::new(0);
            static INSTALLED: AtomicU32 = AtomicU32::new(0);
            pub static CALLS: AtomicU32 = AtomicU32::new(0);

            const SLOTS: usize = 64;
            const DEPTH_MAX: usize = 8;
            struct Slot {
                tid: AtomicU32,
                depth: AtomicU32,
                ret: [AtomicU32; DEPTH_MAX],
            }
            #[allow(clippy::declare_interior_mutable_const)]
            const SLOT_INIT: Slot = Slot { tid: AtomicU32::new(0), depth: AtomicU32::new(0), ret: [const { AtomicU32::new(0) }; DEPTH_MAX] };
            static TABLE: [Slot; SLOTS] = [SLOT_INIT; SLOTS];

            fn my_slot() -> &'static Slot {
                let tid: u32;
                unsafe { core::arch::asm!("mov {}, fs:[0x24]", out(reg) tid, options(nostack, readonly, preserves_flags)) };
                let mut i = (tid as usize / 4) % SLOTS;
                loop {
                    let s = &TABLE[i];
                    let cur = s.tid.load(Ordering::Acquire);
                    if cur == tid {
                        return s;
                    }
                    if cur == 0 && s.tid.compare_exchange(0, tid, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                        return s;
                    }
                    i = (i + 1) % SLOTS;
                }
            }

            extern "C" fn push(ret: u32) {
                let s = my_slot();
                let d = (s.depth.load(Ordering::Relaxed) as usize).min(DEPTH_MAX - 1);
                s.ret[d].store(ret, Ordering::Relaxed);
                s.depth.store(d as u32 + 1, Ordering::Relaxed);
            }

            extern "C" fn post_main() -> u32 {
                let s = my_slot();
                let d = (s.depth.load(Ordering::Relaxed) as usize).saturating_sub(1);
                s.depth.store(d as u32, Ordering::Relaxed);
                CALLS.fetch_add(1, Ordering::Relaxed);
                unsafe { apply() };
                s.ret[d].load(Ordering::Relaxed)
            }

            #[unsafe(naked)]
            unsafe extern "C" fn entry() {
                naked_asm!(
                    "push ecx",
                    "mov eax, [esp + 4]",
                    "push eax",
                    "call {push}",
                    "add esp, 4",
                    "pop ecx",
                    "mov dword ptr [esp], offset {post}",
                    "jmp dword ptr [{tramp}]",
                    push = sym push,
                    post = sym post_stub,
                    tramp = sym TRAMPOLINE,
                )
            }

            #[unsafe(naked)]
            unsafe extern "C" fn post_stub() {
                naked_asm!(
                    "push 0",
                    "pushad",
                    "pushfd",
                    "call {main}",
                    "mov [esp + 36], eax",
                    "popfd",
                    "popad",
                    "ret",
                    main = sym post_main,
                )
            }

            pub unsafe fn install() -> bool {
                if INSTALLED.load(Ordering::SeqCst) != 0 {
                    return true;
                }
                let n = ORIGINAL.len();
                let target = TARGET as *mut u8;
                if core::slice::from_raw_parts(target, n) != ORIGINAL {
                    return false;
                }
                let tramp = VirtualAlloc(core::ptr::null(), 32, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) as *mut u8;
                if tramp.is_null() {
                    return false;
                }
                core::ptr::copy_nonoverlapping(ORIGINAL.as_ptr(), tramp, n);
                *tramp.add(n) = 0xE9;
                let back = (TARGET + n) as i64 - (tramp as i64 + n as i64 + 5);
                core::ptr::write_unaligned(tramp.add(n + 1) as *mut i32, back as i32);
                TRAMPOLINE.store(tramp as u32, Ordering::SeqCst);
                let mut old = 0u32;
                VirtualProtect(target as _, n, PAGE_EXECUTE_READWRITE, &mut old);
                *target = 0xE9;
                let rel = entry as usize as i64 - (TARGET as i64 + 5);
                core::ptr::write_unaligned(target.add(1) as *mut i32, rel as i32);
                for i in 5..n {
                    *target.add(i) = 0x90;
                }
                VirtualProtect(target as _, n, old, &mut old);
                INSTALLED.store(1, Ordering::SeqCst);
                true
            }

            pub unsafe fn remove() {
                if INSTALLED.swap(0, Ordering::SeqCst) == 0 {
                    return;
                }
                let target = TARGET as *mut u8;
                let mut old = 0u32;
                VirtualProtect(target as _, ORIGINAL.len(), PAGE_EXECUTE_READWRITE, &mut old);
                core::ptr::copy_nonoverlapping(ORIGINAL.as_ptr(), target, ORIGINAL.len());
                VirtualProtect(target as _, ORIGINAL.len(), old, &mut old);
            }
        }
    };
}

// push ebp; mov ebp,esp; push esi; mov esi,[ebp+0xc]
define_hook!(update, 0x0060_7B00, [0x55, 0x8B, 0xEC, 0x56, 0x8B, 0x75, 0x0C]);
// push ebp; mov ebp,esp; fld dword ptr [ebp+0x10]
define_hook!(basis, 0x0060_7BD0, [0x55, 0x8B, 0xEC, 0xD9, 0x45, 0x10]);

pub unsafe fn install() -> bool {
    update::install() & basis::install()
}

pub unsafe fn remove() {
    update::remove();
    basis::remove();
    windows_sys::Win32::System::Threading::Sleep(150);
}
