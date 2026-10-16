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

/// Capability flags in [`WowState::flags`].
pub mod wow_flags {
    pub const IN_WORLD: u32 = 1 << 0;
    pub const MOUNTED: u32 = 1 << 1;
    pub const IN_COMBAT: u32 = 1 << 2;
    pub const CHAT_OPEN: u32 = 1 << 3;
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
