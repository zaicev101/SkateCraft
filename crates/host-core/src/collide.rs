//! Where the character may stand: WoW geometry decides, whatever the skater's own world says.
//! Walls (WMO/M2 triangles) stop and slide the movement; the floor is the vmap surface when one is
//! within step height, else the terrain.

use crate::{terrain, vmap};

pub const RADIUS: f32 = 0.45;
pub const STEP_UP: f32 = 1.3;
pub const FALL_SPEED: f32 = 14.0; // yards per second when the floor is far below

pub struct Outcome {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub blocked: bool,
}

fn floor_at(dir: &str, map: u32, x: f32, y: f32, z: f32) -> Option<f32> {
    vmap::floor(dir, map, x, y, z, STEP_UP, 8.0).or_else(|| terrain::height(dir, map, x, y))
}

/// Move from (x0, y0, z0) towards (x1, y1) for `dt` seconds.
pub fn step(dir: &str, map: u32, from: (f32, f32, f32), to: (f32, f32), dt: f32) -> Outcome {
    let (mut x, mut y, z0) = (from.0, from.1, from.2);
    let mut blocked = false;
    let (mut tx, mut ty) = to;
    // two passes: the first stops at a wall, the second slides along it
    for _ in 0..2 {
        let (dx, dy) = (tx - x, ty - y);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-4 {
            break;
        }
        let mut hit: Option<(f32, [f32; 3])> = None;
        for h in [0.5f32, 1.2, 1.9] {
            let ext = RADIUS / len; // look a little past the target so the body does not clip
            let b = [tx + dx * ext, ty + dy * ext, z0 + h];
            if let Some((t, n)) = vmap::wall_hit(dir, map, [x, y, z0 + h], b) {
                if hit.is_none_or(|c| t < c.0) {
                    hit = Some((t, n));
                }
            }
        }
        let Some((t, n)) = hit else {
            x = tx;
            y = ty;
            break;
        };
        blocked = true;
        let full = len * (1.0 + RADIUS / len);
        let go = (t * full - RADIUS).max(0.0).min(len);
        x += dx / len * go;
        y += dy / len * go;
        // slide: the part of the remaining move along the wall
        let rem = (len - go).max(0.0);
        let (ux, uy) = (dx / len * rem, dy / len * rem);
        let into = ux * n[0] + uy * n[1];
        tx = x + (ux - into * n[0]) * 0.9;
        ty = y + (uy - into * n[1]) * 0.9;
    }
    // floor
    let z = match floor_at(dir, map, x, y, z0) {
        Some(f) if f >= z0 - 0.05 => f,                       // up to the floor (within step height)
        Some(f) => (z0 - FALL_SPEED * dt).max(f),             // fall towards a lower floor
        None => z0,
    };
    Outcome { x, y, z, blocked }
}
