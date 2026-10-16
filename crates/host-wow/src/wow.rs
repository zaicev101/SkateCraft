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
