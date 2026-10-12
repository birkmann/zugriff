# bevy_pbr 0.19.1, patched for eu-ui

A copy of `bevy_pbr` 0.19.1 from crates.io, used through `[patch.crates-io]` in the workspace's `Cargo.toml`. The only
change is marked `[eu-ui patch]` in `src/lightmap/mod.rs`.

- **Upstream:** the lightmap allocator gives every lightmapped entity a slot of its own, even when many entities share
  one lightmap image. ZUGRIFF's maps draw about 1 400 lightmapped meshes (level, props, grass and litter cells) from a
  single atlas. That made about 350 slabs (1 400 on WebGPU, which has no binding arrays), a lightmap bind group per
  slab per render phase every frame (3 ms of the render thread), and no batching across slabs.
- **Here:** entities that share an image share its slot (reference-counted).

To drop the patch: remove the `[patch.crates-io]` entry and this folder once upstream shares lightmap slots per image.
To move to a new Bevy version, copy the new `bevy_pbr` here and carry the marked change over.
