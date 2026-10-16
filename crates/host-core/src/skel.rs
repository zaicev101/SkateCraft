//! Retargets the Skate skater's pose onto the local player's WoW model, by rewriting the bone-matrix
//! array after the client computed it (see `hook.rs`).
//!
//! Matrices are 4x4 row-major, row-vector convention (`v * M`), 64 bytes per bone, and already include
//! the model-to-view transform `W` (= the root bone 0's matrix, whose own local transform is identity).
//! Model-space bone transforms are therefore `B = M * W^-1`, and `B_child = L_child * B_parent`.
//!
//! Skate's joint positions say where each limb points; every WoW bone gets the shortest rotation from
//! *its own* rest direction (child pivot - bone pivot, from the M2 bone table) to that direction, so the
//! different proportions and rest poses of the two rigs do not matter. Torso frames use the hips/shoulder
//! lines for twist. Bones without a counterpart keep the client's own local animation.

use core::sync::atomic::{AtomicU64, Ordering};
use skatecraft_protocol::PoseFrame;
use std::sync::Mutex;

pub type Mat = [f32; 16];
type V3 = [f32; 3];
/// Column-vector rotation: `R * v`, `R[row][col]`.
type R3 = [[f32; 3]; 3];

// ---------------------------------------------------------------- state shared with the worker

pub struct Model {
    pub pivot: Vec<V3>,
    pub parent: Vec<i32>,
}

pub static MODEL: Mutex<Option<Model>> = Mutex::new(None);
pub static POSE: Mutex<Option<PoseFrame>> = Mutex::new(None);
pub static POSE_AT: AtomicU64 = AtomicU64::new(0);
pub static DEBUG: Mutex<String> = Mutex::new(String::new());
pub static APPLIED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

const NBONES: usize = 228;
// bone roles of the 228-bone human model (checked against pivots when the model is loaded)
const PELVIS: usize = 2;
const SPINE_LOW: usize = 3;
const WAIST: usize = 8;
const CHEST: usize = 21;
const NECK: usize = 28;
const NECK2: usize = 34;
const HEAD: usize = 44;
const SHOULDER: [usize; 2] = [27, 26]; // [right, left]
const ARM: [usize; 2] = [33, 32];
const FOREARM: [[usize; 2]; 2] = [[41, 42], [38, 40]];
const HAND: [[usize; 2]; 2] = [[51, 52], [46, 48]];
const THIGH: [usize; 2] = [5, 6];
const SHIN: [[usize; 2]; 2] = [[16, 17], [18, 19]];
const FOOT: [[usize; 2]; 2] = [[23, 30], [24, 31]];
const TOE: [usize; 2] = [36, 37];

// Skate pose slots (see `POSE_BONES` in the guest bridge)
const S_HIPS: usize = 0;
const S_SPINE: usize = 1;
const S_SPINE1: usize = 2;
const S_SPINE3: usize = 4;
const S_NECK1: usize = 6;
const S_HEAD: usize = 7;
const S_SHOULDER: [usize; 2] = [8, 12];
const S_ARM: [usize; 2] = [9, 13];
const S_FOREARM: [usize; 2] = [10, 14];
const S_HAND: [usize; 2] = [11, 15];
const S_UPLEG: [usize; 2] = [16, 20];
const S_LEG: [usize; 2] = [17, 21];
const S_FOOT: [usize; 2] = [18, 22];
const S_TOE: [usize; 2] = [19, 23];

// ---------------------------------------------------------------- small math

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn norm(a: V3) -> V3 {
    let l = len(a);
    if l < 1e-6 { [0.0, 0.0, 1.0] } else { scale(a, 1.0 / l) }
}

const I3: R3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn mul3(a: &R3, b: &R3) -> R3 {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    r
}
fn tr3(a: &R3) -> R3 {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = a[j][i];
        }
    }
    r
}
fn apply3(r: &R3, v: V3) -> V3 {
    [dot(r[0], v), dot(r[1], v), dot(r[2], v)]
}
fn from_cols(x: V3, y: V3, z: V3) -> R3 {
    [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]]
}
fn col(r: &R3, c: usize) -> V3 {
    [r[0][c], r[1][c], r[2][c]]
}
/// Gram-Schmidt on the columns (after blending).
fn orthonormal(r: &R3) -> R3 {
    let x = norm(col(r, 0));
    let y0 = col(r, 1);
    let y = norm(sub(y0, scale(x, dot(x, y0))));
    from_cols(x, y, cross(x, y))
}
fn blend(a: &R3, b: &R3, t: f32) -> R3 {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = a[i][j] * (1.0 - t) + b[i][j] * t;
        }
    }
    orthonormal(&r)
}

