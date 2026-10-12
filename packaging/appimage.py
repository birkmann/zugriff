#!/usr/bin/env python3
"""Wrap the Linux bundle in an AppImage. Run package.py first."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[1]
TOOLS = ROOT / 'packaging/tools'
TOOLS.mkdir(exist_ok=True)

def tool(env, name, url):
    override = os.environ.get(env)
    path = Path(override).resolve() if override else TOOLS / name
    if not path.is_file():
        if override:
            raise SystemExit(f'{env} does not exist: {path}')
        with urlopen(url, timeout=120) as response, path.with_suffix('.tmp').open('wb') as out:
            shutil.copyfileobj(response, out)
        path.with_suffix('.tmp').replace(path)
    if not override:
        path.chmod(0o755)
    return path

def main():
    source = ROOT / 'dist/zugriff-linux-x86_64'
    if not (source / 'zugriff').is_file():
        raise SystemExit('Build the Linux bundle first: python3 packaging/package.py')
    appdir = ROOT / 'dist/ZUGRIFF.AppDir'
    if appdir.exists():
        shutil.rmtree(appdir)
    shutil.copytree(source, appdir)
    shutil.copy2(ROOT / 'linux/zugriff.svg', appdir / 'zugriff.svg')
    (appdir / '.DirIcon').symlink_to('zugriff.svg')
    (appdir / 'zugriff.desktop').write_text('[Desktop Entry]\nType=Application\nName=ZUGRIFF\nComment=Tactical squad shooter\nExec=zugriff\nIcon=zugriff\nCategories=Game;ActionGame;\nTerminal=false\n')
    (appdir / 'AppRun').write_text('''#!/bin/sh
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ "${1:-}" = "--dedicated-server" ]; then
    shift
    exec "$HERE/zugriff-server" "$@"
fi
exec "$HERE/zugriff" "$@"
''')
    (appdir / 'AppRun').chmod(0o755)
    packer = tool('APPIMAGE_TOOL', 'appimagetool-x86_64.AppImage', 'https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage')
    runtime = tool('APPIMAGE_RUNTIME', 'runtime-x86_64', 'https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64')
    archive = ROOT / 'dist/zugriff-linux-x86_64.AppImage'
    env = dict(os.environ, ARCH='x86_64', APPIMAGE_EXTRACT_AND_RUN='1')
    subprocess.run([str(packer), '--no-appstream', '--runtime-file', str(runtime), str(appdir), str(archive)], env=env, check=True)
    digest = hashlib.file_digest(archive.open('rb'), 'sha256').hexdigest()
    archive.with_name(archive.name + '.sha256').write_text(f'{digest}  {archive.name}\n')
    print(f'Built {archive} ({archive.stat().st_size / 1024**2:.1f} MiB)')

if __name__ == '__main__':
    main()
