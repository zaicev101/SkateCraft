//! Lua functions the SkateCraft addon calls, registered into the client's Lua state from the render thread (the same
//! thread that runs the UI scripts), so they never race with Lua. The Lua state is rebuilt on every UI reload: it is
//! re-registered whenever its address changes.
use core::sync::atomic::{AtomicU32, Ordering};
use skatecraft_protocol::HudFrame;
use std::sync::Mutex;

const GLOBAL_LUA_STATE: usize = 0x00D3_F78C;
const REGISTER_FUNCTION: usize = 0x0081_7F90; // (name, cfunction), cdecl
const PUSH_NUMBER: usize = 0x0084_E2A0; // (L, double)
const PUSH_STRING: usize = 0x0084_E350; // (L, const char*)

pub static HUD: Mutex<Option<HudFrame>> = Mutex::new(None);
/// map id, x, y, z, facing of the player as the host last saw it
pub static WOW: Mutex<(u32, f64, f64, f64, f32)> = Mutex::new((0, 0.0, 0.0, 0.0, 0.0));
static REGISTERED_FOR: AtomicU32 = AtomicU32::new(0);

unsafe fn push_number(l: usize, v: f64) {
    let f: unsafe extern "C" fn(usize, f64) = core::mem::transmute(PUSH_NUMBER);
    f(l, v);
}

unsafe fn push_string(l: usize, s: &str) {
    let mut b = [0u8; 80];
    let n = s.len().min(79);
    b[..n].copy_from_slice(&s.as_bytes()[..n]);
    let f: unsafe extern "C" fn(usize, *const u8) = core::mem::transmute(PUSH_STRING);
    f(l, b.as_ptr());
}

/// SkateCraft_State() -> flags, sequenceScore, lineScore, multiplier, lineTime, landedPoints, total, speed,
///                       landedSeq, bailSeq, reqSeq, reqKind, trick, landed
unsafe extern "C" fn lua_state(l: usize) -> i32 {
    let h = HUD.try_lock().ok().and_then(|g| *g).unwrap_or_default();
    for v in [
        h.flags as f64, h.sequence_score as f64, h.line_score as f64, h.multiplier as f64, h.line_time as f64,
        h.landed_points as f64, h.total as f64, h.speed as f64, h.landed_seq as f64, h.bail_seq as f64,
        h.req_seq as f64, h.req_kind as f64,
    ] {
        push_number(l, v);
    }
    push_string(l, &HudFrame::text(&h.trick));
    push_string(l, &HudFrame::text(&h.landed));
    14
}

/// SkateCraft_Pos() -> mapId, x, y, z, facing
unsafe extern "C" fn lua_pos(l: usize) -> i32 {
    let w = WOW.try_lock().map(|g| *g).unwrap_or_default();
    for v in [w.0 as f64, w.1, w.2, w.3, w.4 as f64] {
        push_number(l, v);
    }
    5
}

/// The client only accepts Lua C functions that live inside Wow.exe ("Invalid function pointer"), so each function is
/// reached through a 5-byte `jmp` written into the int3 padding between two client functions.
const TRAMPOLINES: [usize; 2] = [0x0054_3642, 0x0054_364A];

unsafe fn write_jump(slot: usize, target: Option<usize>) -> bool {
    use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};
    let p = slot as *mut u8;
    let mut old = 0u32;
    if VirtualProtect(p as _, 5, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
        return false;
    }
    match target {
        Some(t) => {
            let rel = (t as i64 - (slot as i64 + 5)) as i32;
            *p = 0xE9;
            core::ptr::copy_nonoverlapping(rel.to_le_bytes().as_ptr(), p.add(1), 4);
        }
        // unloaded: `xor eax, eax; ret` (no results)
        None => {
            *p = 0x31;
            *p.add(1) = 0xC0;
            *p.add(2) = 0xC3;
            *p.add(3) = 0xCC;
            *p.add(4) = 0xCC;
        }
    }
    VirtualProtect(p as _, 5, old, &mut old);
    true
}

unsafe fn register(name: &[u8], slot: usize, f: unsafe extern "C" fn(usize) -> i32) -> bool {
    // only ever write into padding or into one of our own earlier trampolines
    let b = core::slice::from_raw_parts(slot as *const u8, 5);
    let free = b.iter().all(|x| *x == 0xCC);
    let ours = b[0] == 0xE9 || (b[0] == 0x31 && b[1] == 0xC0 && b[2] == 0xC3);
    if !(free || ours) || !write_jump(slot, Some(f as usize)) {
        return false;
    }
    let reg: unsafe extern "C" fn(*const u8, usize) = core::mem::transmute(REGISTER_FUNCTION);
    reg(name.as_ptr(), slot);
    true
}

/// Called every frame on the render thread.
pub unsafe fn ensure_registered() {
    let Some(l) = crate::wow::read::<u32>(GLOBAL_LUA_STATE) else { return };
    if l == 0 || l == REGISTERED_FOR.load(Ordering::Relaxed) {
        return;
    }
    let ok = register(b"SkateCraft_State\0", TRAMPOLINES[0], lua_state) && register(b"SkateCraft_Pos\0", TRAMPOLINES[1], lua_pos);
    REGISTERED_FOR.store(l, Ordering::Relaxed);
    crate::log(if ok { "Lua functions registered (SkateCraft_State, SkateCraft_Pos)" } else { "Lua functions NOT registered (no free trampoline slot)" });
}

/// The core is being unloaded: the trampolines must not jump into the unloaded DLL.
pub unsafe fn forget() {
    REGISTERED_FOR.store(0, Ordering::Relaxed);
    for slot in TRAMPOLINES {
        write_jump(slot, None);
    }
}