/// Shortest rotation taking direction `a` onto direction `b`.
fn aim(a: V3, b: V3) -> R3 {
    let (a, b) = (norm(a), norm(b));
    let k = cross(a, b);
    let c = dot(a, b);
    let s2 = dot(k, k);
    if s2 < 1e-10 {
        if c > 0.0 {
            return I3;
        }
        // opposite: half turn about any axis perpendicular to a
        let p = if a[0].abs() < 0.9 { norm(cross(a, [1.0, 0.0, 0.0])) } else { norm(cross(a, [0.0, 1.0, 0.0])) };
        return [
            [2.0 * p[0] * p[0] - 1.0, 2.0 * p[0] * p[1], 2.0 * p[0] * p[2]],
            [2.0 * p[1] * p[0], 2.0 * p[1] * p[1] - 1.0, 2.0 * p[1] * p[2]],
            [2.0 * p[2] * p[0], 2.0 * p[2] * p[1], 2.0 * p[2] * p[2] - 1.0],
        ];
    }
    let kx = [[0.0, -k[2], k[1]], [k[2], 0.0, -k[0]], [-k[1], k[0], 0.0]];
    let kx2 = mul3(&kx, &kx);
    let f = (1.0 - c) / s2;
    let mut r = I3;
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] += kx[i][j] + kx2[i][j] * f;
        }
    }
    r
}

/// Frame with axes (forward, left, up) from an up hint and a left hint (model space: x forward, y left, z up).
fn frame(up: V3, left: V3) -> R3 {
    let u = norm(up);
    let l = norm(sub(left, scale(u, dot(u, left))));
    from_cols(cross(l, u), l, u)
}

// 4x4 (row-vector) helpers

pub fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut r = [0.0f32; 16];
    for i in 0..4 {
        for j in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[i * 4 + k] * b[k * 4 + j];
            }
            r[i * 4 + j] = s;
        }
    }
    r
}

/// Inverse of a rotation + translation (rows 0..2 orthonormal, row 3 translation).
pub fn inv_rigid(m: &Mat) -> Mat {
    let mut r = [0.0f32; 16];
    for i in 0..3 {
        for j in 0..3 {
            r[i * 4 + j] = m[j * 4 + i];
        }
    }
    for j in 0..3 {
        r[12 + j] = -(m[12] * r[j] + m[13] * r[4 + j] + m[14] * r[8 + j]);
    }
    r[15] = 1.0;
    r
}

/// Bone transform that turns the bone by `r` (model-space rotation) about its pivot and puts the pivot at `joint`.
fn bone_matrix(r: &R3, pivot: V3, joint: V3) -> Mat {
    let rp = apply3(r, pivot);
    let t = sub(joint, rp);
    let mut m = [0.0f32; 16];
    for i in 0..3 {
        for j in 0..3 {
            m[i * 4 + j] = r[j][i]; // row-vector convention: transpose
        }
    }
    m[12] = t[0];
    m[13] = t[1];
    m[14] = t[2];
    m[15] = 1.0;
    m
}

fn bone_rot(m: &Mat) -> R3 {
    // transpose of the top-left block gives the column-vector rotation
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = m[j * 4 + i];
        }
    }
    r
}

// ---------------------------------------------------------------- model table

/// Find the player's M2 header in this process (a 228-bone model with the expected head pivot) and
/// read its bone table. Slow (scans memory): call from the worker thread, once.
pub unsafe fn load_model() -> bool {
    use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT};
    let needle: [u8; 8] = [b'M', b'D', b'2', b'0', 0x08, 0x01, 0x00, 0x00];
    let mut addr = 0x0001_0000usize;
    while addr < 0x7FFE_0000 {
        let mut mbi: MEMORY_BASIC_INFORMATION = core::mem::zeroed();
        if VirtualQuery(addr as *const _, &mut mbi, core::mem::size_of::<MEMORY_BASIC_INFORMATION>()) == 0 {
            break;
        }
        let (base, size) = (mbi.BaseAddress as usize, mbi.RegionSize);
        let readable = mbi.State == MEM_COMMIT && mbi.Protect & 0x66 != 0 && mbi.Protect & 0x101 == 0; // R/RW/WC
        if readable && size >= 0x100 {
            let hay = core::slice::from_raw_parts(base as *const u8, size);
            let mut i = 0;
            while i + 0x40 <= size {
                if hay[i..i + 8] == needle {
                    if let Some(m) = parse(base + i) {
                        *MODEL.lock().unwrap() = Some(m);
                        return true;
                    }
                }
                i += 4;
            }
        }
        addr = base + size;
    }
    false
}

