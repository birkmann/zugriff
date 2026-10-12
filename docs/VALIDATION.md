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
