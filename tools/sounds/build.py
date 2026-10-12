#!/usr/bin/env python3
"""Builds the game's sounds (../../assets/sounds/<name>_<n>.ogg, index.json, CREDITS.md) from recordings: Freesound
sounds (CC0 or CC BY, fetched by fetch.py) and Kenney's CC0 packs.

Each sound is a list of variants the game picks between; each variant a slice of a recording:
    (source, start, end, {options})    start/end in seconds (None: the whole file)
    (source, {'onsets': n, 'len': s})  the first n events of a take of many (steps, drops), each s long
Sources: 'fs:<id>' or 'kenney:<pack>/<file without .ogg>'. Options: hp / lp (filter Hz), rate (speed and pitch),
fade (seconds out), far (a distant version: dull, with a slap-back), gain (dB after normalising to -1 dBFS peak).
The leading silence is cut, so a shot is heard the moment it is fired.

    python3 fetch.py && python3 build.py
"""
import json, subprocess
from pathlib import Path

import numpy as np

HERE = Path(__file__).parent
CACHE = HERE / '.cache'
OUT = HERE.parent.parent / 'assets' / 'sounds'
SR = 44100


SOUNDS = {
    # Near shots, one set per kind of gun.
    'shot_pistol': [('fs:427592', 0.08, 1.6, {}), ('fs:385811', 0.0, 1.6, {}), ('fs:731942', 3.66, 5.0, {})],
    'shot_heavy': [('fs:683175', 0.03, 2.2, {}), ('fs:683186', 0.0, 0.9, {}), ('fs:683175', 0.03, 2.2, {'rate': 0.94})],
    'shot_smg': [('fs:417689', 1.18, 2.6, {}), ('fs:427592', 0.08, 1.3, {'rate': 1.07, 'hp': 150}), ('fs:731942', 5.53, 6.8, {'rate': 1.05, 'hp': 150})],
    'shot_rifle': [('fs:404562', 0.0, 2.2, {}), ('fs:427596', 0.03, 2.0, {}), ('fs:421710', 0.0, 2.2, {})],
    # The Kalashnikovs (AK-74, AKM, PKM): their own metallic report.
    'shot_ak': [('fs:855842', 0.0, 1.57, {}), ('fs:855843', 0.0, 2.0, {}), ('fs:855842', 0.0, 1.57, {'rate': 0.96})],
    'shot_sniper': [('fs:514228', 0.0, 2.5, {}), ('fs:514228', 0.0, 2.5, {'rate': 0.95}), ('fs:421710', 0.0, 2.2, {'rate': 0.9})],
    'shot_shotgun': [('fs:427595', 0.02, 0.84, {}), ('fs:660299', 0.0, 2.0, {}), ('fs:405504', 0.03, 2.0, {}), ('fs:473846', 0.0, 0.95, {})],
    # (The machine guns' takes are bursts: each variant is a burst's last shot, with the tail after it.)
    'shot_mg': [('fs:380349', 10.22, 12.2, {}), ('fs:396324', 31.13, 33.0, {}), ('fs:380349', 3.40, 3.84, {})],
    # The bolt worked after a sniper's shot, the pump after a shotgun's.
    'cycle_bolt': [('fs:204204', 0.05, 1.27, {})],
    'cycle_pump': [('fs:387190', 0.02, 0.9, {})],
    'shell_in': [('fs:108793', None, None, {}), ('fs:500293', 0.0, 0.9, {})],
    'dry': [('fs:171211', 1.25, 1.75, {})],
    'draw': [('kenney:rpg-audio/Audio/cloth1', None, None, {}), ('kenney:rpg-audio/Audio/beltHandle1', None, None, {})],
    'zoom': [('fs:210018', None, None, {})],
    # Reloads, in three parts the game spaces out over the reload's time: the magazine out, in, the slide or bolt.
    'reload_pistol_out': [('fs:432139', 0.0, 0.5, {})],
    'reload_pistol_in': [('fs:432139', 0.5, 1.05, {})],
    'reload_pistol_bolt': [('fs:432139', 1.05, 1.97, {})],
    'reload_rifle_out': [('fs:815879', 0.0, 0.5, {})],
    'reload_rifle_in': [('fs:815879', 0.5, 1.3, {})],
    'reload_rifle_bolt': [('fs:815879', 1.3, 1.87, {})],
    'reload_ak_out': [('fs:674742', 0.0, 0.7, {})],
    'reload_ak_in': [('fs:674742', 0.75, 1.3, {})],
    'reload_ak_bolt': [('fs:674742', 1.3, 1.57, {})],
    'reload_smg_out': [('fs:677160', 0.0, 1.0, {})],
    'reload_smg_in': [('fs:677160', 1.5, 2.6, {})],
    'reload_smg_bolt': [('fs:677160', 3.0, 4.0, {})],
    # Footsteps by surface.
    'step_concrete': [(f'kenney:impact-sounds/Audio/footstep_concrete_00{i}', None, None, {}) for i in range(5)],
    'step_metal': [('fs:208101', {'onsets': 5, 'len': 0.4})],
    'step_wood': [(f'kenney:impact-sounds/Audio/footstep_wood_00{i}', None, None, {}) for i in range(5)],
    'step_dirt': [('fs:530589', {'onsets': 5, 'len': 0.35})],
    'step_grass': [(f'kenney:impact-sounds/Audio/footstep_grass_00{i}', None, None, {}) for i in range(5)],
    'step_tile': [('fs:50725', {'onsets': 5, 'len': 0.3})],
    'step_glass': [(f'kenney:impact-sounds/Audio/impactGlass_light_00{i}', None, None, {'gain': -6}) for i in range(4)],
    'step_flesh': [(f'kenney:impact-sounds/Audio/impactSoft_medium_00{i}', None, None, {}) for i in range(4)],
    'land': [('fs:364690', 0.0, 0.8, {}), ('fs:572459', None, None, {})],
    # Bullet impacts by surface.
    'impact_concrete': [('fs:150837', None, None, {}), ('fs:150838', 0.15, 0.86, {}), ('fs:150839', None, None, {}), ('fs:144444', {'onsets': 2, 'len': 0.35})],
    'impact_metal': [('kenney:impact-sounds/Audio/impactMetal_medium_000', None, None, {}), ('kenney:impact-sounds/Audio/impactMetal_medium_001', None, None, {}), ('fs:148840', 0.0, 0.42, {}), ('fs:392975', None, None, {})],
    'impact_wood': [(f'kenney:impact-sounds/Audio/impactWood_heavy_00{i}', None, None, {'hp': 250}) for i in range(3)] + [('kenney:impact-sounds/Audio/impactPlank_medium_000', None, None, {})],
    'impact_dirt': [('fs:319222', 0.25, 0.8, {}), ('fs:319229', 0.3, 0.9, {}), ('fs:319222', 0.8, 1.4, {})],
    'impact_grass': [('fs:319222', 0.25, 0.8, {'lp': 3000}), ('fs:319229', 0.3, 0.9, {'lp': 3000})],
    'impact_tile': [('fs:321478', 0.42, 0.84, {}), ('fs:150839', None, None, {'hp': 400}), ('fs:150837', None, None, {'hp': 400})],
    'impact_glass': [(f'kenney:impact-sounds/Audio/impactGlass_medium_00{i}', None, None, {}) for i in range(3)],
    'impact_flesh': [('fs:423301', {'onsets': 4, 'len': 0.16}), ('fs:660769', None, None, {}), ('fs:511194', 0.0, 0.24, {})],
    'helmet': [('fs:351371', None, None, {}), ('kenney:impact-sounds/Audio/impactMetal_light_000', None, None, {})],
    'hurt': [(f'kenney:impact-sounds/Audio/impactPunch_medium_00{i}', None, None, {}) for i in range(3)],
    # The knife: a slash, and the heavier stab.
    'whoosh': [('fs:389690', 0.4, 1.1, {}), ('kenney:rpg-audio/Audio/knifeSlice', None, None, {})],
    'whoosh_stab': [('fs:471097', 1.6, 2.4, {})],
    'bounce': [('fs:466453', 0.95, None, {}), ('kenney:impact-sounds/Audio/impactTin_medium_000', None, None, {}), ('kenney:impact-sounds/Audio/impactTin_medium_001', None, None, {})],
    'brass': [('fs:210102', {'onsets': 5, 'len': 0.45})],
    # Grenades and the bomb.
    'boom': [('fs:244394', 0.0, 4.0, {})],
    'boom_far': [('fs:372086', 0.0, 4.3, {})],
    'boom_big': [('fs:255111', 0.0, 6.0, {})],
    'boom_big_far': [('fs:149966', 0.0, 5.1, {})],
    'flashbang': [('fs:737326', 0.0, 3.0, {})],
    'hiss': [('fs:551429', 0.5, 5.0, {'fade': 1.0})],
    'beep': [('fs:138108', 0.12, 0.35, {})],
    'keypad': [('fs:840450', 0.25, 2.2, {})],
    'defuse': [('fs:523292', {'onsets': 1, 'len': 0.7}), ('fs:383449', 1.3, 2.2, {})],
    # The interface.
    'click': [('kenney:ui-audio/Audio/click1', None, None, {})],
    'hover': [('kenney:ui-audio/Audio/rollover2', None, None, {})],
    'buy': [('kenney:rpg-audio/Audio/metalLatch', None, None, {})],
    'pickup': [('kenney:rpg-audio/Audio/beltHandle2', None, None, {})],
    'radio': [('fs:524205', None, None, {}), ('fs:760245', None, None, {})],
}

