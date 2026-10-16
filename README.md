# SkateCraft

Skate 3 physics inside the World of Warcraft 3.3.5a client, using the "two games, one picture"
approach: WoW is the **host** (world, server, other players), the Skate 3 engine is the **guest**
(skater, board, trick physics). A plugin in each game talks over shared memory.

Status: early prototype (milestone E0 done). Nothing here is playable yet.

## Layout

| Path | What |
|---|---|
| `crates/protocol` | shared-memory layout (seqlock slots: WoW state, guest state) |
| `crates/host-wow` | the WoW plugin, built as a 32-bit `dinput8.dll` proxy that Wow.exe loads at start |
| `tools/fake-guest` | stand-in guest to test the link without Skate |
| `third_party/` | (not committed) clones of `SK8-ENGINE/skate-3-rust-engine` and `chasmlol/SkyCraft` |
| `docs/PLAN.md` | architecture, milestones, findings |

## Build (Windows)

```
rustup target add i686-pc-windows-msvc
cargo build --release -p skatecraft-host-wow --target i686-pc-windows-msvc   # -> dinput8.dll
cargo build --release -p fake-guest
```

Copy `target/i686-pc-windows-msvc/release/dinput8.dll` next to `Wow.exe`. The log is
`skatecraft.log` in the same folder. Run `target/release/fake-guest.exe` to see the link.

You need your own copy of Skate 3 (Xbox 360 files) for the guest. No game files are included.
License: GPL-3.0-only (the guest builds on the GPL-3.0 Skate 3 Rust engine).
