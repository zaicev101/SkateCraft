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
- [ ] E1: read the player's position/facing/map from the client (3.3.5a object manager),
      verify against `.gps`; ground height / ray queries for collision.
- [ ] E2: engine builds and runs with the user's Skate 3 files; a bridge crate in the engine
      fork reads host state, teleports the skater, publishes skater state.
- [ ] E3: puppet movement in the client (write position), camera follow.
- [ ] E4: overlay composite (readback first, then shared texture with depth).
- [ ] E5: collision streaming into Rapier bodies, input routing, polish.