# Distant shots: the near ones, dulled and with a slap-back off the buildings.
for gun in ['pistol', 'heavy', 'smg', 'rifle', 'ak', 'sniper', 'shotgun', 'mg']:
    SOUNDS[f'shot_{gun}_far'] = [(s, a, b, {**o, 'far': True}) for s, a, b, o in SOUNDS[f'shot_{gun}'][:2]]


def path_of(source: str) -> Path:
    kind, rest = source.split(':', 1)
    return CACHE / 'fs' / f'{rest}.ogg' if kind == 'fs' else CACHE / 'kenney' / f'{rest}.ogg'


def decode(p: Path) -> np.ndarray:
    raw = subprocess.run(['ffmpeg', '-v', 'error', '-i', str(p), '-ac', '1', '-ar', str(SR), '-f', 'f32le', '-'], capture_output=True, check=True).stdout
    return np.frombuffer(raw, np.float32).copy()


def onsets(x: np.ndarray, n: int, length: float) -> list[tuple[float, float]]:
    """The first `n` events in a take of many, at least `length` apart."""
    hop = int(SR * 0.005)
    m = len(x) // hop
    e = 20 * np.log10(np.sqrt((x[:m * hop].reshape(m, hop) ** 2).mean(1)) + 1e-9)
    floor, top = np.percentile(e, 15), e.max()
    out = []
    for i in range(3, m):
        t = i * hop / SR
        if e[i] - e[i - 3] > 10 and e[i] > floor + 14 and e[i] > top - 30 and (not out or t - out[-1] >= length):
            out.append(t)
    return [(max(0.0, t - 0.015), t + length) for t in out[:n]]


