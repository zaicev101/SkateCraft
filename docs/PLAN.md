# SkateCraft plan and findings

## Approach

Two-process host + guest passthrough (as in SkyCraft: Minecraft inside Skyrim):

- The guest owns the player's movement. The host's player becomes a "puppet" that is moved to
  the guest's position each frame.
- The host streams collision (ground and buildings near the player) to the guest.
- The guest renders only its own content (skater, board) into a texture; the host composites
  it into its frame, with a depth test so the WoW world can occlude it.
- Camera: the host camera follows the guest's view.

Guest = `SK8-ENGINE/skate-3-rust-engine` (Rust + Bevy, GPL-3.0). It has a Lua SDK with Rapier
bodies (static solids, trimeshes), `sdk.player.teleport`, camera capture and graphics meshes,
which is the extension point for streamed WoW collision. The alternative guest is the
recompilation `mchughalex/skate3recomp` (no extension API, license not stated).

## Findings

- Wow.exe 3.3.5a (12340) imports `VERSION.dll` and `DINPUT8.dll` among others. A `version.dll`
  next to the exe is NOT used on this Windows 11 build (the system copy is already mapped, and
  the loader matches imports by name). `dinput8.dll` works: Wow.exe imports exactly one
  function from it, `DirectInput8Create`.
- The plugin is 32-bit (i686) with a statically linked CRT so it needs no runtime next to the exe.

## Milestones

- [x] E0: plugin loads in the client, shared region, heartbeat, fake guest sees it.
- [x] E1a: player position/facing/map read from the client object manager; verified against `.gps`
      (Ragefire Chasm, map 389, x/y/z/facing identical to 6 digits). Offsets in `wow.rs`.
- [ ] E1b: collision for the guest. Decision: take it from the server data (maps/ heightmaps first, then vmaps for
      buildings), loaded by the guest bridge, instead of reverse engineering the client's own trace
      functions. The host only needs to publish position and map id. Data copied to `data/maps` (not committed).
- [~] E2: engine builds and runs with the user's Skate 3 files (DONE: assets converted headless with tools/run-engine-setup.ps1, 0 warnings; engine runs on University, physics ticking; TODO: DualSense check); a bridge crate in the engine
      fork reads host state, teleports the skater, publishes skater state.
- [ ] E3: puppet movement in the client (write position), camera follow.
- [ ] E4: overlay composite (readback first, then shared texture with depth).
- [ ] E5: collision streaming into Rapier bodies, input routing, polish.
