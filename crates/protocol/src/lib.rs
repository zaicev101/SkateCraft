//! Shared-memory protocol between the WoW 3.3.5a host plugin (32-bit) and the Skate 3
//! guest (64-bit). Both sides map the same named region; fixed-size little-endian structs,
//! no serialization library on the hot path. Single-writer "latest value" slots use a seqlock
//! (odd sequence = writer busy).
//!
//! Coordinate frames: the WoW fields are in WoW world units (yards, Z up). Mapping into the
//! Skate world (metres, Y up) is done by the guest, never here.

#![allow(clippy::missing_safety_doc, unsafe_op_in_unsafe_fn)]

use core::ptr;
use core::sync::atomic::{fence, AtomicU32, Ordering};

pub const MAGIC: u32 = 0x4B43_5341; // "ASCK"
pub const VERSION: u32 = 1;
/// Local\ names are per login session, which is what we want for two processes of one user.
pub const MAPPING_NAME: &str = "Local\\SkateCraft_v1";
pub const MAPPING_BYTES: usize = 1 << 20;

pub const OFF_HEADER: usize = 0x000;
pub const OFF_WOW_STATE: usize = 0x100;
pub const OFF_GUEST_STATE: usize = 0x200;
/// The terrain/buildings patch the host cuts out of WoW's collision data around the player.
pub const OFF_PATCH: usize = 0x1000;
pub const PATCH_MAX_TRIS: usize = 24000;
/// The skater's pose (guest -> host), for the host to retarget onto the WoW character's bones.
pub const OFF_POSE: usize = 0xD4000;
pub const POSE_PARTS: usize = 32;
pub const POSE_BOARD: usize = 8;

/// Capability flags in [`WowState::flags`].
pub mod wow_flags {
    pub const IN_WORLD: u32 = 1 << 0;
    pub const MOUNTED: u32 = 1 << 1;
    pub const IN_COMBAT: u32 = 1 << 2;
    pub const CHAT_OPEN: u32 = 1 << 3;
    /// The player switched skate mode on (the in-game addon says so, see `Header::skate_on`).
    pub const SKATE_ON: u32 = 1 << 4;
}

/// Capability flags in [`GuestState::flags`].
pub mod guest_flags {
    pub const ALIVE: u32 = 1 << 0;
    /// The guest wants to drive the player (board mode on).
    pub const DRIVING: u32 = 1 << 1;
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub magic: u32,
    pub version: u32,
    pub wow_pid: u32,
    pub guest_pid: u32,
    /// GetTickCount64 of the last beat; a side that stops beating is treated as dead.
    pub wow_heartbeat_ms: u64,
    pub guest_heartbeat_ms: u64,
    /// Set by the proxy when the addon's message passes through the client's network send:
    /// 1 = skate mode on, 0 = off; `skate_seen_ms` is when it was last heard (the addon repeats it
    /// every couple of seconds while on, so silence means off).
    pub skate_on: u32,
    pub pad_h: u32,
    pub skate_seen_ms: u64,
}

/// WoW -> guest. The player as the client sees it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WowState {
    pub seq: u32,
    pub flags: u32,
    pub map_id: u32,
    pub frame: u32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// radians, WoW convention (0 = north, counter-clockwise)
    pub facing: f32,
    pub pitch: f32,
    pub speed: f32,
    pub reserved: f32,
    /// Where the host actually put the character after its collision (WoW units), and a counter
    /// that advances each time the guest's wish was blocked: the guest pulls the skater back.
    pub ax: f64,
    pub ay: f64,
    pub az: f64,
    pub block_seq: u32,
    pub pad2: u32,
}

/// guest -> WoW. Where the skater wants the player to be.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct GuestState {
    pub seq: u32,
    pub flags: u32,
    pub teleport_seq: u32,
    pub frame: u32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub facing: f32,
    pub speed: f32,
}

/// Header of the patch slot; `count` triangles of `[f32; 9]` (three WoW-space vertices) follow it.
/// `serial` identifies the patch (changes whenever the host cuts a new one).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PatchHeader {
    pub seq: u32,
    pub count: u32,
    pub map_id: u32,
    pub serial: u32,
    pub cx: f64,
    pub cy: f64,
}

/// One body of the skater: position relative to the skater's root, world-aligned WoW axes
/// (X north, Y west, Z up), and its orientation quaternion (x, y, z, w) in the same axes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PoseBody {
    pub pos: [f32; 3],
    pub rot: [f32; 4],
}

pub mod pose_flags {
    pub const ON_BOARD: u32 = 1 << 0;
    pub const BAILING: u32 = 1 << 1;
    pub const AIRBORNE: u32 = 1 << 2;
}