def filters(opts: dict, length: float) -> str:
    f = []
    if 'rate' in opts:
        f += [f"asetrate={SR * opts['rate']:.0f}", f'aresample={SR}']
    if 'hp' in opts:
        f.append(f"highpass=f={opts['hp']}")
    if 'lp' in opts:
        f.append(f"lowpass=f={opts['lp']}")
    if opts.get('far'):
        f += ['lowpass=f=1600', 'highpass=f=120', 'aecho=0.8:0.6:90|210:0.35|0.2', 'apad=pad_dur=0.3']
        length += 0.3
    fade = opts.get('fade', min(0.12, 0.3 * length))
    f.append(f'afade=t=out:st={max(0.0, length - fade):.3f}:d={fade:.3f}')
    return ','.join(f)


def render(x: np.ndarray, start, end, opts: dict, out: Path):
    a = int((start or 0) * SR)
    b = len(x) if end is None else min(len(x), int(end * SR))
    y = x[a:b]
    # The leading silence off (to the first sample within 40 dB of the peak), with a 2 ms fade in.
    lead = np.argmax(np.abs(y) > np.abs(y).max() * 0.01)
    y = y[max(0, lead - int(0.002 * SR)):].copy()
    ramp = min(len(y), int(0.002 * SR))
    y[:ramp] *= np.linspace(0, 1, ramp)
    length = len(y) / SR / opts.get('rate', 1.0)
    proc = subprocess.run(['ffmpeg', '-v', 'error', '-f', 'f32le', '-ar', str(SR), '-ac', '1', '-i', '-', '-af', filters(opts, length), '-f', 'f32le', '-'], input=y.tobytes(), capture_output=True, check=True).stdout
    y = np.frombuffer(proc, np.float32)
    y = y * (10 ** ((-1.0 + opts.get('gain', 0)) / 20) / max(np.abs(y).max(), 1e-6))
    subprocess.run(['ffmpeg', '-v', 'error', '-y', '-f', 'f32le', '-ar', str(SR), '-ac', '1', '-i', '-', '-c:a', 'libvorbis', '-q:a', '6', str(out)], input=y.astype(np.float32).tobytes(), check=True)


