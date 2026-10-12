# Standalone extraction checks

Validated locally on Linux on 2026-10-12:

- 73 simulation unit tests and the integration check compiling all bundled maps passed.
- The native client and dedicated server compiled with the retained Bevy renderer patch.
- A packaged client launched from `/tmp`, loaded Altstadt, rendered gameplay, and saved a screenshot.
- A packaged server launched from a temporary working directory and served a persistent Hafen room.
- Static website references, release asset filtering, desktop/mobile overflow, FAQ interactions,
  private download messaging, empty releases, available assets, and API failure states were checked.

Windows and macOS packages are configured in GitHub Actions; they have not been compiled or run
locally. Each release job runs simulation tests and a packaged server smoke test. The GUI needs
platform-specific playtesting. GitHub Pages needs to be enabled by a repository administrator.

Release 0.1.1 adds a Linux AppImage, verified with `--appimage-extract-and-run --version`.
The GitHub failures were diagnosed through authenticated API access: jobs never started because
an Actions budget prevented runner use. Enabling Pages returned HTTP 422 because the current
account plan does not support Pages for this private repository. These were account restrictions,
not compiler or site build failures. The owner subsequently authorized making the repository
public so Pages and public-repository runners can be used.

## MiG-29 hangar update (0.1.2)

- Imported the Nexus MiG-29 from `eu-ui` through `arch.py`: 65,000 triangles,
  baked materials, a second UV set for lighting, and a fuselage collision hull.
- Regenerated Fliegerhorst geometry, navigation, and its Cycles lightmap after
  adding roof ribs, strip lights, maintenance stations, and floor markings.
- All 73 simulation tests and the bundled-map compilation check passed again.
- The Linux client rendered the updated hangar using the bundled MiG and lighting.
  `website/assets/hangar.webp` is a capture from this build.

## Sachsen and Hafen docks (0.1.3)

- Exported F219 Sachsen from the `eu-ui` model code in Chromium. Unlike the older
  stored GLB, the export includes rendered hull markings and deck fittings.
  Running the checked-in export tool reproduced the same GLB checksum.
- Retained separate hull, deck, glass, and marking materials; corrected hull
  shading and packed lighting UVs; rebuilt Hafen navigation and baked lighting.
- Added mooring lines, quay services, cargo, gangway rails and crane detailing.
  Harbour ripples now fade fine detail with distance to reduce shimmering.
- All 73 simulation tests and the bundled-map integration check passed. The 0.1.3
  Linux bundle smoke test passed; the native renderer loaded the final water shader
  and ship materials without errors. The Hafen website image is a fresh 1080p capture.

## Map selection (0.1.4)

- Solo and multiplayer setup now share a two-column map grid. Each card shows a
  complete 16:9 preview, separate map/objective labels, and an amber selected badge.
- Refreshed all four map previews at 1280 × 720, including Sachsen at Hafen and
  a new training-courtyard capture. Loading screens use the same aspect ratio.
- Inspected native captures at 1920 × 1080 and 1280 × 720. Scripted mouse input
  selected all four solo maps through image/caption hit areas; the load log
  confirmed each map. Multiplayer selection updated its badge and room summary.
- Client and server compiled. The Linux 0.1.4 bundle smoke check passed.
- Refreshed the website/README menu capture. Static asset checks and the eight-image
  gallery check passed, including viewer navigation and desktop/mobile layouts.

## Original map-list layout restored (0.1.5)

- Restored the original narrow vertical map list, overlaid titles/objectives,
  selected-card accent, and operator spacing in solo and multiplayer setup.
- Kept the refreshed map captures and their 16:9 proportions. This supersedes
  the two-column layout introduced in 0.1.4.
- The native build passed; scripted clicks loaded all four maps. The website and
  README menu image were refreshed from the restored layout.
