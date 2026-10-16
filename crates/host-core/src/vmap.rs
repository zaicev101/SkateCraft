//! Collision from the server's `vmaps/` (AzerothCore VMAP 4): buildings (WMO) and objects (M2)
//! as triangles in WoW world coordinates, with a floor query and a segment test.
//!
//! Tile files are `NNN_<gy>_<gx>.vmtile` (note the order: y first), each a list of model spawns
//! (model name, position/rotation/scale in the "internal" frame). Models are `<name>.vmo` files
//! with the triangle meshes of their groups.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type V3 = [f32; 3];

/// Internal VMAP frame -> WoW: x = MID - X, y = MID - Y, z = Z.
const MID: f32 = 0.5 * 64.0 * 533.333_33;
const SIZE_OF_GRIDS: f32 = 533.333_33;
const CELL: f32 = 8.0;

struct Model {
    groups: Vec<(Vec<V3>, Vec<[u32; 3]>)>,
}

#[derive(Clone, Copy)]
pub struct Tri {
    pub a: V3,
    pub b: V3,
    pub c: V3,
}

struct Tile {
    tris: Vec<Tri>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn u32(&mut self) -> Option<u32> {
        let v = u32::from_le_bytes(self.b.get(self.p..self.p + 4)?.try_into().ok()?);
        self.p += 4;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        let v = u16::from_le_bytes(self.b.get(self.p..self.p + 2)?.try_into().ok()?);
        self.p += 2;
        Some(v)
    }
    fn f32(&mut self) -> Option<f32> {
        let v = f32::from_le_bytes(self.b.get(self.p..self.p + 4)?.try_into().ok()?);
        self.p += 4;
        Some(v)
    }
    fn v3(&mut self) -> Option<V3> {
        Some([self.f32()?, self.f32()?, self.f32()?])
    }
    fn tag(&mut self, t: &[u8; 4]) -> Option<()> {
        (self.b.get(self.p..self.p + 4)? == t).then(|| self.p += 4)
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        (self.p + n <= self.b.len()).then(|| self.p += n)
    }
}

fn load_model(dir: &str, name: &str) -> Option<Model> {
    let bytes = std::fs::read(format!("{dir}\\vmaps\\{name}")).ok()?;
    let mut r = Reader { b: &bytes, p: 8 }; // "VMAP_4.0"
    r.tag(b"WMOD")?;
    r.u32()?; // chunk size
    r.u32()?; // root WMO id
    r.tag(b"GMOD")?;
    let count = r.u32()? as usize;
    let mut groups = Vec::with_capacity(count);
    for _ in 0..count {
        r.skip(24)?; // bounds
        r.u32()?; // mogp flags
        r.u32()?; // group wmo id
        r.tag(b"VERT")?;
        r.u32()?;
        let nv = r.u32()? as usize;
        let mut verts = Vec::with_capacity(nv);
        for _ in 0..nv {
            verts.push(r.v3()?);
        }
        r.tag(b"TRIM")?;
        r.u32()?;
        let nt = r.u32()? as usize;
        let mut tris = Vec::with_capacity(nt);
        for _ in 0..nt {
            tris.push([r.u32()?, r.u32()?, r.u32()?]);
        }
        // the per-group BIH tree and the liquid block are not needed: skip them
        r.tag(b"MBIH")?;
        // BIH: bounds(24) + treeSize(u32) + tree + count(u32) + objects
        r.skip(24)?;
        let tree = r.u32()? as usize;
        r.skip(tree * 4)?;
        let objs = r.u32()? as usize;
        r.skip(objs * 4)?;
        r.tag(b"LIQU")?;
        let liquid = r.u32()? as usize;
        r.skip(liquid)?;
        groups.push((verts, tris));
    }
    Some(Model { groups })
}