def credits(used: dict[str, set[str]]) -> str:
    rows = []
    for source, names in sorted(used.items(), key=lambda kv: kv[0]):
        kind, rest = source.split(':', 1)
        if kind == 'fs':
            m = json.loads((CACHE / 'fs' / f'{rest}.json').read_text())
            rows.append(f"| [{m['title']}]({m['url']}) | {m['author']} | {m['licence']} | {', '.join(sorted(names))} |")
    kenney = sorted({n for s, names in used.items() if s.startswith('kenney:') for n in names})
    packs = sorted({s.split(':', 1)[1].split('/')[0] for s in used if s.startswith('kenney:')})
    return '\n'.join([
        '# Sounds: credits',
        '',
        'Built by `tools/sounds/build.py` from these recordings (sliced, filtered, normalised; the distant shots are the',
        'near ones dulled, with an echo added). Each is used under the licence shown; CC0 needs no attribution, but the',
        'authors are named all the same.',
        '',
        '| Recording | Author | Licence | Used in |',
        '|---|---|---|---|',
        *rows,
        f"| Kenney's {', '.join(packs)} packs (kenney.nl) | Kenney | CC0 1.0 | {', '.join(kenney)} |",
        '',
    ])


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob('*.ogg'):
        old.unlink()
    index, used, cache = {}, {}, {}
    for name, spec in SOUNDS.items():
        n = 0
        for item in spec:
            source = item[0]
            x = cache.setdefault(source, None)
            if x is None:
                x = cache[source] = decode(path_of(source))
            if isinstance(item[1], dict):
                slices = [(a, b, {}) for a, b in onsets(x, item[1]['onsets'], item[1]['len'])]
            else:
                slices = [(item[1], item[2], item[3])]
            for a, b, opts in slices:
                render(x, a, b, opts, OUT / f'{name}_{n}.ogg')
                n += 1
            used.setdefault(source, set()).add(name)
        index[name] = n
        print(f'{name:22} {n}')
    (OUT / 'index.json').write_text(json.dumps(index, indent=1) + '\n')
    (OUT / 'CREDITS.md').write_text(credits(used))


if __name__ == '__main__':
    main()