unsafe fn parse(h: usize) -> Option<Model> {
    let rd = |a: usize| -> u32 { *(a as *const u32) };
    let n = rd(h + 0x2C) as usize;
    let ob = rd(h + 0x30) as usize;
    if n != NBONES || !(0x10000..0x7FFE_0000).contains(&ob) {
        return None;
    }
    let mut pivot = Vec::with_capacity(n);
    let mut parent = Vec::with_capacity(n);
    for i in 0..n {
        let b = ob + i * 0x58;
        parent.push(*((b + 8) as *const i16) as i32);
        pivot.push([*((b + 0x4C) as *const f32), *((b + 0x50) as *const f32), *((b + 0x54) as *const f32)]);
    }
    // sanity: the head sits high on the centre line, the right thigh is on the right
    let (hp, tp) = (pivot[HEAD], pivot[THIGH[0]]);
    if !(1.6..2.1).contains(&hp[2]) || hp[1].abs() > 0.1 || tp[1] > -0.05 || parent[HEAD] != NECK2 as i32 {
        return None;
    }
    Some(Model { pivot, parent })
}

// ---------------------------------------------------------------- the retarget

/// Pose-derived absolute model-space rotations/joints for the mapped bones.
struct Targets {
    rot: Vec<Option<R3>>,
    joint_pelvis: V3,
}

fn targets(m: &Model, p: &PoseFrame) -> Option<Targets> {
    let (s, c) = p.facing.sin_cos();
    // WoW world (relative to root) -> model space (x forward, y left, z up)
    let to_model = |v: [f32; 3]| [c * v[0] + s * v[1], -s * v[0] + c * v[1], v[2]];
    let q = |slot: usize| to_model(p.parts[slot].pos);
    let rest = |b: usize| m.pivot[b];

    // size ratio from the leg lengths (ankle-knee-hip)
    let wow_leg = len(sub(rest(THIGH[0]), rest(SHIN[0][1]))) + len(sub(rest(SHIN[0][1]), rest(FOOT[0][0])));
    let sk_leg = len(sub(q(S_UPLEG[0]), q(S_LEG[0]))) + len(sub(q(S_LEG[0]), q(S_FOOT[0])));
    if sk_leg < 0.2 || !sk_leg.is_finite() {
        return None;
    }
    let k = (wow_leg / sk_leg).clamp(0.5, 2.0);
    let qs = |slot: usize| scale(q(slot), k);

    let mut rot: Vec<Option<R3>> = vec![None; NBONES];

    // torso frames: up along the spine, left along the hips / shoulder line
    let pelvis_rest = frame(sub(rest(WAIST), rest(PELVIS)), sub(rest(THIGH[1]), rest(THIGH[0])));
    let pelvis_now = frame(sub(qs(S_SPINE), qs(S_HIPS)), sub(qs(S_UPLEG[1]), qs(S_UPLEG[0])));
    let r_pelvis = mul3(&pelvis_now, &tr3(&pelvis_rest));

    let chest_rest = frame(sub(rest(NECK), rest(WAIST)), sub(rest(ARM[1]), rest(ARM[0])));
    let chest_now = frame(sub(qs(S_NECK1), qs(S_SPINE1)), sub(qs(S_ARM[1]), qs(S_ARM[0])));
    let r_chest = mul3(&chest_now, &tr3(&chest_rest));

    // head: the Skate head's rotation relative to the chest, carried onto the WoW chest
    let rel = {
        let rs = |slot: usize| {
            let qn = p.parts[slot].rot;
            quat_to_r3(qn)
        };
        mul3(&tr3(&rs(S_SPINE3)), &rs(S_HEAD))
    };
    let r_head = mul3(&r_chest, &rel);

    rot[PELVIS] = Some(r_pelvis);
    rot[SPINE_LOW] = Some(blend(&r_pelvis, &r_chest, 0.4));
    rot[WAIST] = Some(blend(&r_pelvis, &r_chest, 0.8));
    rot[CHEST] = Some(r_chest);
    rot[NECK] = Some(blend(&r_chest, &r_head, 0.5));
    rot[NECK2] = Some(blend(&r_chest, &r_head, 0.75));
    rot[HEAD] = Some(r_head);
    for side in 0..2 {
        rot[SHOULDER[side]] = Some(r_chest);
        // upper arm: elbow direction; forearm: wrist direction
        let a = aim(sub(rest(FOREARM[side][1]), rest(ARM[side])), sub(qs(S_FOREARM[side]), qs(S_ARM[side])));
        rot[ARM[side]] = Some(a);
        let f = aim(sub(rest(HAND[side][0]), rest(FOREARM[side][1])), sub(qs(S_HAND[side]), qs(S_FOREARM[side])));
        for b in FOREARM[side] {
            rot[b] = Some(f);
        }
        for b in HAND[side] {
            rot[b] = Some(f);
        }
        // legs
        let t = aim(sub(rest(SHIN[side][1]), rest(THIGH[side])), sub(qs(S_LEG[side]), qs(S_UPLEG[side])));
        rot[THIGH[side]] = Some(t);
        let sh = aim(sub(rest(FOOT[side][0]), rest(SHIN[side][1])), sub(qs(S_FOOT[side]), qs(S_LEG[side])));
        for b in SHIN[side] {
            rot[b] = Some(sh);
        }
        let ft = aim(sub(rest(TOE[side]), rest(FOOT[side][0])), sub(qs(S_TOE[side]), qs(S_FOOT[side])));
        for b in FOOT[side] {
            rot[b] = Some(ft);
        }
        rot[TOE[side]] = Some(ft);
    }
    if let Ok(mut d) = DEBUG.try_lock() {
        *d = format!(
            "k={k:.3} facing={:.3} hips={:?} head={:?} lknee={:?} lfoot={:?} larm={:?} lhand={:?} pelvisrot_diag=({:.2},{:.2},{:.2}) legs wow/sk={wow_leg:.2}/{sk_leg:.2}",
            p.facing, qs(S_HIPS), qs(S_HEAD), qs(S_LEG[1]), qs(S_FOOT[1]), qs(S_ARM[1]), qs(S_HAND[1]),
            r_pelvis[0][0], r_pelvis[1][1], r_pelvis[2][2]
        );
    }
    Some(Targets { rot, joint_pelvis: qs(S_HIPS) })
}

