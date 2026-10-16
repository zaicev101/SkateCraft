//! Draws the skateboard into the WoW frame: a hook on the D3D9 device's `EndScene` (vtable slot 42)
//! draws a few coloured boxes (deck, trucks, wheels) with fixed-function rendering, using the client's
//! own camera so the board sits exactly where the Skate side says it is (flips included).
//! It is a stand-in model: WoW has no skateboard asset.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};

/// Where the board is, in WoW world axes (yards), set by the worker thread each loop.
#[derive(Clone, Copy)]
pub struct BoardPose {
    pub pos: [f64; 3],
    /// long axis (forward), left, up of the deck in world axes
    pub axes: [[f32; 3]; 3],
    pub scale: f32,
    /// the character's own position (feet), for the debug marker
    pub unit: [f64; 3],
    pub at_ms: u64,
}

pub static BOARD: Mutex<Option<BoardPose>> = Mutex::new(None);
pub static DRAWN: AtomicU32 = AtomicU32::new(0);
/// debug: 0 = depth test off (always visible), 1 = depth tested
pub static DEPTH_TEST: AtomicU32 = AtomicU32::new(1);
pub static ZFUNC: AtomicU32 = AtomicU32::new(4);
pub static DEBUG_MARKER: AtomicU32 = AtomicU32::new(0);

pub static DEV_PROJ: Mutex<[f32; 16]> = Mutex::new([0.0; 16]);
pub static DEV_VIEW: Mutex<[f32; 16]> = Mutex::new([0.0; 16]);
/// debug: first shader-constant uploads of a frame (register, count, first 8 floats)
pub static VSC_LOG: Mutex<Vec<(u32, u32, [f32; 8])>> = Mutex::new(Vec::new());
pub static VSC_ARM: AtomicU32 = AtomicU32::new(0);
static ORIG_VSC: AtomicU32 = AtomicU32::new(0);
static SLOT_VSC: AtomicU32 = AtomicU32::new(0);

/// debug: hook of Clear (slot 43) to see when the depth buffer is cleared relative to EndScene
unsafe extern "system" fn set_vsc(dev: usize, count: u32, rects: *const c_void, flags: u32, color: u32, z: f32, stencil: u32) -> Hr {
    if VSC_ARM.load(Ordering::Relaxed) != 0 {
        if let Ok(mut l) = VSC_LOG.try_lock() {
            if l.len() < 400 {
                l.push((flags, count, [z, stencil as f32, color as f32, 0.0, 0.0, 0.0, 0.0, 0.0]));
            }
        }
    }
    let orig: unsafe extern "system" fn(usize, u32, *const c_void, u32, u32, f32, u32) -> Hr = core::mem::transmute(ORIG_VSC.load(Ordering::Relaxed) as usize);
    orig(dev, count, rects, flags, color, z, stencil)
}

pub unsafe fn install_vsc() -> bool {
    let Some(gx) = rd::<u32>(GLOBAL_GX) else { return false };
    let Some(dev) = rd::<u32>(gx as usize + GX_DEVICE) else { return false };
    let vt = *(dev as usize as *const usize);
    let slot = vt + 43 * 4;
    let orig = *(slot as *const usize);
    let mut old = 0u32;
    VirtualProtect(slot as _, 4, PAGE_EXECUTE_READWRITE, &mut old);
    ORIG_VSC.store(orig as u32, Ordering::SeqCst);
    *(slot as *mut usize) = set_vsc as usize;
    VirtualProtect(slot as _, 4, old, &mut old);
    SLOT_VSC.store(slot as u32, Ordering::SeqCst);
    true
}

pub unsafe fn remove_vsc() {
    let slot = SLOT_VSC.swap(0, Ordering::SeqCst) as usize;
    if slot == 0 {
        return;
    }
    let mut old = 0u32;
    VirtualProtect(slot as _, 4, PAGE_EXECUTE_READWRITE, &mut old);
    *(slot as *mut usize) = ORIG_VSC.load(Ordering::SeqCst) as usize;
    VirtualProtect(slot as _, 4, old, &mut old);
    windows_sys::Win32::System::Threading::Sleep(100);
}

static ORIG: AtomicU32 = AtomicU32::new(0);
static SLOT: AtomicU32 = AtomicU32::new(0);

