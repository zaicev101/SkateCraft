//! Detour of the client's per-model bone update (Wow.exe 3.3.5a build 12340, 0x82F0F0, thiscall).
//!
//! That function fills the model's bone-matrix array (`[this + 0x98]`, 64 bytes per bone, already
//! multiplied with the model-to-view transform) once per frame. We let it run, then, when `this` is
//! the local player's model, rewrite the matrices (see `skel.rs`). The return address is hijacked so
//! the original's own stack arguments never need to be known.

use core::arch::naked_asm;
use core::sync::atomic::{AtomicU32, Ordering};
use windows_sys::Win32::System::Memory::{VirtualAlloc, VirtualProtect, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE};

pub const TARGET: usize = 0x0082_F0F0;
const ORIGINAL: [u8; 9] = [0x55, 0x8B, 0xEC, 0x81, 0xEC, 0x9C, 0x01, 0x00, 0x00];

static TRAMPOLINE: AtomicU32 = AtomicU32::new(0);
static INSTALLED: AtomicU32 = AtomicU32::new(0);
/// Bone array of the local player's model, kept up to date by the worker thread.
pub static PLAYER_BONES: AtomicU32 = AtomicU32::new(0);
pub static HOOK_CALLS: AtomicU32 = AtomicU32::new(0);
pub static PLAYER_HITS: AtomicU32 = AtomicU32::new(0);
pub static ENTRY_CALLS: AtomicU32 = AtomicU32::new(0);
/// debug: the model holder of the local player, and the nearest `this` the hook has seen
pub static PLAYER_HOLDER: AtomicU32 = AtomicU32::new(0);
pub static PROBE_THIS: AtomicU32 = AtomicU32::new(0);
pub static PROBE_BONES: AtomicU32 = AtomicU32::new(0);

/// The client calls the bone update from several threads, so the saved return addresses live in a
/// per-thread slot (found by thread id, claimed on first use).
const SLOTS: usize = 64;
const DEPTH_MAX: usize = 16;
struct Slot {
    tid: AtomicU32,
    depth: AtomicU32,
    ret: [AtomicU32; DEPTH_MAX],
    this: [AtomicU32; DEPTH_MAX],
}
#[allow(clippy::declare_interior_mutable_const)]
const SLOT_INIT: Slot = Slot {
    tid: AtomicU32::new(0),
    depth: AtomicU32::new(0),
    ret: [const { AtomicU32::new(0) }; DEPTH_MAX],
    this: [const { AtomicU32::new(0) }; DEPTH_MAX],
};
static SLOT_TABLE: [Slot; SLOTS] = [SLOT_INIT; SLOTS];

fn thread_id() -> u32 {
    let tid: u32;
    unsafe { core::arch::asm!("mov {}, fs:[0x24]", out(reg) tid, options(nostack, readonly, preserves_flags)) };
    tid
}

fn my_slot() -> &'static Slot {
    let tid = thread_id();
    let mut i = (tid as usize / 4) % SLOTS;
    loop {
        let s = &SLOT_TABLE[i];
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

#[unsafe(no_mangle)]
extern "C" fn sc_shadow_push(ret: u32, this: u32) {
    ENTRY_CALLS.fetch_add(1, Ordering::Relaxed);
    let s = my_slot();
    let d = (s.depth.load(Ordering::Relaxed) as usize).min(DEPTH_MAX - 1);
    s.ret[d].store(ret, Ordering::Relaxed);
    s.this[d].store(this, Ordering::Relaxed);
    s.depth.store(d as u32 + 1, Ordering::Relaxed);
}

/// Runs after the original returned; returns the original return address.
#[unsafe(no_mangle)]
extern "C" fn sc_post_main() -> u32 {
    let s = my_slot();
    let d = (s.depth.load(Ordering::Relaxed) as usize).saturating_sub(1);
    s.depth.store(d as u32, Ordering::Relaxed);
    let (ret, this) = (s.ret[d].load(Ordering::Relaxed), s.this[d].load(Ordering::Relaxed));
    HOOK_CALLS.fetch_add(1, Ordering::Relaxed);
    unsafe {
        let holder = PLAYER_HOLDER.load(Ordering::Relaxed);
        if holder != 0 && this.wrapping_sub(holder) < 0x200 {
            PROBE_THIS.store(this, Ordering::Relaxed);
            PROBE_BONES.store(crate::wow::read::<u32>(this as usize + 0x98).unwrap_or(0), Ordering::Relaxed);
        }
        if holder != 0 && this == holder {
            let bones = crate::wow::read::<u32>(this as usize + crate::wow::MODEL_BONES).unwrap_or(0);
            if bones != 0 {
                PLAYER_BONES.store(bones, Ordering::Relaxed);
                PLAYER_HITS.fetch_add(1, Ordering::Relaxed);
                crate::skel::apply(bones as usize);
            }
        }
    }
    ret
}

#[unsafe(naked)]
unsafe extern "C" fn hook_entry() {
    naked_asm!(
        "push ecx",
        "mov eax, [esp + 4]",
        "push ecx",
        "push eax",
        "call {push}",
        "add esp, 8",
        "pop ecx",
        "mov dword ptr [esp], offset {post}",
        "jmp dword ptr [{tramp}]",
        push = sym sc_shadow_push,
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
        main = sym sc_post_main,
    )
}

pub unsafe fn install() -> bool {
    if INSTALLED.load(Ordering::SeqCst) != 0 {
        return true;
    }
    let target = TARGET as *mut u8;
    if core::slice::from_raw_parts(target, 9) != ORIGINAL {
        return false; // not the client build we know
    }
    let tramp = VirtualAlloc(core::ptr::null(), 32, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) as *mut u8;
    if tramp.is_null() {
        return false;
    }
    core::ptr::copy_nonoverlapping(ORIGINAL.as_ptr(), tramp, 9);
    *tramp.add(9) = 0xE9;
    let back = (TARGET + 9) as i64 - (tramp as i64 + 14);
    core::ptr::write_unaligned(tramp.add(10) as *mut i32, back as i32);
    TRAMPOLINE.store(tramp as u32, Ordering::SeqCst);

    let mut old = 0u32;
    VirtualProtect(target as _, 9, PAGE_EXECUTE_READWRITE, &mut old);
    *target = 0xE9;
    let rel = hook_entry as usize as i64 - (TARGET as i64 + 5);
    core::ptr::write_unaligned(target.add(1) as *mut i32, rel as i32);
    for i in 5..9 {
        *target.add(i) = 0x90;
    }
    VirtualProtect(target as _, 9, old, &mut old);
    INSTALLED.store(1, Ordering::SeqCst);
    true
}

pub unsafe fn remove() {
    if INSTALLED.swap(0, Ordering::SeqCst) == 0 {
        return;
    }
    let target = TARGET as *mut u8;
    let mut old = 0u32;
    VirtualProtect(target as _, 9, PAGE_EXECUTE_READWRITE, &mut old);
    core::ptr::copy_nonoverlapping(ORIGINAL.as_ptr(), target, 9);
    VirtualProtect(target as _, 9, old, &mut old);
    // give a call that is already inside the hook time to leave before the DLL goes away
    windows_sys::Win32::System::Threading::Sleep(200);
}