fn quat_to_r3(q: [f32; 4]) -> R3 {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

/// Called from the hook right after the client filled `base` (bone matrices of the local player).
pub unsafe fn apply(base: usize) {
    let now = windows_sys::Win32::System::SystemInformation::GetTickCount64();
    if now.saturating_sub(POSE_AT.load(Ordering::Relaxed)) > 300 {
        return; // no fresh pose: leave the client's own animation
    }
    let (Ok(model), Ok(pose)) = (MODEL.try_lock(), POSE.try_lock()) else { return };
    let (Some(model), Some(pose)) = (model.as_ref(), pose.as_ref()) else { return };
    let Some(t) = targets(model, pose) else { return };

    let mats = core::slice::from_raw_parts_mut(base as *mut Mat, NBONES);
    let w = mats[0];
    if !looks_rigid(&w) || !looks_rigid(&mats[HEAD]) || !looks_rigid(&mats[THIGH[0]]) {
        return; // not a bone array (stale pointer): never write into unknown memory
    }
    let wi = inv_rigid(&w);
    // model-space transforms as the client computed them
    let b_client: Vec<Mat> = mats.iter().map(|m| mul(m, &wi)).collect();
    let mut b_new: Vec<Mat> = b_client.clone();
    for i in 1..NBONES {
        let par = model.parent[i];
        if par < 0 {
            continue;
        }
        let par = if i == SPINE_LOW { PELVIS } else { par as usize }; // the torso hangs off the pelvis
        let pm = &b_new[par];
        if let Some(r) = &t.rot[i] {
            // joint = where this bone's pivot goes under the parent's new transform (pelvis: the pose's hips)
            let joint = if i == PELVIS { t.joint_pelvis } else { point(model.pivot[i], pm) };
            // keep the client's own local twist for bones whose parent is not retargeted? (none here)
            b_new[i] = bone_matrix(r, model.pivot[i], joint);
        } else {
            let l = mul(&b_client[i], &inv_rigid(&b_client[model.parent[i] as usize]));
            b_new[i] = mul(&l, pm);
        }
    }
    for i in 1..NBONES {
        mats[i] = mul(&b_new[i], &w);
    }
    APPLIED.fetch_add(1, Ordering::Relaxed);
    let _ = bone_rot;
}

/// A finite rotation + translation matrix with w = 1 (row-vector layout).
fn looks_rigid(m: &Mat) -> bool {
    if m.iter().any(|v| !v.is_finite()) || m[3] != 0.0 || m[7] != 0.0 || m[11] != 0.0 || (m[15] - 1.0).abs() > 1e-3 {
        return false;
    }
    let n = |a: usize| (m[a] * m[a] + m[a + 1] * m[a + 1] + m[a + 2] * m[a + 2]).sqrt();
    (0.5..2.0).contains(&n(0)) && (0.5..2.0).contains(&n(4)) && (0.5..2.0).contains(&n(8))
}

pub fn point(p: V3, m: &Mat) -> V3 {
    add(
        [p[0] * m[0] + p[1] * m[4] + p[2] * m[8], p[0] * m[1] + p[1] * m[5] + p[2] * m[9], p[0] * m[2] + p[1] * m[6] + p[2] * m[10]],
        [m[12], m[13], m[14]],
    )
}
