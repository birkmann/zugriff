# Texture variants and trim sets

The scripts behind the texture sets added on 2026-09-29, the variants (`*_b`, `*_c`, `concrete_slabs`) and the trim sets
(`glass_window`, `window_frame`, `metal_dark`, `rubber`, `door_steel`, `rollershutter`, `vent_grille`, `sign_plate`,
`lamp_glass`). Sources, tints and tile sizes are listed in `assets/textures/CREDITS.md`.

1. `fetch_ph.py` downloads the Poly Haven sources into `../.cache/ph`. The ambientCG zips go into `../.cache/acg`.
2. `grade.py` tints the colour maps into `../.cache/graded/` and writes `jobs.json`.
3. `gen_proc.py` writes the procedural sets (glass, louvre, lamp).
4. `build.py` runs `zg-tex` for every job.
