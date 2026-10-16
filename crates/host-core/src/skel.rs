//! Rewrites the local player's bone matrices after the client computed them (see `hook.rs`).
//! Matrices are 4x4 row-major, row-vector convention (`v * M`), 64 bytes per bone, and already
//! include the model-to-view transform `W` (= the root bone 0's matrix, whose own local transform is
//! identity). Model-space bone transforms are therefore `B = M * W^-1`.

pub type Mat = [f32; 16];

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

pub fn point(p: [f32; 3], m: &Mat) -> [f32; 3] {
    [
        p[0] * m[0] + p[1] * m[4] + p[2] * m[8] + m[12],
        p[0] * m[1] + p[1] * m[5] + p[2] * m[9] + m[13],
        p[0] * m[2] + p[1] * m[6] + p[2] * m[10] + m[14],
    ]
}

/// Rotation about the model's up axis (Z), around the point `c`.
pub fn yaw_about(c: [f32; 3], a: f32) -> Mat {
    let (s, co) = a.sin_cos();
    let mut r = [0.0f32; 16];
    r[0] = co;
    r[1] = s;
    r[4] = -s;
    r[5] = co;
    r[10] = 1.0;
    r[15] = 1.0;
    // v' = (v - c) R + c
    r[12] = c[0] - (c[0] * co - c[1] * s);
    r[13] = c[1] - (c[0] * s + c[1] * co);
    r[14] = 0.0;
    r
}

const HEAD: usize = 44;
const HEAD_PIVOT: [f32; 3] = [-0.0046, 0.0009, 1.8476];

pub unsafe fn apply(base: usize) {
    let m = |i: usize| (base + i * 64) as *mut Mat;
    let w = *m(0);
    let wi = inv_rigid(&w);
    let b = mul(&*m(HEAD), &wi);
    let t = windows_sys::Win32::System::SystemInformation::GetTickCount64() as f32 / 1000.0;
    let pc = point(HEAD_PIVOT, &b);
    let x = yaw_about(pc, 0.9 * (t * 2.0).sin());
    *m(HEAD) = mul(&mul(&b, &x), &w);
}
