# Texture credits

Every material set comes from a CC0 library: [ambientCG](https://ambientcg.com) or
[Poly Haven](https://polyhaven.com/textures). Both release their assets under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/), so no attribution is required. We credit them anyway.

Each set was built from the source's 2K JPG maps with `zg-tex` and scaled down to 1024×1024:
- `color.ktx2` is the colour map, BC7 sRGB.
- `normal.ktx2` is the OpenGL normal map (ambientCG `NormalGL`, Poly Haven `nor_gl`), BC5.
- `orm.ktx2` holds AO, roughness and metalness, BC7 linear. When the source has no AO map it is white, and when it
  has no metalness map it is zero.

The downloads are cached in `tools/textures/.cache/`, which git ignores.

"Tile" is the real-world size one texture repeat shows, where the source states it. To show the texture at true
size, use a map texture scale of about **0.077 × tile in metres**: 1 m → 0.077, 2 m → 0.154, 3 m → 0.23. At the
default 0.25 a repeat spans 3.25 m.

| Set | Source | Licence | Tile | Notes |
|---|---|---|---|---|
| `concrete_wall` | ambientCG [Concrete030](https://ambientcg.com/view?id=Concrete030) | CC0 | – | Smooth grey cast concrete |
| `concrete_floor` | Poly Haven [concrete_floor_worn_001](https://polyhaven.com/a/concrete_floor_worn_001) (Dimitrios Savva, Rico Cilliers) | CC0 | 3 m | Worn grey industrial slab |
| `concrete_painted` | Poly Haven [painted_concrete_02](https://polyhaven.com/a/painted_concrete_02) (Rob Tuytel) | CC0 | 4 m | **Tinted**: per-channel gain so the mean colour goes from (86, 83, 76) to light green-grey (150, 156, 146). Hangar floor paint. |
| `plaster_white` | ambientCG [Plaster002](https://ambientcg.com/view?id=Plaster002) | CC0 | – | No AO map |
| `plaster_ochre` | Poly Haven [yellow_plaster](https://polyhaven.com/a/yellow_plaster) (Rob Tuytel) | CC0 | 2 m | Ochre facade render, colour as in the source |
| `plaster_old` | ambientCG [PaintedPlaster006](https://ambientcg.com/view?id=PaintedPlaster006) | CC0 | – | Off-white plaster with broken patches |
| `bricks090` | ambientCG [Bricks090](https://ambientcg.com/view?id=Bricks090) | CC0 | 1.5 m | The existing set, unchanged |
| `brick_dark` | Poly Haven [red_bricks_04](https://polyhaven.com/a/red_bricks_04) (Rob Tuytel) | CC0 | 2.5 m | Dark red-brown brick with dark mortar (clinker look) |
| `cobblestone` | ambientCG [PavingStones070](https://ambientcg.com/view?id=PavingStones070) | CC0 | 1.15 m | Grey granite setts with moss in the joints |
| `paving` | ambientCG [PavingStones136](https://ambientcg.com/view?id=PavingStones136) | CC0 | ~1.6 m (est., 4 × 4 slabs) | **Tinted**: per-channel gain from beige (162, 151, 115) to neutral grey (150, 150, 146), giving grey sidewalk slabs |
| `asphalt` | ambientCG [Road012A](https://ambientcg.com/view?id=Road012A) | CC0 | – | Dark road or taxiway asphalt, no markings |
| `gravel` | ambientCG [Gravel022](https://ambientcg.com/view?id=Gravel022) | CC0 | 1.5 m | Loose mixed gravel |
| `wood_planks` | ambientCG [Planks033B](https://ambientcg.com/view?id=Planks033B) | CC0 | – | Wooden floor planks |
| `wood_beam` | ambientCG [Wood062](https://ambientcg.com/view?id=Wood062) | CC0 | – | Dark weathered grain with no plank seams, for beams and doors |
| `wood_crate` | ambientCG [Planks021](https://ambientcg.com/view?id=Planks021) | CC0 | 1.4 m | Light raw pine planks |
| `metal_corrugated` | ambientCG [CorrugatedSteel005](https://ambientcg.com/view?id=CorrugatedSteel005) | CC0 | – | Bare galvanised sheet. The ribs are only in the normal map and run along V. |
| `metal_painted_red` | ambientCG [PaintedMetal004](https://ambientcg.com/view?id=PaintedMetal004) | CC0 | – | **Tinted**: HSV value × 0.58 and saturation × 0.95, turning the signal red into container red (mean (143, 41, 41)). Scratches are bare metal. |
| `metal_painted_blue` | ambientCG [PaintedMetal002](https://ambientcg.com/view?id=PaintedMetal002) | CC0 | – | **Tinted**: HSV value × 0.55, turning bright blue into container blue (mean (30, 61, 125)). Scratches are bare metal. |
| `metal_painted_green` | Poly Haven [green_metal_rust](https://polyhaven.com/a/green_metal_rust) (Rob Tuytel) | CC0 | 1 m | Olive-green painted steel with a few rust streaks. The source has no metalness map, so it is 0 (paint). |
| `metal_plate` | ambientCG [DiamondPlate003](https://ambientcg.com/view?id=DiamondPlate003) | CC0 | – | Steel tread plate, no AO map |
| `metal_rust` | ambientCG [Metal022](https://ambientcg.com/view?id=Metal022) | CC0 | – | Rusty steel, no AO map |
| `roof_tiles` | ambientCG [RoofingTiles014A](https://ambientcg.com/view?id=RoofingTiles014A) | CC0 | 2.9 m | Red clay tiles in a beaver-tail style. The opacity map is ignored because it is fully opaque. |
| `tiles_floor` | ambientCG [Tiles141](https://ambientcg.com/view?id=Tiles141) | CC0 | 2 m | Worn beige floor tiles (~33 cm), no AO map |
| `ceiling_tiles` | ambientCG [OfficeCeiling001](https://ambientcg.com/view?id=OfficeCeiling001) | CC0 | 3.85 m | Acoustic ceiling tiles (~64 cm) with no light panels |
| `dirt` | ambientCG [Ground103](https://ambientcg.com/view?id=Ground103) | CC0 | – | Packed brown dirt |
| `grass` | ambientCG [Grass004](https://ambientcg.com/view?id=Grass004) | CC0 | 1.4 m | Dense short grass |
| `concrete_wall_b` | Poly Haven [concrete_wall_008](https://polyhaven.com/a/concrete_wall_008) (Dario Barresi, Charlotte Baglioni) | CC0 | 2.7 m | **Tinted**: saturation × 0.2, then per-channel gain from olive (141, 134, 112) to neutral grey (126, 125, 121). Board-marked cast concrete with tie holes |
| `concrete_wall_c` | Poly Haven [concrete_wall_006](https://polyhaven.com/a/concrete_wall_006) (Dario Barresi, Charlotte Baglioni) | CC0 | 2 m | **Tinted**: saturation × 0.3, gain from (92, 81, 69) to (116, 113, 107). Weathered, rain-stained concrete with tie holes |
| `plaster_b` | Poly Haven [beige_wall_001](https://polyhaven.com/a/beige_wall_001) (Dimitrios Savva, Rico Cilliers) | CC0 | 3 m | **Tinted**: large-scale shading flattened (divided by a wrap-around blur of the luma, σ = 2 % of the width, so a faint edge band no longer shows as a seam), saturation × 0.7, gain from (158, 140, 120) to a beige render (184, 174, 154) |
| `plaster_c` | Poly Haven [grey_plaster_02](https://polyhaven.com/a/grey_plaster_02) (Rob Tuytel) | CC0 | 1 m | **Tinted**: saturation × 0.25, gain from (141, 129, 108) to light grey (168, 167, 162). Slightly dirty grey render with patches |
| `brick_b` | Poly Haven [medieval_red_brick](https://polyhaven.com/a/medieval_red_brick) (Rob Tuytel) | CC0 | 2 m | Red brick with thin courses (30 per repeat), colour as in the source |
| `asphalt_b` | Poly Haven [asphalt_02](https://polyhaven.com/a/asphalt_02) (Rob Tuytel) | CC0 | 3 m | **Tinted**: saturation × 0.5, gain from (91, 90, 85) to (70, 70, 68). Worn asphalt with cracks and sealed patches |
| `concrete_slabs` | Poly Haven [large_square_pattern_01](https://polyhaven.com/a/large_square_pattern_01) (Rob Tuytel) | CC0 | 3 m (3 × 3 slabs of 1 m) | **Tinted**: saturation × 0.4, gain from (124, 120, 108) to neutral mid-grey (125, 125, 121). Concrete ground slabs with dark gritty joints |
| `paving_b` | Poly Haven [rectangular_paving](https://polyhaven.com/a/rectangular_paving) (Dimitrios Savva) | CC0 | 2 m (~20 cm pavers) | **Tinted**: saturation × 0.2, gain from brown (127, 108, 86) to neutral grey (128, 128, 125) |
| `metal_corrugated_b` | Poly Haven [corrugated_iron](https://polyhaven.com/a/corrugated_iron) (Jenelle van Heerden, Dimitrios Savva) | CC0 | 1.12 m | **Tinted**: desaturated, contrast × 1.4, recoloured from galvanised (170, 179, 174) to dull grey-green paint (104, 114, 102). Metalness set to 0 (paint) instead of the source map. 15 ribs per repeat, running along V |
| `container_red`, `container_blue`, `container_green`, `container_grey`, `container_orange` | Poly Haven [corrugated_iron](https://polyhaven.com/a/corrugated_iron) | CC0 | 1.12 m | **Recoloured** from `metal_corrugated_b`'s graded colour into container paint, keeping its wear (`tools/textures/variants/containers.py`); normal and ORM from the source |
| `planks_b` | Poly Haven [wood_planks_grey](https://polyhaven.com/a/wood_planks_grey) (Rob Tuytel) | CC0 | 1.5 m | **Tinted**: gain from (70, 68, 63) to (88, 84, 78). Weathered grey boards, 5 per repeat, running along V |
| `roof_b` | Poly Haven [grey_roof_tiles_02](https://polyhaven.com/a/grey_roof_tiles_02) (Rob Tuytel) | CC0 | 1.5 m | **Tinted**: saturation × 0.4, gain from (120, 122, 120) to anthracite (80, 82, 86). Slate-style tiles, 11 courses per repeat along U (RoofingTiles014A is already `roof_tiles`) |
| `tiles_floor_b` | Poly Haven [floor_tiles_08](https://polyhaven.com/a/floor_tiles_08) (Rob Tuytel) | CC0 | 1.5 m (~30 cm tiles) | **Tinted**: saturation × 0.15, gain from terracotta (135, 117, 100) to mid-grey (128, 128, 125) |
| `dirt_b` | Poly Haven [rocky_trail](https://polyhaven.com/a/rocky_trail) (Amal Kumar) | CC0 | 2 m | **Tinted**: gain from (147, 128, 107) to (136, 120, 100). Stony dirt track (Ground103 is already `dirt`) |
| `window_frame` | ambientCG [Plastic010](https://ambientcg.com/view?id=Plastic010) | CC0 | – | **Tinted**: saturation × 0.2, gain from (163, 166, 171) to (192, 192, 189). White PVC or painted aluminium frame, no AO map |
| `metal_dark` | ambientCG [Metal028](https://ambientcg.com/view?id=Metal028) | CC0 | – | **Tinted**: desaturated, gain from black (29, 33, 32) to dark grey (60, 62, 64). Powder-coated steel: metalness set to 0 instead of the source's 1. No AO map |
| `rubber` | ambientCG [Rubber004](https://ambientcg.com/view?id=Rubber004) | CC0 | 0.75 m | **Tinted**: desaturated, gain from blue-black (42, 43, 50) to (40, 40, 40). Fine rubber grain for tyres and seals, no AO map |
| `door_steel` | Poly Haven [blue_metal_plate](https://polyhaven.com/a/blue_metal_plate) (Rob Tuytel) | CC0 | 2.5 m | **Tinted**: desaturated and recoloured from blue (41, 62, 76) to grey-green (96, 106, 96). Painted steel sheet with faint welded panel lines |
| `rollershutter` | Poly Haven [painted_metal_shutter](https://polyhaven.com/a/painted_metal_shutter) (Dario Barresi, Rico Cilliers, Charlotte Baglioni) | CC0 | 2 m (31 slats, ~6.5 cm) | **Tinted**: saturation × 0.1, gain to (130, 131, 132). Slats run along U |
| `sign_plate` | ambientCG [PaintedMetal012](https://ambientcg.com/view?id=PaintedMetal012) | CC0 | – | **Tinted**: saturation × 0.7, contrast × 0.5, gain from white (214, 212, 211) to off-white enamel (196, 192, 182). The chips are bare metal |
| `vent_grille` | Procedural (NumPy), fine grain from ambientCG [Metal028](https://ambientcg.com/view?id=Metal028) | CC0 | – | 16 louvre blades per repeat, running along U: dark painted steel (74, 76, 78) with near-black gaps, blade normals tilted 38° down, AO darkens the gaps and blade tops. Roughness ~0.55, metalness 0 |
| `glass_window` | Procedural (NumPy, FFT noise, tiles seamlessly) | CC0 | – | Dark blue-grey pane (34, 43, 53) with a soft periodic sky reflection towards (118, 136, 156), faint diagonal sheen, vertical rain streaks and dirt. Flat normal, roughness 0.08 (up to ~0.5 on dirt), metalness 0, no AO |
| `lamp_glass` | Procedural (NumPy) | CC0 | – | Warm-white frosted plastic (226, 222, 208) with faint noise. Flat normal, roughness 0.3, metalness 0, no AO |

The tints were applied with Pillow to the 2K colour map before conversion. The graded images are in
`tools/textures/.cache/graded/`. The normal and ORM maps are untouched.

## Decal atlas (`decals/`)

`decals/color.ktx2` (2048², BC7 sRGB, straight alpha), `decals/normal.ktx2` (BC5) and `decals/decals.json` are built
by `tools/textures/decals.py` (`python3 apps/zugriff/tools/textures/decals.py`), which downloads its sources into
`tools/textures/.cache/decals/`. `decals.json` lists each decal's rect, suggested size, blend mode and surface.

| Decals | Source | Licence | Notes |
|---|---|---|---|
| `leak_water_a` | ambientCG [Leaking001](https://ambientcg.com/view?id=Leaking001) | CC0 | A tall crop, recoloured to a grey-brown multiply stain |
| `leak_window` | ambientCG [Leaking003](https://ambientcg.com/view?id=Leaking003) | CC0 | Recoloured to a multiply stain |
| `leak_rust_pipe` | ambientCG [Leaking006](https://ambientcg.com/view?id=Leaking006) | CC0 | Recoloured to rust |
| `leak_rust_bolts` | Generated, with ambientCG [Leaking012B](https://ambientcg.com/view?id=Leaking012B) drip lines | CC0 | Two rust streaks from bolts |
| `grime_base_a`, `grime_base_b`, `damp_base` | ambientCG [Leaking007](https://ambientcg.com/view?id=Leaking007), [Leaking009](https://ambientcg.com/view?id=Leaking009), [Leaking008](https://ambientcg.com/view?id=Leaking008) | CC0 | Rising streaks at a wall's foot, as multiply decals |
| `moss_base` | ambientCG [Moss002](https://ambientcg.com/view?id=Moss002) | CC0 | The colour map in a generated clump mask |
| `grime_patch_a` | ambientCG [SurfaceImperfections003](https://ambientcg.com/view?id=SurfaceImperfections003) | CC0 | In a generated soft blob |
| `grime_patch_b` | ambientCG [Smear003](https://ambientcg.com/view?id=Smear003) | CC0 | In a generated soft blob |
| `floor_damage` | ambientCG [AsphaltDamage001](https://ambientcg.com/view?id=AsphaltDamage001) | CC0 | Colour, opacity and normal |
| `manhole_cover` | ambientCG [ManholeCover004](https://ambientcg.com/view?id=ManholeCover004) | CC0 | Colour, opacity and normal |
| `footprints_mud` | ambientCG [Footsteps004](https://ambientcg.com/view?id=Footsteps004) | CC0 | The prints come from its displacement, the normal from its normal map |
| `tyre_track` | ambientCG [TireTracks001](https://ambientcg.com/view?id=TireTracks001) | CC0 | The tread lugs come from its displacement, the strip is doubled lengthwise |
| Cracks, oil stains, puddle, skid mark | Generated (numpy / Pillow) | Our own | |
| Signs, stencils, floor markings, posters | Generated (Pillow). Text in Barlow Condensed and Inter (`assets/fonts`, SIL OFL 1.1) | Our own | Every name, company, logo and phone number is made up. The pictograms are drawn after the ISO 7010 / DIN conventions, and no sign is copied. |
| `plaster_red` | Poly Haven [red_plaster_weathered](https://polyhaven.com/a/red_plaster_weathered) (Amal Kumar) | CC0 | 2 m | Look pass (2026-09-29) |
| `stone_rustic` | Poly Haven [rustic_stone_wall](https://polyhaven.com/a/rustic_stone_wall) (Dimitrios Savva) | CC0 | 1.5 m | Look pass (2026-09-29) |
| `stone_rubble` | Poly Haven [old_stone_wall](https://polyhaven.com/a/old_stone_wall) (Charlotte Baglioni) | CC0 | 2 m | Look pass (2026-09-29) |
| `wood_old` | Poly Haven [medieval_wood](https://polyhaven.com/a/medieval_wood) (Rob Tuytel) | CC0 | 2 m | Look pass (2026-09-29) |
| `roof_clay` | Poly Haven [clay_roof_tiles_02](https://polyhaven.com/a/clay_roof_tiles_02) (Amal Kumar) | CC0 | 2.5 m | Look pass (2026-09-29) |
| `cobble_mossy` | Poly Haven [mossy_cobblestone](https://polyhaven.com/a/mossy_cobblestone) (Sơn Nguyễn) | CC0 | 2 m | Look pass (2026-09-29) |
| `stone_pavers` | Poly Haven [stone_pavers](https://polyhaven.com/a/stone_pavers) (Amal Kumar) | CC0 | 2 m | Look pass (2026-09-29) |
| `metal_rusty_paint` | Poly Haven [rusty_painted_metal](https://polyhaven.com/a/rusty_painted_metal) (Amal Kumar) | CC0 | 2.2 m | Look pass (2026-09-29) |
| `metal_corrugated_rust` | Poly Haven [rusty_corrugated_iron](https://polyhaven.com/a/rusty_corrugated_iron) (Charlotte Baglioni) | CC0 | 2 m | Look pass (2026-09-29) |
| `concrete_dirty` | Poly Haven [dirty_concrete](https://polyhaven.com/a/dirty_concrete) (Rob Tuytel) | CC0 | 3 m | Look pass (2026-09-29) |
| `concrete_hangar` | Poly Haven [hangar_concrete_floor](https://polyhaven.com/a/hangar_concrete_floor) (Dimitrios Savva) | CC0 | 2 m | Look pass (2026-09-29) |

`metal_painted_red`, `metal_painted_blue` and `metal_painted_green` are recoloured from ambientCG [PaintedMetal012](https://ambientcg.com/view?id=PaintedMetal012) (CC0) by `variants/painted_metal.py`; the `container_*` sets from Poly Haven box_profile_metal_sheet by `variants/containers.py`.

`plaster_cream`, `plaster_pink`, `plaster_sky` and `plaster_sage` are Poly Haven red_plaster_weathered (Amal Kumar, CC0) recoloured by `variants/plaster_tints.py`.
