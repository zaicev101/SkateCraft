//! Reading the local player out of the WoW 3.3.5a (build 12340) client.
//!
//! All addresses are for Wow.exe 3.3.5a build 12340 (image base 0x400000, no ASLR) and come
//! from public client research; every one of them is checked at run time by comparing with the
//! server's `.gps` output (see docs/PLAN.md, milestone E1) before anything depends on it.

use skatecraft_protocol::{wow_flags, WowState};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

/// Static pointer to the client connection (null before login / after disconnect).
pub const CLIENT_CONNECTION: usize = 0x00C7_9CE0;
/// ClientConnection -> object manager.
pub const CONN_OBJECT_MANAGER: usize = 0x2ED0;
/// Object manager: first object in the list, and the local player's GUID.
pub const OM_FIRST_OBJECT: usize = 0xAC;
pub const OM_LOCAL_GUID: usize = 0xC0;
/// Object: next object, type id, GUID.
pub const OBJ_NEXT: usize = 0x3C;
pub const OBJ_TYPE: usize = 0x14;
pub const OBJ_GUID: usize = 0x30;
/// Unit position (x, y, z as f32) and facing (f32, radians).
pub const UNIT_POS: usize = 0x798;
pub const UNIT_FACING: usize = 0x7A8;
/// Current continent id (static).
pub const MAP_ID: usize = 0x00AB_63BC;

pub const TYPE_PLAYER: u32 = 4;

fn read<T: Copy + Default>(addr: usize) -> Option<T> {
    let mut v = T::default();
    let mut got = 0usize;
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            addr as *const _,
            &mut v as *mut T as *mut _,
            core::mem::size_of::<T>(),
            &mut got,
        )
    };
    (ok != 0 && got == core::mem::size_of::<T>()).then_some(v)
}

/// Address of the local player object, if the client is in the world.
pub fn local_player() -> Option<usize> {
    let conn: u32 = read(CLIENT_CONNECTION)?;
    if conn == 0 {
        return None;
    }
    let om: u32 = read(conn as usize + CONN_OBJECT_MANAGER)?;
    if om == 0 {
        return None;
    }
    let guid: u64 = read(om as usize + OM_LOCAL_GUID)?;
    if guid == 0 {
        return None;
    }
    let mut obj: u32 = read(om as usize + OM_FIRST_OBJECT)?;
    // the list is short (hundreds); cap the walk so a bad pointer cannot hang the thread
    for _ in 0..20_000 {
        if obj == 0 || obj & 1 == 1 {
            return None;
        }
        let g: u64 = read(obj as usize + OBJ_GUID)?;
        if g == guid {
            return Some(obj as usize);
        }
        obj = read(obj as usize + OBJ_NEXT)?;
    }
    None
}

/// The local player's state, or `None` while not in the world.
pub fn read_player(frame: u32) -> Option<WowState> {
    let p = local_player()?;
    let x: f32 = read(p + UNIT_POS)?;
    let y: f32 = read(p + UNIT_POS + 4)?;
    let z: f32 = read(p + UNIT_POS + 8)?;
    let facing: f32 = read(p + UNIT_FACING)?;
    let map_id: u32 = read(MAP_ID).unwrap_or(0);
    Some(WowState {
        flags: wow_flags::IN_WORLD,
        map_id,
        frame,
        x: x as f64,
        y: y as f64,
        z: z as f64,
        facing,
        ..Default::default()
    })
}

// ------------------------------------------------------------------ movement (CMovement)
// The client keeps the authoritative movement state in a CMovement object hanging off the unit.
// Offsets are from public client research and are verified against `.gps` at run time.
pub const UNIT_MOVEMENT: usize = 0xD8;
pub const MOVE_POS: usize = 0x10;
pub const MOVE_FACING: usize = 0x20;
/// A second copy of position and facing lives further in the object.
pub const MOVE_POS2: usize = 0x4C;
pub const MOVE_FACING2: usize = 0x58;
/// Movement flags (bit 0 = moving forward, found by diffing the object while walking); +0x8C is the run speed.
pub const MOVE_FLAGS: usize = 0x44;
pub const MOVEFLAG_FORWARD: u32 = 1;

pub fn write<T: Copy>(addr: usize, v: T) -> bool {
    use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    let mut put = 0usize;
    let ok = unsafe {
        WriteProcessMemory(
            GetCurrentProcess(),
            addr as *mut _,
            &v as *const T as *const _,
            core::mem::size_of::<T>(),
            &mut put,
        )
    };
    ok != 0 && put == core::mem::size_of::<T>()
}

fn movement_ptr(player: usize) -> Option<usize> {
    let m: u32 = read(player + UNIT_MOVEMENT)?;
    (m != 0 && m & 3 == 0).then_some(m as usize)
}

/// (x, y, z, facing, flags) as the CMovement object holds them.
pub fn read_movement(player: usize) -> Option<(f32, f32, f32, f32, u32)> {
    let m = movement_ptr(player)?;
    Some((
        read(m + MOVE_POS)?,
        read(m + MOVE_POS + 4)?,
        read(m + MOVE_POS + 8)?,
        read(m + MOVE_FACING)?,
        0,
    ))
}

/// Put the player at a position/facing: both the cached unit position and the movement object.
pub fn puppet(player: usize, x: f32, y: f32, z: f32, facing: f32) {
    write(player + UNIT_POS, x);
    write(player + UNIT_POS + 4, y);
    write(player + UNIT_POS + 8, z);
    write(player + UNIT_FACING, facing);
    if let Some(m) = movement_ptr(player) {
        write(m + MOVE_POS, x);
        write(m + MOVE_POS + 4, y);
        write(m + MOVE_POS + 8, z);
        write(m + MOVE_FACING, facing);
        write(m + MOVE_POS2, x);
        write(m + MOVE_POS2 + 4, y);
        write(m + MOVE_POS2 + 8, z);
        write(m + MOVE_FACING2, facing);
        if let Some(fl) = read::<u32>(m + MOVE_FLAGS) {
            write(m + MOVE_FLAGS, fl | MOVEFLAG_FORWARD);
        }
    }
}

/// Raw words of the CMovement object (to discover the movement-flags field by diffing while moving).
pub fn dump_movement(player: usize) -> Option<String> {
    let m = movement_ptr(player)?;
    let mut out = String::new();
    for i in 0..48u32 {
        let w: u32 = read(m + (i as usize) * 4)?;
        out.push_str(&format!("{w:08X} "));
    }
    Some(out)
}

/// Stop pretending to walk (clear the forward flag) when the puppet is released.
pub fn release(player: usize) {
    if let Some(m) = movement_ptr(player) {
        if let Some(fl) = read::<u32>(m + MOVE_FLAGS) {
            write(m + MOVE_FLAGS, fl & !MOVEFLAG_FORWARD);
        }
    }
}
