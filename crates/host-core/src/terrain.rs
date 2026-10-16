//! Ground height from the server's `maps/*.map` files (AzerothCore GridMap format).
//!
//! The same files the server uses for `GroundZ`, so the numbers can be checked against `.gps`.
//! Only terrain: buildings and objects come from vmaps (separate, later).

use std::collections::HashMap;
use std::sync::Mutex;

const MAP_MAGIC: u32 = u32::from_le_bytes(*b"MAPS");
const HEIGHT_MAGIC: u32 = u32::from_le_bytes(*b"MHGT");
const NO_HEIGHT: u32 = 0x0001;
const AS_INT16: u32 = 0x0002;
const AS_INT8: u32 = 0x0004;
const SIZE_OF_GRIDS: f32 = 533.333_33;
const RES: usize = 128;

struct Tile {
    flat: Option<f32>,
    v9: Vec<f32>, // 129 x 129
    v8: Vec<f32>, // 128 x 128
}

static CACHE: Mutex<Option<HashMap<(u32, i32, i32), Option<Tile>>>> = Mutex::new(None);

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn f32_at(b: &[u8], o: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn load(dir: &str, map: u32, gx: i32, gy: i32) -> Option<Tile> {
    let path = format!("{dir}\\maps\\{map:03}{gx:02}{gy:02}.map");
    let b = std::fs::read(path).ok()?;
    if u32_at(&b, 0)? != MAP_MAGIC {
        return None;
    }
    // header: mapMagic, versionMagic, buildMagic, areaMapOffset, areaMapSize, heightMapOffset, ...
    let off = u32_at(&b, 20)? as usize;
    if u32_at(&b, off)? != HEIGHT_MAGIC {
        return None;
    }
    let flags = u32_at(&b, off + 4)?;
    let grid_height = f32_at(&b, off + 8)?;
    let grid_max = f32_at(&b, off + 12)?;
    if flags & NO_HEIGHT != 0 {
        return Some(Tile { flat: Some(grid_height), v9: vec![], v8: vec![] });
    }
    let n9 = 129 * 129;
    let n8 = RES * RES;
    let mut p = off + 16;
    let mut read = |count: usize| -> Option<Vec<f32>> {
        let mut out = Vec::with_capacity(count);
        if flags & AS_INT16 != 0 {
            let mul = (grid_max - grid_height) / 65535.0;
            for i in 0..count {
                let v = u16::from_le_bytes(b.get(p + i * 2..p + i * 2 + 2)?.try_into().ok()?);
                out.push(grid_height + v as f32 * mul);
            }
            p += count * 2;
        } else if flags & AS_INT8 != 0 {
            let mul = (grid_max - grid_height) / 255.0;
            for i in 0..count {
                out.push(grid_height + *b.get(p + i)? as f32 * mul);
            }
            p += count;
        } else {
            for i in 0..count {
                out.push(f32_at(&b, p + i * 4)?);
            }
            p += count * 4;
        }
        Some(out)
    };
    let v9 = read(n9)?;
    let v8 = read(n8)?;
    Some(Tile { flat: None, v9, v8 })
}

/// Terrain height at a WoW position, or `None` when there is no map tile there (indoors, or
/// a map that has no heightmap).
pub fn height(dir: &str, map: u32, x: f32, y: f32) -> Option<f32> {
    let fx = 32.0 - x / SIZE_OF_GRIDS;
    let fy = 32.0 - y / SIZE_OF_GRIDS;
    let (gx, gy) = (fx.floor() as i32, fy.floor() as i32);
    if !(0..64).contains(&gx) || !(0..64).contains(&gy) {
        return None;
    }
    let mut guard = CACHE.lock().ok()?;
    let cache = guard.get_or_insert_with(HashMap::new);
    let tile = cache.entry((map, gx, gy)).or_insert_with(|| load(dir, map, gx, gy)).as_ref()?;
    if let Some(h) = tile.flat {
        return Some(h);
    }
    let xr = RES as f32 * fx;
    let yr = RES as f32 * fy;
    let (xi, yi) = (xr as i32, yr as i32);
    let (xf, yf) = (xr - xi as f32, yr - yi as f32);
    let (xi, yi) = ((xi as usize) & (RES - 1), (yi as usize) & (RES - 1));
    let v9 = |a: usize, b: usize| tile.v9[a * 129 + b];
    let h5 = 2.0 * tile.v8[xi * RES + yi];
    let (a, b, c);
    if xf + yf < 1.0 {
        if xf > yf {
            let (h1, h2) = (v9(xi, yi), v9(xi + 1, yi));
            a = h2 - h1;
            b = h5 - h1 - h2;
            c = h1;
        } else {
            let (h1, h3) = (v9(xi, yi), v9(xi, yi + 1));
            a = h5 - h1 - h3;
            b = h3 - h1;
            c = h1;
        }
    } else if xf > yf {
        let (h2, h4) = (v9(xi + 1, yi), v9(xi + 1, yi + 1));
        a = h2 + h4 - h5;
        b = h4 - h2;
        c = h5 - h4;
    } else {
        let (h3, h4) = (v9(xi, yi + 1), v9(xi + 1, yi + 1));
        a = h4 - h3;
        b = h3 + h4 - h5;
        c = h5 - h4;
    }
    Some(a * xf + b * yf + c)
}