const GLOBAL_GX: usize = 0x00C5_DF88;
const GX_DEVICE: usize = 0x397C;
const CAMERA_MGR: usize = 0x00B7_436C;
const CAMERA_OFF: usize = 0x7E20;

const VT_END_SCENE: usize = 42;
const VT_SET_TRANSFORM: usize = 44;
const VT_SET_RENDER_STATE: usize = 57;
const VT_CREATE_STATE_BLOCK: usize = 59;
const VT_SET_TEXTURE: usize = 65;
const VT_SET_TSS: usize = 67;
const VT_DRAW_PRIMITIVE_UP: usize = 83;
const VT_SET_FVF: usize = 89;
const VT_SET_VERTEX_SHADER: usize = 92;
const VT_SET_PIXEL_SHADER: usize = 107;

type Hr = i32;

unsafe fn vtbl(obj: usize, idx: usize) -> usize {
    *((*(obj as *const usize) + idx * 4) as *const usize)
}

unsafe fn set_rs(dev: usize, state: u32, value: u32) {
    let f: unsafe extern "system" fn(usize, u32, u32) -> Hr = core::mem::transmute(vtbl(dev, VT_SET_RENDER_STATE));
    f(dev, state, value);
}

unsafe fn set_tss(dev: usize, stage: u32, kind: u32, value: u32) {
    let f: unsafe extern "system" fn(usize, u32, u32, u32) -> Hr = core::mem::transmute(vtbl(dev, VT_SET_TSS));
    f(dev, stage, kind, value);
}

