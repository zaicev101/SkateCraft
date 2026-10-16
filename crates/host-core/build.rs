// The board's wood texture is extracted from the player's own client (tools/extract-deck-texture.py) and
// never committed; without it a plain brown placeholder is built in.
use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=assets/deck.rgba");
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("deck.rgba");
    if fs::copy("assets/deck.rgba", &out).is_err() {
        let (w, h) = (16u32, 16u32);
        let mut data = Vec::new();
        data.extend_from_slice(&w.to_le_bytes());
        data.extend_from_slice(&h.to_le_bytes());
        for _ in 0..w * h {
            data.extend_from_slice(&[0x80, 0x50, 0x28, 0xFF]);
        }
        fs::write(&out, data).unwrap();
    }
}
