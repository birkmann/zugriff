#!/usr/bin/env bash
# Starts ZUGRIFF natively (the launcher's command): builds it first when the code has changed since the last build (a
# notification says so; the first build takes a few minutes, later ones seconds), then runs it from the repository's
# root, where it finds its assets. Arguments go to the game (e.g. --play zg_hafen).
set -uo pipefail
ROOT="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
cd "$ROOT"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
LOG="${XDG_CACHE_HOME:-$HOME/.cache}/zugriff-build.log"
mkdir -p "$(dirname "$LOG")"
notify() { command -v notify-send >/dev/null && notify-send -a ZUGRIFF -i zugriff "ZUGRIFF" "$1"; }
if command -v cargo >/dev/null; then
    # Anything newer than the binary? Then say it's being built (cargo decides what is really out of date).
    BIN="target/native/zugriff"
    if [ ! -x "$BIN" ] || [ -n "$(find game crates/zugriff_sim vendor Cargo.toml Cargo.lock -newer "$BIN" -name '*.rs' -o -newer "$BIN" -name '*.wgsl' -o -newer "$BIN" -name 'Cargo.*' 2>/dev/null | head -1)" ]; then
        notify "Wird gebaut … (der erste Bau dauert einige Minuten)"
    fi
    if ! cargo build --profile native -p zugriff >"$LOG" 2>&1; then
        notify "Bauen fehlgeschlagen – siehe $LOG"
        [ -x "$BIN" ] || exit 1
    fi
fi
exec target/native/zugriff "$@"
