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
account plan does not support Pages for this private repository. These are account restrictions,
not compiler or site build failures.