/// G3D Matrix3::fromEulerAnglesZYX(z, y, x) = Rz * Ry * Rx.
fn rotation(rot: V3) -> [[f32; 3]; 3] {
    let (az, ay, ax) = (rot[1].to_radians(), rot[0].to_radians(), rot[2].to_radians());
    let (sz, cz) = az.sin_cos();
    let (sy, cy) = ay.sin_cos();
    let (sx, cx) = ax.sin_cos();
    [
        [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
        [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
        [-sy, cy * sx, cy * cx],
    ]
}

fn to_wow(p: V3) -> V3 {
    [MID - p[0], MID - p[1], p[2]]
}

fn load_tile(dir: &str, map: u32, gx: i32, gy: i32, models: &mut HashMap<String, Option<Arc<Model>>>) -> Option<Tile> {
    let bytes = std::fs::read(format!("{dir}\\vmaps\\{map:03}_{gy:02}_{gx:02}.vmtile")).ok()?;
    let mut r = Reader { b: &bytes, p: 8 };
    let n = r.u32()?;
    let mut tris = Vec::new();
    for _ in 0..n {
        let flags = r.u32()?;
        r.u16()?; // adt id
        r.u32()?; // spawn id
        let pos = r.v3()?;
        let rot = r.v3()?;
        let scale = r.f32()?;
        if flags & 4 != 0 {
            r.skip(24)?; // bounds
        }
        let len = r.u32()? as usize;
        let name = String::from_utf8_lossy(r.b.get(r.p..r.p + len)?).into_owned();
        r.skip(len)?;
        r.u32()?; // referenced node
        let model = models
            .entry(name.clone())
            .or_insert_with(|| {
                let file = if name.ends_with(".vmo") { name.clone() } else { format!("{name}.vmo") };
                load_model(dir, &file).map(Arc::new)
            })
            .clone();
        let Some(model) = model else { continue };
        let m = rotation(rot);
        let xf = |v: V3| -> V3 {
            let s = [v[0] * scale, v[1] * scale, v[2] * scale];
            to_wow([
                pos[0] + m[0][0] * s[0] + m[0][1] * s[1] + m[0][2] * s[2],
                pos[1] + m[1][0] * s[0] + m[1][1] * s[1] + m[1][2] * s[2],
                pos[2] + m[2][0] * s[0] + m[2][1] * s[1] + m[2][2] * s[2],
            ])
        };
        for (verts, idx) in &model.groups {
            let w: Vec<V3> = verts.iter().map(|&v| xf(v)).collect();
            for t in idx {
                let (a, b, c) = (w.get(t[0] as usize), w.get(t[1] as usize), w.get(t[2] as usize));
                if let (Some(&a), Some(&b), Some(&c)) = (a, b, c) {
                    tris.push(Tri { a, b, c });
                }
            }
        }
    }
    let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        let (minx, maxx) = (t.a[0].min(t.b[0]).min(t.c[0]), t.a[0].max(t.b[0]).max(t.c[0]));
        let (miny, maxy) = (t.a[1].min(t.b[1]).min(t.c[1]), t.a[1].max(t.b[1]).max(t.c[1]));
        for cx in (minx / CELL).floor() as i32..=(maxx / CELL).floor() as i32 {
            for cy in (miny / CELL).floor() as i32..=(maxy / CELL).floor() as i32 {
                grid.entry((cx, cy)).or_default().push(i as u32);
            }
        }
    }
    Some(Tile { tris, grid })
}

struct State {
    tiles: HashMap<(u32, i32, i32), Option<Tile>>,
    models: HashMap<String, Option<Arc<Model>>>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_tile<R>(dir: &str, map: u32, x: f32, y: f32, f: impl FnOnce(&Tile) -> R) -> Option<R> {
    let (gx, gy) = ((32.0 - x / SIZE_OF_GRIDS).floor() as i32, (32.0 - y / SIZE_OF_GRIDS).floor() as i32);
    let mut guard = STATE.lock().ok()?;
    let st = guard.get_or_insert_with(|| State { tiles: HashMap::new(), models: HashMap::new() });
    if !st.tiles.contains_key(&(map, gx, gy)) {
        let t = load_tile(dir, map, gx, gy, &mut st.models);
        st.tiles.insert((map, gx, gy), t);
    }
    st.tiles.get(&(map, gx, gy))?.as_ref().map(f)
}

/// z of the triangle above/below (x, y), if the point is inside its XY projection.
fn z_at(t: &Tri, x: f32, y: f32) -> Option<f32> {
    let (ax, ay) = (t.a[0], t.a[1]);
    let (v0x, v0y) = (t.b[0] - ax, t.b[1] - ay);
    let (v1x, v1y) = (t.c[0] - ax, t.c[1] - ay);
    let (v2x, v2y) = (x - ax, y - ay);
    let d = v0x * v1y - v1x * v0y;
    if d.abs() < 1e-6 {
        return None; // vertical wall seen from above
    }
    let u = (v2x * v1y - v1x * v2y) / d;
    let v = (v0x * v2y - v2x * v0y) / d;
    if u < 0.0 || v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some(t.a[2] + u * (t.b[2] - t.a[2]) + v * (t.c[2] - t.a[2]))
}

/// The highest surface at or below `z_ref + step_up` under (x, y), searching at most `depth` down.
pub fn floor(dir: &str, map: u32, x: f32, y: f32, z_ref: f32, step_up: f32, depth: f32) -> Option<f32> {
    with_tile(dir, map, x, y, |tile| {
        let ids = tile.grid.get(&((x / CELL).floor() as i32, (y / CELL).floor() as i32))?;
        let top = z_ref + step_up;
        let mut best: Option<f32> = None;
        for &i in ids {
            if let Some(z) = z_at(&tile.tris[i as usize], x, y) {
                if z <= top && z >= z_ref - depth && best.is_none_or(|b| z > b) {
                    best = Some(z);
                }
            }
        }
        best
    })
    .flatten()
}

/// Number of triangles loaded for the tile under (x, y) (diagnostics).
pub fn tile_triangles(dir: &str, map: u32, x: f32, y: f32) -> Option<usize> {
    with_tile(dir, map, x, y, |t| t.tris.len())
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// First wall (a triangle steeper than ~45 degrees) hit by the segment a -> b: the fraction along
/// the segment and the unit normal (facing the segment's start).
pub fn wall_hit(dir: &str, map: u32, a: V3, b: V3) -> Option<(f32, V3)> {
    with_tile(dir, map, a[0], a[1], |tile| {
        let (minx, maxx) = (a[0].min(b[0]), a[0].max(b[0]));
        let (miny, maxy) = (a[1].min(b[1]), a[1].max(b[1]));
        let d = sub(b, a);
        let mut best: Option<(f32, V3)> = None;
        let mut seen: Vec<u32> = Vec::new();
        for cx in (minx / CELL).floor() as i32..=(maxx / CELL).floor() as i32 {
            for cy in (miny / CELL).floor() as i32..=(maxy / CELL).floor() as i32 {
                let Some(ids) = tile.grid.get(&(cx, cy)) else { continue };
                for &i in ids {
                    if seen.contains(&i) {
                        continue;
                    }
                    seen.push(i);
                    let t = &tile.tris[i as usize];
                    let e1 = sub(t.b, t.a);
                    let e2 = sub(t.c, t.a);
                    let n = cross(e1, e2);
                    let nl = dot(n, n).sqrt();
                    if nl < 1e-6 {
                        continue;
                    }
                    let nz = (n[2] / nl).abs();
                    if nz > 0.7 {
                        continue; // floor-like: handled by the floor query, not a wall
                    }
                    // Moller-Trumbore for a segment
                    let p = cross(d, e2);
                    let det = dot(e1, p);
                    if det.abs() < 1e-8 {
                        continue;
                    }
                    let inv = 1.0 / det;
                    let s = sub(a, t.a);
                    let u = dot(s, p) * inv;
                    if !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    let q = cross(s, e1);
                    let v = dot(d, q) * inv;
                    if v < 0.0 || u + v > 1.0 {
                        continue;
                    }
                    let tt = dot(e2, q) * inv;
                    if (0.0..=1.0).contains(&tt) && best.is_none_or(|b| tt < b.0) {
                        let mut nn = [n[0] / nl, n[1] / nl, n[2] / nl];
                        if dot(nn, d) > 0.0 {
                            nn = [-nn[0], -nn[1], -nn[2]];
                        }
                        best = Some((tt, nn));
                    }
                }
            }
        }
        best
    })
    .flatten()
}

/// All building/object triangles whose 8-yard cells fall inside the square of half-size `r`
/// around (cx, cy), appended to `out` as WoW-space vertex triples.
pub fn collect(dir: &str, map: u32, cx: f32, cy: f32, r: f32, limit: usize, out: &mut Vec<[f32; 9]>) {
    let mut done: Vec<(i32, i32)> = Vec::new();
    for (px, py) in [(cx - r, cy - r), (cx + r, cy - r), (cx - r, cy + r), (cx + r, cy + r)] {
        let key = ((32.0 - px / SIZE_OF_GRIDS).floor() as i32, (32.0 - py / SIZE_OF_GRIDS).floor() as i32);
        if done.contains(&key) {
            continue;
        }
        done.push(key);
        with_tile(dir, map, px, py, |tile| {
            let mut seen = std::collections::HashSet::new();
            for gx in ((cx - r) / CELL).floor() as i32..=((cx + r) / CELL).floor() as i32 {
                for gy in ((cy - r) / CELL).floor() as i32..=((cy + r) / CELL).floor() as i32 {
                    if let Some(ids) = tile.grid.get(&(gx, gy)) {
                        for &i in ids {
                            if out.len() >= limit {
                                return;
                            }
                            if seen.insert(i) {
                                let t = &tile.tris[i as usize];
                                out.push([t.a[0], t.a[1], t.a[2], t.b[0], t.b[1], t.b[2], t.c[0], t.c[1], t.c[2]]);
                            }
                        }
                    }
                }
            }
        });
    }
}