unsafe fn set_transform(dev: usize, which: u32, m: &[f32; 16]) {
    let f: unsafe extern "system" fn(usize, u32, *const f32) -> Hr = core::mem::transmute(vtbl(dev, VT_SET_TRANSFORM));
    f(dev, which, m.as_ptr());
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Vert {
    p: [f32; 3],
    c: u32,
}

fn rd<T: Copy + Default>(a: usize) -> Option<T> {
    crate::wow::read::<T>(a)
}

/// The client camera's forward / left / up axes (world), for the camera-bone offset.
pub fn camera_axes() -> Option<[[f32; 3]; 3]> {
    camera().map(|c| c.1)
}

/// The client camera: position, forward/left/up (world axes), near, far, fov, aspect.
fn camera() -> Option<([f64; 3], [[f32; 3]; 3], f32, f32, f32, f32)> {
    let mgr: u32 = rd(CAMERA_MGR)?;
    let cam: u32 = rd(mgr as usize + CAMERA_OFF)?;
    if cam == 0 {
        return None;
    }
    let c = cam as usize;
    let mut f = [0f32; 18];
    for (i, v) in f.iter_mut().enumerate() {
        *v = rd::<f32>(c + i * 4)?;
    }
    if f.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let pos = [f[2] as f64, f[3] as f64, f[4] as f64];
    let axes = [[f[5], f[6], f[7]], [f[8], f[9], f[10]], [f[11], f[12], f[13]]];
    Some((pos, axes, f[14], f[15], f[16], f[17]))
}

fn add_box(out: &mut Vec<Vert>, board: &BoardPose, center: [f32; 3], half: [f32; 3], color: u32, cam_pos: [f64; 3]) {
    // box corners in the board's frame (x forward, y left, z up), scaled
    let s = board.scale;
    let corner = |sx: f32, sy: f32, sz: f32| -> [f32; 3] {
        let l = [(center[0] + sx * half[0]) * s, (center[1] + sy * half[1]) * s, (center[2] + sz * half[2]) * s];
        let mut w = [0f64; 3];
        for i in 0..3 {
            w[i] = board.pos[i] + (l[0] * board.axes[0][i] + l[1] * board.axes[1][i] + l[2] * board.axes[2][i]) as f64 - cam_pos[i];
        }
        [w[0] as f32, w[1] as f32, w[2] as f32]
    };
    let shade = |c: u32, f: f32| -> u32 {
        let ch = |sh: u32| (((c >> sh) & 0xFF) as f32 * f).min(255.0) as u32;
        0xFF00_0000 | (ch(16) << 16) | (ch(8) << 8) | ch(0)
    };
    // (face normal sign, axis) -> two triangles each
    let faces: [([i32; 3], f32); 6] = [
        ([1, 0, 0], 0.8),
        ([-1, 0, 0], 0.8),
        ([0, 1, 0], 0.7),
        ([0, -1, 0], 0.7),
        ([0, 0, 1], 1.0),
        ([0, 0, -1], 0.5),
    ];
    for (n, f) in faces {
        // two tangent axes for this face
        let (a, b) = if n[0] != 0 { (1usize, 2usize) } else if n[1] != 0 { (0, 2) } else { (0, 1) };
        let ax = if n[0] != 0 { 0 } else if n[1] != 0 { 1 } else { 2 };
        let mut q = |u: f32, v: f32| {
            let mut s3 = [0f32; 3];
            s3[ax] = n[ax] as f32;
            s3[a] = u;
            s3[b] = v;
            corner(s3[0], s3[1], s3[2])
        };
        let (p00, p10, p11, p01) = (q(-1.0, -1.0), q(1.0, -1.0), q(1.0, 1.0), q(-1.0, 1.0));
        let col = shade(color, f);
        for p in [p00, p10, p11, p00, p11, p01] {
            out.push(Vert { p, c: col });
        }
    }
}

/// A box from `a` to `b` (world, camera-relative handled by the caller's `cam_pos`) with the given half width.
fn add_segment(out: &mut Vec<Vert>, a: [f64; 3], b: [f64; 3], half: f32, cam_pos: [f64; 3]) {
    let d = [(b[0] - a[0]) as f32, (b[1] - a[1]) as f32, (b[2] - a[2]) as f32];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1e-4 {
        return;
    }
    let x = [d[0] / len, d[1] / len, d[2] / len];
    let hint = if x[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let cr = |p: [f32; 3], q: [f32; 3]| [p[1] * q[2] - p[2] * q[1], p[2] * q[0] - p[0] * q[2], p[0] * q[1] - p[1] * q[0]];
    let n = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let y = n(cr(hint, x));
    let z = n(cr(x, y));
    let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5];
    let frame = BoardPose { pos: mid, axes: [x, y, z], scale: 1.0, unit: mid, at_ms: 0 };
    add_box(out, &frame, [0.0, 0.0, 0.0], [len * 0.5 + half * 0.6, half, half], 0xFFFFFFFF, cam_pos);
}

unsafe fn draw(dev: usize) {
    let Ok(guard) = BOARD.try_lock() else { return };
    let Some(board) = *guard else { return };
    drop(guard);
    let now = windows_sys::Win32::System::SystemInformation::GetTickCount64();
    if now.saturating_sub(board.at_ms) > 300 {
        return;
    }
    let Some((cpos, axes, near, far, fov, aspect)) = camera() else { return };

    // geometry: deck, grip stripe, two trucks, four wheels (board frame, metres; board bone is at the deck)
    let mut v: Vec<Vert> = Vec::with_capacity(36 * 10);
    add_box(&mut v, &board, [0.0, 0.0, 0.0], [0.40, 0.10, 0.008], 0xFFE07020, cpos);
    add_box(&mut v, &board, [0.0, 0.0, 0.009], [0.38, 0.09, 0.002], 0xFF303030, cpos);
    for sx in [-1.0f32, 1.0] {
        add_box(&mut v, &board, [sx * 0.26, 0.0, -0.025], [0.03, 0.07, 0.015], 0xFFB0B0B0, cpos);
        for sy in [-1.0f32, 1.0] {
            add_box(&mut v, &board, [sx * 0.26, sy * 0.085, -0.04], [0.03, 0.015, 0.03], 0xFFF0F0F0, cpos);
        }
    }

    if DEBUG_MARKER.load(Ordering::Relaxed) != 0 {
        let mark = BoardPose { pos: board.unit, axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], scale: 1.0, unit: board.unit, at_ms: board.at_ms };
        add_box(&mut v, &mark, [0.0, 0.0, 0.0], [0.08, 0.08, 0.08], 0xFF20E020, cpos);
    }
    // view (camera-relative: rotation only) and projection, D3D left-handed row-vector layout
    let (f, l, u) = (axes[0], axes[1], axes[2]);
    let r = [-l[0], -l[1], -l[2]];
    let view: [f32; 16] = [
        r[0], u[0], f[0], 0.0, //
        r[1], u[1], f[1], 0.0, //
        r[2], u[2], f[2], 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    // the client's fov is the horizontal one
    let xs = 1.0 / (fov * 0.5).tan();
    let ys = xs * aspect;
    let proj: [f32; 16] = [
        xs, 0.0, 0.0, 0.0, //
        0.0, ys, 0.0, 0.0, //
        0.0, 0.0, far / (far - near), 1.0, //
        0.0, 0.0, -near * far / (far - near), 0.0,
    ];
    let ident: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

    // debug: what the client set as fixed-function transforms (GetTransform = slot 45)
    {
        let get: unsafe extern "system" fn(usize, u32, *mut f32) -> Hr = core::mem::transmute(vtbl(dev, 45));
        let mut m = [0f32; 16];
        if get(dev, 3, m.as_mut_ptr()) >= 0 {
            if let Ok(mut d) = DEV_PROJ.try_lock() { *d = m; }
        }
        if get(dev, 2, m.as_mut_ptr()) >= 0 {
            if let Ok(mut d) = DEV_VIEW.try_lock() { *d = m; }
        }
    }
    // depth proxies for the legs (WoW's own depth buffer is not usable here): invisible boxes along the leg bones
    let mut proxy: Vec<Vert> = Vec::new();
    if let Ok(l) = crate::skel::LEGS.try_lock() {
        if let Some((legs, facing)) = *l {
            let (sn, cs) = facing.sin_cos();
            let k = board.scale as f64;
            let to_world = |p: [f32; 3]| -> [f64; 3] {
                let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
                [board.unit[0] + cs as f64 * x - sn as f64 * y, board.unit[1] + sn as f64 * x + cs as f64 * y, board.unit[2] + z]
            };
            let _ = k;
            for side in legs.iter() {
                let pts: Vec<[f64; 3]> = side.iter().map(|p| to_world(*p)).collect();
                add_segment(&mut proxy, pts[0], pts[1], 0.10, cpos);
                add_segment(&mut proxy, pts[1], pts[2], 0.08, cpos);
                add_segment(&mut proxy, pts[2], pts[3], 0.07, cpos);
            }
        }
    }
    // keep the client's device state intact
    let mut sb: usize = 0;
    let create: unsafe extern "system" fn(usize, u32, *mut usize) -> Hr = core::mem::transmute(vtbl(dev, VT_CREATE_STATE_BLOCK));
    if create(dev, 1, &mut sb) < 0 || sb == 0 {
        return;
    }

    let set_vs: unsafe extern "system" fn(usize, usize) -> Hr = core::mem::transmute(vtbl(dev, VT_SET_VERTEX_SHADER));
    let set_ps: unsafe extern "system" fn(usize, usize) -> Hr = core::mem::transmute(vtbl(dev, VT_SET_PIXEL_SHADER));
    let set_fvf: unsafe extern "system" fn(usize, u32) -> Hr = core::mem::transmute(vtbl(dev, VT_SET_FVF));
    let set_tex: unsafe extern "system" fn(usize, u32, usize) -> Hr = core::mem::transmute(vtbl(dev, VT_SET_TEXTURE));
    let draw_up: unsafe extern "system" fn(usize, u32, u32, *const c_void, u32) -> Hr = core::mem::transmute(vtbl(dev, VT_DRAW_PRIMITIVE_UP));

    set_vs(dev, 0);
    set_ps(dev, 0);
    set_fvf(dev, 0x42); // XYZ | DIFFUSE
    set_tex(dev, 0, 0);
    set_tss(dev, 0, 1, 2); // COLOROP = SELECTARG1
    set_tss(dev, 0, 2, 0); // COLORARG1 = DIFFUSE
    set_tss(dev, 0, 4, 1); // ALPHAOP = DISABLE
    set_tss(dev, 1, 1, 1); // stage 1 off
    let depth = if proxy.is_empty() { 0 } else { DEPTH_TEST.load(Ordering::Relaxed) };
    set_rs(dev, 7, depth); // ZENABLE
    set_rs(dev, 14, 0); // ZWRITEENABLE
    set_rs(dev, 23, ZFUNC.load(Ordering::Relaxed)); // ZFUNC (default LESSEQUAL)
    set_rs(dev, 22, 1); // CULLMODE = NONE
    set_rs(dev, 137, 0); // LIGHTING
    set_rs(dev, 27, 0); // ALPHABLENDENABLE
    set_rs(dev, 15, 0); // ALPHATESTENABLE
    set_rs(dev, 8, 3); // FILLMODE = SOLID
    set_rs(dev, 28, 0); // FOGENABLE
    set_rs(dev, 52, 0); // STENCILENABLE
    set_rs(dev, 174, 0); // SCISSORTESTENABLE
    set_rs(dev, 168, 0xF); // COLORWRITEENABLE
    set_rs(dev, 194, 0); // SRGBWRITEENABLE
    set_transform(dev, 256, &ident); // WORLD
    set_transform(dev, 2, &view); // VIEW
    set_transform(dev, 3, &proj); // PROJECTION
    if !proxy.is_empty() && depth != 0 {
        // our own depth: clear it, lay down the legs (no colour), then the board is depth-tested against them
        let clear: unsafe extern "system" fn(usize, u32, *const c_void, u32, u32, f32, u32) -> Hr = core::mem::transmute(vtbl(dev, 43));
        clear(dev, 0, core::ptr::null(), 2, 0, 1.0, 0);
        set_rs(dev, 7, 1);
        set_rs(dev, 14, 1);
        set_rs(dev, 168, 0); // no colour
        draw_up(dev, 4, (proxy.len() / 3) as u32, proxy.as_ptr() as *const c_void, core::mem::size_of::<Vert>() as u32);
        set_rs(dev, 14, 0);
        set_rs(dev, 168, 0xF);
    }
    draw_up(dev, 4, (v.len() / 3) as u32, v.as_ptr() as *const c_void, core::mem::size_of::<Vert>() as u32);

    // restore
    let apply: unsafe extern "system" fn(usize) -> Hr = core::mem::transmute(vtbl(sb, 5));
    apply(sb);
    let release: unsafe extern "system" fn(usize) -> u32 = core::mem::transmute(vtbl(sb, 2));
    release(sb);
    DRAWN.fetch_add(1, Ordering::Relaxed);
}

unsafe extern "system" fn end_scene(dev: usize) -> Hr {
    if VSC_ARM.load(Ordering::Relaxed) != 0 {
        if let Ok(mut l) = VSC_LOG.try_lock() {
            l.push((9999, 0, [0.0; 8]));
        }
    }
    draw(dev);
    let orig: unsafe extern "system" fn(usize) -> Hr = core::mem::transmute(ORIG.load(Ordering::Relaxed) as usize);
    orig(dev)
}

pub unsafe fn install() -> bool {
    if SLOT.load(Ordering::SeqCst) != 0 {
        return true;
    }
    let Some(gx) = rd::<u32>(GLOBAL_GX) else { return false };
    let Some(dev) = rd::<u32>(gx as usize + GX_DEVICE) else { return false };
    if dev == 0 {
        return false;
    }
    let vt = *(dev as usize as *const usize);
    let slot = vt + VT_END_SCENE * 4;
    let orig = *(slot as *const usize);
    // the function must live in d3d9.dll (not already hooked by someone else)
    let mut old = 0u32;
    if VirtualProtect(slot as _, 4, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
        return false;
    }
    ORIG.store(orig as u32, Ordering::SeqCst);
    *(slot as *mut usize) = end_scene as usize;
    VirtualProtect(slot as _, 4, old, &mut old);
    SLOT.store(slot as u32, Ordering::SeqCst);
    true
}

pub unsafe fn remove() {
    remove_vsc();
    let slot = SLOT.swap(0, Ordering::SeqCst) as usize;
    if slot == 0 {
        return;
    }
    let mut old = 0u32;
    VirtualProtect(slot as _, 4, PAGE_EXECUTE_READWRITE, &mut old);
    *(slot as *mut usize) = ORIG.load(Ordering::SeqCst) as usize;
    VirtualProtect(slot as _, 4, old, &mut old);
    windows_sys::Win32::System::Threading::Sleep(100);
}
