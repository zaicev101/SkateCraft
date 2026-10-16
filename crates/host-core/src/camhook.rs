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
pub struct Imposed {
    pub pos: [f32; 3],
    pub fwd: [f32; 3],
    pub left: [f32; 3],
    pub up: [f32; 3],
    pub fov_h: f32,
    pub at_ms: u64,
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
    *((cam + 0x08) as *mut [f32; 3]) = c.pos;
    *((cam + 0x14) as *mut [f32; 3]) = c.fwd;
    *((cam + 0x20) as *mut [f32; 3]) = c.left;
    *((cam + 0x2C) as *mut [f32; 3]) = c.up;
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
