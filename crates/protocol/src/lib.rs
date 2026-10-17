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
/// bytes of `PatchHeader` before the triangles
pub const PATCH_HDR: usize = 40;
/// The skater's pose (guest -> host), for the host to retarget onto the WoW character's bones.
pub const OFF_POSE: usize = 0xD4000;
/// The Skate camera, in WoW coordinates, for the host to drive the client's camera with.
pub const OFF_CAM: usize = 0xD8000;
/// Score and trick names (guest -> host), and the gamepad requests (set / return to a saved place).
pub const OFF_HUD: usize = 0xD9000;
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
    /// when the sample was taken (`now_us`)
    pub t_us: u64,
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
    /// the first `terrain_tris` triangles are terrain, the rest buildings and objects
    pub terrain_tris: u32,
    pub pad: u32,
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
    /// when the sample was taken (`now_us`)
    pub t_us: u64,
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
            t_us: 0,
            parts: [PoseBody::default(); POSE_PARTS],
            board: [PoseBody::default(); POSE_BOARD],
        }
    }
}

/// The guest's camera in WoW axes (X north, Y west, Z up): position, forward / left / up unit vectors and the
/// vertical field of view (radians).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CamFrame {
    pub seq: u32,
    pub flags: u32,
    pub frame: u32,
    pub pad: u32,
    pub pos: [f64; 3],
    pub fwd: [f32; 3],
    pub left: [f32; 3],
    pub up: [f32; 3],
    pub fov_v: f32,
    /// when the sample was taken (`now_us`)
    pub t_us: u64,
}


/// The trick HUD: what the Skate scoring says right now (guest -> host), plus pad requests for the addon.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct HudFrame {
    pub seq: u32,
    /// bit 0: a trick sequence is running, bit 1: clean, bit 2: sketchy, bit 3: bailing
    pub flags: u32,
    pub landed_seq: u32,
    pub bail_seq: u32,
    /// advances on every gamepad request; `req_kind` 1 = remember this place, 2 = return to the remembered place,
    /// 3 / 4 = next / previous teleport destination
    pub req_seq: u32,
    pub req_kind: u32,
    pub sequence_score: f32,
    pub line_score: f32,
    pub multiplier: f32,
    pub line_time: f32,
    pub landed_points: f32,
    pub total: f32,
    pub speed: f32,
    pub pad: f32,
    pub trick: [u8; 56],
    pub landed: [u8; 56],
}

impl Default for HudFrame {
    fn default() -> Self {
        Self { seq: 0, flags: 0, landed_seq: 0, bail_seq: 0, req_seq: 0, req_kind: 0, sequence_score: 0.0, line_score: 0.0, multiplier: 1.0, line_time: 0.0, landed_points: 0.0, total: 0.0, speed: 0.0, pad: 0.0, trick: [0; 56], landed: [0; 56] }
    }
}

impl HudFrame {
    pub fn set_text(dst: &mut [u8; 56], text: &str) {
        let mut n = text.len().min(55);
        while !text.is_char_boundary(n) {
            n -= 1;
        }
        *dst = [0; 56];
        dst[..n].copy_from_slice(&text.as_bytes()[..n]);
    }
    pub fn text(src: &[u8; 56]) -> String {
        let n = src.iter().position(|b| *b == 0).unwrap_or(56);
        String::from_utf8_lossy(&src[..n]).into_owned()
    }
}

const _: () = assert!(OFF_CAM + core::mem::size_of::<CamFrame>() <= OFF_HUD);
const _: () = assert!(OFF_HUD + core::mem::size_of::<HudFrame>() <= MAPPING_BYTES);
const _: () = assert!(OFF_POSE + core::mem::size_of::<PoseFrame>() <= OFF_CAM);
const _: () = assert!(OFF_CAM + core::mem::size_of::<CamFrame>() <= MAPPING_BYTES);
const _: () = assert!(core::mem::size_of::<PatchHeader>() == PATCH_HDR);
const _: () = assert!(OFF_PATCH + PATCH_HDR + PATCH_MAX_TRIS * 36 <= OFF_POSE);
const _: () = assert!(OFF_PATCH + PATCH_HDR + PATCH_MAX_TRIS * 36 <= MAPPING_BYTES);
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
                ptr::write_bytes(s.base.add(OFF_PATCH), 0, PATCH_HDR);
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
    pub fn write_patch(&self, map_id: u32, cx: f64, cy: f64, serial: u32, terrain_tris: u32, tris: &[[f32; 9]]) {
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
            ptr::write_volatile(ptr::addr_of_mut!((*p).terrain_tris), terrain_tris);
            ptr::copy_nonoverlapping(tris.as_ptr() as *const u8, self.base.add(OFF_PATCH + PATCH_HDR), n * 36);
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
                ptr::copy_nonoverlapping(self.base.add(OFF_PATCH + PATCH_HDR), v.as_mut_ptr() as *mut u8, n * 36);
                v.set_len(n);
                fence(Ordering::Acquire);
                if seq.load(Ordering::Acquire) == a {
                    return (h, v);
                }
            }
        }
    }

    pub fn write_cam(&self, v: CamFrame) {
        unsafe { self.write_slot(OFF_CAM, v) }
    }
    pub fn read_cam(&self) -> CamFrame {
        unsafe { self.read_slot(OFF_CAM) }
    }

    pub fn write_hud(&self, v: HudFrame) {
        unsafe { self.write_slot(OFF_HUD, v) }
    }
    pub fn read_hud(&self) -> HudFrame {
        unsafe { self.read_slot(OFF_HUD) }
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

/// Microseconds on the system-wide performance counter: the same clock in both processes.
#[cfg(windows)]
pub fn now_us() -> u64 {
    use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    unsafe {
        let (mut c, mut f) = (0i64, 0i64);
        QueryPerformanceCounter(&mut c);
        QueryPerformanceFrequency(&mut f);
        ((c as u128 * 1_000_000) / f.max(1) as u128) as u64
    }
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

/// Frame-sync event: the WoW side signals it once per presented frame, the guest waits on it instead of its own timer.
#[cfg(windows)]
fn frame_event() -> windows_sys::Win32::Foundation::HANDLE {
    use std::sync::OnceLock;
    static H: OnceLock<isize> = OnceLock::new();
    *H.get_or_init(|| unsafe {
        let name = Shared::wide(r"Local\SkateCraft_frame");
        windows_sys::Win32::System::Threading::CreateEventW(ptr::null(), 0, 0, name.as_ptr()) as isize
    }) as _
}

/// WoW side: a frame was presented.
#[cfg(windows)]
pub fn frame_signal() {
    unsafe {
        windows_sys::Win32::System::Threading::SetEvent(frame_event());
    }
}

/// Guest side: wait for the next WoW frame (true) or give up after `ms` (false).
#[cfg(windows)]
pub fn frame_wait(ms: u32) -> bool {
    unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(frame_event(), ms) == 0 }
}