/// The skater's pose. `parts` follow the Skate physics skeleton's part order, `board` its board bodies.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PoseFrame {
    pub seq: u32,
    pub flags: u32,
    pub frame: u32,
    pub nparts: u32,
    pub nboard: u32,
    pub pad: u32,
    pub facing: f32,
    pub speed: f32,
    pub parts: [PoseBody; POSE_PARTS],
    pub board: [PoseBody; POSE_BOARD],
}

impl Default for PoseFrame {
    fn default() -> Self {
        Self {
            seq: 0,
            flags: 0,
            frame: 0,
            nparts: 0,
            nboard: 0,
            pad: 0,
            facing: 0.0,
            speed: 0.0,
            parts: [PoseBody::default(); POSE_PARTS],
            board: [PoseBody::default(); POSE_BOARD],
        }
    }
}

const _: () = assert!(OFF_POSE + core::mem::size_of::<PoseFrame>() <= MAPPING_BYTES);
const _: () = assert!(OFF_PATCH + 32 + PATCH_MAX_TRIS * 36 <= OFF_POSE);
const _: () = assert!(OFF_PATCH + 32 + PATCH_MAX_TRIS * 36 <= MAPPING_BYTES);
const _: () = assert!(core::mem::size_of::<Header>() <= 0x100);
const _: () = assert!(core::mem::size_of::<WowState>() <= 0x100);
const _: () = assert!(core::mem::size_of::<GuestState>() <= 0x100);

/// A mapped view of the shared region (either side).
pub struct Shared {
    base: *mut u8,
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

#[cfg(windows)]
impl Shared {
    fn wide(name: &str) -> Vec<u16> {
        name.encode_utf16().chain(Some(0)).collect()
    }

    /// Create (or open when it already exists) the region and map it.
    pub fn create() -> Result<Self, u32> {
        use windows_sys::Win32::Foundation::GetLastError;
        use windows_sys::Win32::System::Memory::*;
        let name = Self::wide(MAPPING_NAME);
        unsafe {
            let handle = CreateFileMappingW(
                -1isize as _,
                ptr::null(),
                PAGE_READWRITE,
                0,
                MAPPING_BYTES as u32,
                name.as_ptr(),
            );
            if handle.is_null() {
                return Err(GetLastError());
            }
            let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, MAPPING_BYTES);
            if view.Value.is_null() {
                return Err(GetLastError());
            }
            let s = Shared { base: view.Value as *mut u8, handle };
            let h = s.header_ptr();
            if (*h).magic != MAGIC {
                ptr::write_bytes(s.base, 0, 0x300);
                ptr::write_bytes(s.base.add(OFF_PATCH), 0, 32);
                (*h).magic = MAGIC;
                (*h).version = VERSION;
            }
            Ok(s)
        }
    }

    /// Open an existing region (the side that did not create it).
    pub fn open() -> Result<Self, u32> {
        Self::create()
    }
}

impl Shared {
    fn header_ptr(&self) -> *mut Header {
        unsafe { self.base.add(OFF_HEADER) as *mut Header }
    }
    pub fn header(&self) -> Header {
        unsafe { ptr::read_volatile(self.header_ptr()) }
    }
    pub fn set_skate(&self, on: bool, ms: u64) {
        unsafe {
            let h = self.header_ptr();
            ptr::write_volatile(ptr::addr_of_mut!((*h).skate_on), on as u32);
            ptr::write_volatile(ptr::addr_of_mut!((*h).skate_seen_ms), ms);
        }
    }
    pub fn set_wow_beat(&self, pid: u32, ms: u64) {
        unsafe {
            let h = self.header_ptr();
            ptr::write_volatile(ptr::addr_of_mut!((*h).wow_pid), pid);
            ptr::write_volatile(ptr::addr_of_mut!((*h).wow_heartbeat_ms), ms);
        }
    }
    pub fn set_guest_beat(&self, pid: u32, ms: u64) {
        unsafe {
            let h = self.header_ptr();
            ptr::write_volatile(ptr::addr_of_mut!((*h).guest_pid), pid);
            ptr::write_volatile(ptr::addr_of_mut!((*h).guest_heartbeat_ms), ms);
        }
    }

    /// Seqlock write of a slot whose first field is a `seq: u32`.
    unsafe fn write_slot<T: Copy>(&self, off: usize, mut v: T) {
        let p = self.base.add(off) as *mut T;
        let seq = &*(p as *const AtomicU32);
        let s = seq.load(Ordering::Relaxed) & !1; // single writer: always starts even
        seq.store(s.wrapping_add(1), Ordering::Relaxed); // odd: writing
        fence(Ordering::Release);
        let size = core::mem::size_of::<T>();
        ptr::copy_nonoverlapping(
            (&mut v as *mut T as *const u8).add(4),
            (p as *mut u8).add(4),
            size - 4,
        );
        fence(Ordering::Release);
        seq.store(s.wrapping_add(2), Ordering::Release); // even: done
    }

