#!/usr/bin/env python3
"""Downloads the sound sources into .cache/: Freesound sounds (their high-quality previews, which need no account)
by id, with each one's title, author and licence into .cache/fs/<id>.json, and Kenney's CC0 audio packs.

    python3 fetch.py            # everything build.py uses
    python3 fetch.py 404562 …   # just these Freesound ids (to try candidates)
"""
import json, re, sys, time, urllib.error, urllib.request, zipfile, io
from pathlib import Path

HERE = Path(__file__).parent
CACHE = HERE / '.cache'
KENNEY = ['impact-sounds', 'interface-sounds', 'ui-audio', 'rpg-audio']
UA = {'User-Agent': 'Mozilla/5.0 (zugriff sound fetch)'}
LICENCES = {
    'publicdomain/zero/1.0': 'CC0 1.0',
    'licenses/by/4.0': 'CC BY 4.0',
    'licenses/by/3.0': 'CC BY 3.0',
}


def get(url: str) -> bytes:
    for i in range(6):
        try:
            return urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=60).read()
        except urllib.error.HTTPError as e:
            if e.code != 429:
                raise
            time.sleep(10 * (i + 1))
    raise SystemExit(f'rate limited: {url}')


def freesound(sid: int) -> dict:
    meta_path = CACHE / 'fs' / f'{sid}.json'
    audio = CACHE / 'fs' / f'{sid}.ogg'
    if meta_path.exists() and audio.exists():
        return json.loads(meta_path.read_text())
    audio.parent.mkdir(parents=True, exist_ok=True)
    page = get(f'https://freesound.org/s/{sid}/').decode()
    preview = re.search(r'data-ogg="([^"]+)-lq\.ogg"', page)[1]
    title, author = re.search(r'<title>Freesound - (.*) by ([^<]+)</title>', page).groups()
    lic = re.search(r'creativecommons\.org/((?:publicdomain|licenses)/[a-z-]+/[\d.]+)', page)[1]
    if lic not in LICENCES:
        raise SystemExit(f'{sid}: licence {lic} not allowed (no NC/ND/sampling+)')
    audio.write_bytes(get(preview + '-hq.ogg'))
    meta = {'id': sid, 'title': title, 'author': author, 'licence': LICENCES[lic], 'url': f'https://freesound.org/people/{author}/sounds/{sid}/'}
    meta_path.write_text(json.dumps(meta, indent=1))
    time.sleep(1.5)
    return meta


def kenney(pack: str):
    out = CACHE / 'kenney' / pack
    if out.exists():
        return
    page = get(f'https://kenney.nl/assets/{pack}').decode()
    url = re.search(r'https://kenney\.nl/media/pages/assets/[^"]+\.zip', page)[0]
    zipfile.ZipFile(io.BytesIO(get(url))).extractall(out)


if __name__ == '__main__':
    ids = [int(a) for a in sys.argv[1:]]
    if not ids:
        sys.path.insert(0, str(HERE))
        from build import SOUNDS
        ids = sorted({int(s[3:]) for spec in SOUNDS.values() for s, *_ in spec if s.startswith('fs:')})
        for p in KENNEY:
            kenney(p)
    for sid in ids:
        m = freesound(sid)
        print(f"{sid:>7} {m['licence']:10} {m['author'][:18]:18} {m['title'][:60]}")
