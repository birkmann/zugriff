# ZUGRIFF

A round-based tactical 5v5 shooter in Bevy: one player with four bot teammates against five bots, on three maps.
Teams are **KSK** (defenders and rescuers) and **OPFOR** (attackers and hostage takers). The modes are bomb defusal
with an economy, hostage rescue, and deathmatch; every match starts with a short warmup. It runs in the browser
(WebGPU) and natively from one codebase.

All content is our own or openly licensed: maps, names, textures, models, icons and sounds (see Credits). The game
follows the genre's conventions (controls, HUD layout, round rules, the economy) but copies no other game's assets or
names.

## Layout

| Path | What |
|---|---|
| `crates/zugriff_sim` (repo root) | The game, pure Rust with no rendering: `.map` parsing and compiling (collision brushes, render faces, volumes, lightmap charts), movement, weapons (`weapons.rs`: the arsenal's numbers and the inventory), firing and damage (`combat.rs`: penetration, hit zones, armour), grenades, the match (`game.rs`: rounds, economy, bomb, hostages, deathmatch, the event stream), the navigation graph (`nav.rs`) and the bots (`bots.rs`). Bins: `zg-compile`, `zg-balance`. |
| `server/` | The dedicated server (crate `zugriff_server`, bin `zugriff-server`): rooms over WebSockets, the sim authoritative; see Online |
| `game/` | The Bevy 0.19 client (crate `zugriff`): the match session and input, the view and the weapon in hand (`viewmodel.rs`), everyone else's bodies (`actors.rs`), effects, sound (`audio.rs` plays `sfx.rs`'s synthesised bank), HUD, buy menu, scoreboard and radio, menus, sky, options. Built for the web with Trunk (`index.html`, `Trunk.toml`) and natively with cargo. |
| `assets/` | Game content: `maps/*.map` with their baked `*.lightmap.hdr` (and `*.nav` when built), `textures/<set>/{color,normal,orm}.ktx2`, `models/*.glb`, `fonts/`, the menus' pictures in `ui/` (buy-menu icons, the lobby's operators) and `thumbs/` |
| `tools/maps/` | Layout scripts (Python) that write Valve 220 `.map` files; `zmap.py` holds the brush helpers, `zkit.py` the building kit |
| `tools/lightmap/bake.py` | Bakes a map's lightmap with Blender Cycles (headless `bpy`) |
| `tools/textures/` | `zg-tex`: source images → KTX2 (BC7 / BC5, mips, zstd) |
| `tools/models/` | The weapon and character model pipeline (Blender, headless); `menu_art.py` renders `assets/ui/` from those models |
| `tools/sounds/` | The sound pipeline: `fetch.py` downloads the recordings (Freesound CC0/CC BY by id, Kenney's CC0 packs), `build.py` slices and normalises them into `assets/sounds/` (+ `index.json`, `CREDITS.md`) |
| `build-web.sh`, `prebuilt/` | Optional browser client output (generated, not committed) |

Coordinates: maps are authored in inches with Z up (72 units is a 1.83 m player). The sim and the client use metres
with Y up. The sim ticks at 66 Hz; the client interpolates between ticks.

## Setup (once)

- Stable Rust for desktop builds. For the optional browser build: the `wasm32-unknown-unknown` target, Trunk, and wasm-opt ≥ 133.
- For the tools:
  - Python 3.
  - `uv`, for Blender as a Python module. No Blender install is needed:
    `~/.local/bin/uv run --python 3.11 --with "bpy==4.2.*" …`.

## Commands

```sh
cd game && trunk serve                              # optional browser preview on port 5195
cargo run --profile native -p zugriff                # native
cargo test -p zugriff_sim                            # sim tests, including every map in assets/maps
./build-web.sh                          # the deployed page into prebuilt/

# Maps: generate, compile (+ navigation), bake.
python3 tools/maps/zg_hafen.py
cargo run --release -p zugriff_sim --bin zg-compile -- assets/maps/zg_hafen.map \
    --bake-json /tmp/zg_hafen.bake.json --nav assets/maps/zg_hafen.nav
~/.local/bin/uv run --python 3.11 --with "bpy==4.2.*" python tools/lightmap/bake.py \
    /tmp/zg_hafen.bake.json assets/maps/zg_hafen.lightmap.hdr --samples 256

# Reimport the MiG-29 from a sibling eu-ui checkout (the ready-to-use GLB is bundled).
ZUGRIFF_VEHICLE_SOURCES=../eu-ui/apps/bevy-spike/bevy/assets/models \
    ~/.local/bin/uv run --python 3.11 --with "bpy==4.2.*" python tools/models/arch.py veh_mig29
# Then regenerate zg_fliegerhorst, its navigation and lightmap using the commands above.

# Sachsen: export in Chromium so the hull markings render, retaining deck fittings.
node tools/models/export_sachsen.cjs ../eu-ui /tmp/nexus-vehicles
ZUGRIFF_VEHICLE_SOURCES=/tmp/nexus-vehicles \
    ~/.local/bin/uv run --python 3.11 --with "bpy==4.2.*" python tools/models/arch.py veh_f124
# Then regenerate zg_hafen, navigation and lighting. esbuild/Playwright come from eu-ui.


# Balance: bot-against-bot matches, headless.
cargo run --release -p zugriff_sim --bin zg-balance -- assets/maps/zg_hafen.map --seeds 6 --rounds 16 [--why]

cargo run --release -p zg_tex -- color  <albedo.jpg>  assets/textures/<set>/color.ktx2
cargo run --release -p zg_tex -- normal <normalGL.jpg> assets/textures/<set>/normal.ktx2
cargo run --release -p zg_tex -- orm <ao.jpg|-> <roughness.jpg> <metal.jpg|-> assets/textures/<set>/orm.ktx2
cargo run --release -p zg_tex -- rgba <decal.png> <out.ktx2>        # BC7 sRGB with straight alpha (the decal atlas)

python3 tools/textures/decals.py                        # the decal atlas + decals.json

# Sounds: download the recordings, then cut them into assets/sounds (rebuild the game after: it embeds index.json).
python3 tools/sounds/fetch.py && python3 tools/sounds/build.py
```

Launch options (`?name=value` in the address on the web, `--name value` natively):

| Option | What |
|---|---|
| `play=<map>` | Start a match on that map straight away (no menu). With `team=ksk|opfor`, `mode=dm`, `warmup=<s>`, `rounds=<n>` (match length; 2 ends quickly, for the end screen). |
| `menu=options\|controls\|credits\|pause\|buy` | Open that page for screenshots: the first three from the lobby; `pause` once a `play=` match is live; `buy` while it can buy, kept open without a captured mouse, with `cursor=x,y` (UI pixels at 1080p) to show an item's details. |
| `--clicks "500,250@5;180,640@9"` | Native QA: click menu positions at the given seconds (coordinates in the 1080-high UI design space). Combine with `--shot` to capture the result; this exercises UI hit testing. |
| `autoplay` | A bot plays for the local player (natively a bare `--autoplay`); for watching and testing. |
| `map=<map>` + `pose=x,y,z,yaw,pitch` | A still view of a map without menu (metres, degrees), for screenshots. |
| `noactors`, `nobots`, `noshadow`, `nohud`, `nofx` | Performance bisection: leave out the bodies, the bots' thinking, sun shadows, the HUD, the effects. |
| `perf=1\|2\|3` | Open the performance overlay small, medium or large for this run (in game: F3 steps through the sizes and off, Shift+F3 back). Small and medium sit under the radar, large on the right; over menus, the buy menu and loading only the small line shows, bottom right. |

Map faces use `mat/<set>` for a KTX2 material set and `dev/<name>` for the procedural blockout textures (`dev/water`,
`dev/glass` among them). For every surface, texel coordinates are divided by 512. At texture scale 0.25 a tile spans
128 units (3.25 m), whatever the image's resolution.

**After a map's geometry changes, rebake its lightmap and rebuild its `.nav`.** The atlas layout comes from the
geometry and the bake input carries a hash of it; a stale or missing `.nav` is rebuilt at load (half a second natively,
longer on the web).

## How it plays

- **Rounds:** freeze time 6 s (buy), round 1:55, bomb 40 s, defuse 10 s (5 s with a kit). MR8 or MR15; sides switch
  at half. Money: $800 at the start, $3,250 to $3,500 for a win, a loss bonus growing $1,400 → $3,400, kill rewards by
  weapon class, $300 for the planter and defuser, $800 to the attackers for a plant.
- **Weapons:** 21 guns plus knife, HE, flash, smoke. Damage per round with range falloff, head ×4 / stomach ×1.25 /
  legs ×0.75, armour by each weapon's penetration ratio, the helmet for the head. Spread from stance, movement, jumping
  and the burst; recoil as a punch that climbs and wanders and recovers. Rounds go through thin walls by material.
- **Bots:** they buy by the economy, split over the sites (defenders) or pick one (attackers, sometimes with a lurker),
  rotate when a site is hit, plant, defuse, retake, escort hostages, throw grenades, and obey the radio (Z: follow me,
  hold, regroup, go A/B, storm). Difficulty sets reaction time, aim, recoil control and grenade use.

## Online

The lobby's **Mehrspieler** tab lists the rooms of every known server with their ping, and creates and joins them.

- **Server:** `cargo run --release -p zugriff_server -- [--name "…"] [--room zg_hafen,zg_altstadt:Dauerbetrieb]`
  (port 27015; `--help` lists the options). One process hosts many rooms; a room is one match. Start it separately from the client. Plain HTTP on the same port: `GET /rooms` is the list as JSON.
- **Rooms:** anyone can create one (map, mode, players per side 1–8, bots on or off and their strength, length,
  friendly fire). Public rooms are listed; private ones are joined with their six-character code (shown in the pause
  menu, `/code` in chat), and either may have a password. The creator runs the room with chat commands: `/map`,
  `/restart`, `/kick`, `/bots an|aus`, `/stärke 0–3`, `/runden`; `/hilfe` lists them. A room closes 45 s after the last
  player leaves (`--room` rooms never close, and cycle their maps).
- **Slots:** a room has its players per side from the start. Bots play the slots no one plays; joining takes a slot
  over (the bot's body, mid-round), leaving hands it back. Without bots, free slots stay empty. Pause menu → *Seite
  wechseln* changes sides.
- **Server list across servers:** the list asks the default server (`?server=`/`--server`, else the page's host,
  else this machine), the servers added by address, and the servers announced to them: start a server with
  `--master ws://master:27015/zg --public ws://my-host:27015/zg` and it shows up in the master's list.
- **How it plays:** the server ticks at 66 Hz and sends each client a snapshot per tick (the state as a delta against
  the previous one, packed: typically a few hundred bytes). Clients draw everyone else from a small buffer of
  snapshots and predict themselves (their own shots, steps and reloads play at once; the spread and recoil are seeded
  by the command, so the prediction matches). The server judges shots against what the shooter saw (lag compensation,
  up to one second back: `crates/zugriff_sim/src/lagcomp.rs`). Chat: Y to everyone, U to your side; Z/X radio goes to
  the room's bots.
- **A static browser deployment** has no game server of its own: run one and add its address in the list,
  or open the page with `?server=wss://…`. From an https page the browser only allows `wss://`, so put the server
  behind a TLS proxy (for example Caddy: `reverse_proxy /zg localhost:27015`).
- **Testing:** `--join <id|code>` or `--create <map>` (`?join=`, `?create=`) go straight into a room; with
  `--autoplay` a bot plays your slot. Two native clients and a server make a full online round on one machine.

## QA

- **Native screenshots** (the reliable automated loop): `DISPLAY=:0 target/native/zugriff --play zg_hafen --autoplay
  --warmup 60 --shot out.png --wait 10 --shots 8 --every 2` (`out_1.png` …), or a still map view with `--map … --pose …`.
  It waits for the level and for shader compilation, saves the images and exits.
- **Web:**
  - Run a headful Chromium with `--enable-unsafe-webgpu` against the served build.
  - `?play=zg_hafen&autoplay=1&warmup=60` shows a match without input.
  - The page captures the mouse on click only when the game says so (`data-lock` on the body), so menus stay usable.
  - Audio starts on the first click or key, as browsers require.
- **Balance:** `zg-balance` above: round wins per side should stay near 50/50, plants in about half the rounds.
  `--why` prints what the bots were doing when a round ran out or the bomb went off. It also lists snags (a bot
  pushing to move for 2.5 s but covering less than 0.6 m) by spot, and names anyone pressed against the bot; `--snags N`
  lists more spots.
- **Map checks** (`cargo run --release -p zugriff_sim --example <name> -- <map.map> …`):
  - `holes`: voids beside walkable ground (a player could fall out of the map);
  - `reach`: whether bots can walk from each spawn to the sites, hostages and rescue zone;
  - `route_dump <from x,y,z> <to x,y,z>`: the route's nodes and links (a jump where a walk was meant shows here);
  - `trace_bot <ksk|opfor> <seconds>`: one bot's position, goal and next waypoints over a round;
  - `snag_probe x,y,z x,y,z …`: walks a player from a spot through waypoints as a bot steers (is it the map, or the bots?).
- **Difficulty:** `cargo run --release -p zugriff_sim --example aim_duel -- assets/maps/dev_arena.map
  <easy|normal|hard|expert> 2400 [strafe] [pistol]` has one bot hunt a dummy that doesn't shoot back. It prints the
  reaction time, the first hit, the kill time and the hit rate. With a G36 against a standing target at about 9 m, the
  kill takes about 4.0 s on Easy, 2.7 s on Normal, 1.3 s on Hard and 0.7 s on Expert, and the hit rate is 21, 43, 54 and
  59 %.

## Credits

- Fonts: Barlow Condensed (The Barlow Project Authors) and Inter (The Inter Project Authors), SIL Open Font License
  1.1. See `assets/fonts/OFL-*.txt`.
- Textures: every material set is from ambientCG or Poly Haven, CC0 1.0. See `assets/textures/CREDITS.md` for each
  set's source and any tint.
- Models: Sketchfab models under CC BY 4.0 and Quaternius' animations (CC0). See `assets/models/CREDITS.md`.
- Sounds: recordings with per-source attribution in `assets/sounds/CREDITS.md`, plus synthesised fallback effects in `game/src/sfx.rs`.
