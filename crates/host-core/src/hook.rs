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

static mut SHADOW_RET: [u32; 32] = [0; 32];
static mut SHADOW_THIS: [u32; 32] = [0; 32];
static mut DEPTH: usize = 0;

#[unsafe(no_mangle)]
extern "C" fn sc_shadow_push(ret: u32, this: u32) {
    ENTRY_CALLS.fetch_add(1, Ordering::Relaxed);
    unsafe {
        let d = DEPTH.min(31);
        SHADOW_RET[d] = ret;
        SHADOW_THIS[d] = this;
        DEPTH = d + 1;
    }
}

/// Runs after the original returned; returns the original return address.
#[unsafe(no_mangle)]
extern "C" fn sc_post_main() -> u32 {
    unsafe {
        let d = DEPTH.saturating_sub(1);
        DEPTH = d;
        let (ret, this) = (SHADOW_RET[d], SHADOW_THIS[d]);
        HOOK_CALLS.fetch_add(1, Ordering::Relaxed);
        if let Some(bones) = crate::wow::read::<u32>(this as usize + 0x98) {
            let mine = PLAYER_BONES.load(Ordering::Relaxed);
            if bones != 0 && bones == mine {
                PLAYER_HITS.fetch_add(1, Ordering::Relaxed);
                crate::skel::apply(bones as usize);
            }
        }
        ret
    }
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