    unsafe fn read_slot<T: Copy + Default>(&self, off: usize) -> T {
        let p = self.base.add(off) as *const T;
        let seq = &*(p as *const AtomicU32);
        loop {
            let a = seq.load(Ordering::Acquire);
            if a & 1 == 1 {
                core::hint::spin_loop();
                continue;
            }
            let v = ptr::read_volatile(p);
            fence(Ordering::Acquire);
            if seq.load(Ordering::Acquire) == a {
                return v;
            }
        }
    }

    /// Publish a new patch (single writer: the host).
    pub fn write_patch(&self, map_id: u32, cx: f64, cy: f64, serial: u32, tris: &[[f32; 9]]) {
        let n = tris.len().min(PATCH_MAX_TRIS);
        unsafe {
            let p = self.base.add(OFF_PATCH) as *mut PatchHeader;
            let seq = &*(p as *const AtomicU32);
            let s = seq.load(Ordering::Relaxed) & !1;
            seq.store(s.wrapping_add(1), Ordering::Relaxed);
            fence(Ordering::Release);
            ptr::write_volatile(ptr::addr_of_mut!((*p).count), n as u32);
            ptr::write_volatile(ptr::addr_of_mut!((*p).map_id), map_id);
            ptr::write_volatile(ptr::addr_of_mut!((*p).serial), serial);
            ptr::write_volatile(ptr::addr_of_mut!((*p).cx), cx);
            ptr::write_volatile(ptr::addr_of_mut!((*p).cy), cy);
            ptr::copy_nonoverlapping(tris.as_ptr() as *const u8, self.base.add(OFF_PATCH + 32), n * 36);
            fence(Ordering::Release);
            seq.store(s.wrapping_add(2), Ordering::Release);
        }
    }

    /// Serial of the current patch (0 = none yet); cheap, for change detection.
    pub fn patch_serial(&self) -> u32 {
        unsafe { ptr::read_volatile(ptr::addr_of!((*(self.base.add(OFF_PATCH) as *const PatchHeader)).serial)) }
    }

    /// Copy the current patch out (retries while the host is writing).
    pub fn read_patch(&self) -> (PatchHeader, Vec<[f32; 9]>) {
        unsafe {
            let p = self.base.add(OFF_PATCH) as *const PatchHeader;
            let seq = &*(p as *const AtomicU32);
            loop {
                let a = seq.load(Ordering::Acquire);
                if a & 1 == 1 {
                    core::hint::spin_loop();
                    continue;
                }
                let h = ptr::read_volatile(p);
                let n = (h.count as usize).min(PATCH_MAX_TRIS);
                let mut v: Vec<[f32; 9]> = Vec::with_capacity(n);
                ptr::copy_nonoverlapping(self.base.add(OFF_PATCH + 32), v.as_mut_ptr() as *mut u8, n * 36);
                v.set_len(n);
                fence(Ordering::Acquire);
                if seq.load(Ordering::Acquire) == a {
                    return (h, v);
                }
            }
        }
    }

    pub fn write_pose(&self, v: PoseFrame) {
        unsafe { self.write_slot(OFF_POSE, v) }
    }
    pub fn read_pose(&self) -> PoseFrame {
        unsafe { self.read_slot(OFF_POSE) }
    }

    pub fn write_wow(&self, v: WowState) {
        unsafe { self.write_slot(OFF_WOW_STATE, v) }
    }
    pub fn read_wow(&self) -> WowState {
        unsafe { self.read_slot(OFF_WOW_STATE) }
    }
    pub fn write_guest(&self, v: GuestState) {
        unsafe { self.write_slot(OFF_GUEST_STATE, v) }
    }
    pub fn read_guest(&self) -> GuestState {
        unsafe { self.read_slot(OFF_GUEST_STATE) }
    }
}

#[cfg(windows)]
impl Drop for Shared {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Memory::{UnmapViewOfFile, MEMORY_MAPPED_VIEW_ADDRESS};
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: self.base as _ });
            CloseHandle(self.handle);
        }
    }
}

/// Milliseconds on the clock both sides use for heartbeats (GetTickCount64).
#[cfg(windows)]
pub fn tick_ms() -> u64 {
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

#[cfg(windows)]
pub fn process_id() -> u32 {
    unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessId() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = Shared::create().unwrap();
        let o = Shared::open().unwrap();
        s.write_wow(WowState { map_id: 530, x: 1.5, y: -2.0, z: 3.0, facing: 0.5, ..Default::default() });
        let r = o.read_wow();
        assert_eq!((r.map_id, r.x, r.y, r.z), (530, 1.5, -2.0, 3.0));
        assert_eq!(r.seq & 1, 0, "seq must be even after a write");
        o.write_guest(GuestState { teleport_seq: 7, x: 9.0, ..Default::default() });
        assert_eq!(s.read_guest().teleport_seq, 7);
        s.set_wow_beat(11, 1234);
        assert_eq!(o.header().wow_pid, 11);
    }
}
