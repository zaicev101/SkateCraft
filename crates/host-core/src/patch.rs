//! Cuts the collision patch (terrain + buildings/objects) around a point, in WoW coordinates, for
//! the Skate side to build its physics world from. Runs on its own thread so a cold tile load
//! never stalls the puppet loop.

use skatecraft_protocol::{Shared, PATCH_MAX_TRIS};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// True while the builder thread runs (the core waits for it before it is unloaded).
pub static ALIVE: AtomicBool = AtomicBool::new(false);

/// What the worker wants a patch for: (map, x, y). The builder thread cuts it when it changes.
pub static REQUEST: Mutex<Option<(u32, f64, f64)>> = Mutex::new(None);

const RADIUS: f32 = 40.0;
const STEP: f32 = 2.0;

pub fn build(dir: &str, map: u32, cx: f32, cy: f32) -> (Vec<[f32; 9]>, u32) {
    let mut out: Vec<[f32; 9]> = Vec::new();
    // terrain: the heightmap sampled on a coarse grid
    let n = (2.0 * RADIUS / STEP) as usize + 1;
    let mut h: Vec<Option<f32>> = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            h.push(crate::terrain::height(dir, map, cx - RADIUS + i as f32 * STEP, cy - RADIUS + j as f32 * STEP));
        }
    }
    for i in 0..n - 1 {
        for j in 0..n - 1 {
            if let (Some(a), Some(b), Some(c), Some(d)) = (h[i * n + j], h[(i + 1) * n + j], h[(i + 1) * n + j + 1], h[i * n + j + 1]) {
                let (x0, y0) = (cx - RADIUS + i as f32 * STEP, cy - RADIUS + j as f32 * STEP);
                let (x1, y1) = (x0 + STEP, y0 + STEP);
                out.push([x0, y0, a, x1, y0, b, x1, y1, c]);
                out.push([x0, y0, a, x1, y1, c, x0, y1, d]);
            }
        }
    }
    let terrain = out.len() as u32;
    // buildings and objects
    crate::vmap::collect(dir, map, cx, cy, RADIUS, PATCH_MAX_TRIS, &mut out);
    (out, terrain)
}

pub fn spawn_builder(dir: String) {
    ALIVE.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        let Ok(shared) = Shared::create() else {
            ALIVE.store(false, Ordering::SeqCst);
            return;
        };
        let mut done: Option<(u32, f64, f64)> = None;
        let mut serial = 0u32;
        while !crate::STOP.load(Ordering::SeqCst) {
            let want = *REQUEST.lock().unwrap();
            if let Some((map, x, y)) = want {
                if done != Some((map, x, y)) {
                    let t = std::time::Instant::now();
                    let (tris, terrain) = build(&dir, map, x as f32, y as f32);
                    serial = serial.wrapping_add(1).max(1);
                    shared.write_patch(map, x, y, serial, terrain, &tris);
                    crate::log(&format!("patch #{serial}: map {map} at ({x:.1}, {y:.1}), {} triangles, {} ms", tris.len(), t.elapsed().as_millis()));
                    done = Some((map, x, y));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        ALIVE.store(false, Ordering::SeqCst);
    });
}
