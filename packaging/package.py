#!/usr/bin/env python3
"""Build a portable native bundle, with assets and the dedicated server. No third-party Python modules."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import tarfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--skip-build', action='store_true')
    parser.add_argument('--target-dir', type=Path, help='Cargo output directory (otherwise read from cargo metadata)')
    args = parser.parse_args()
    system = {'Linux': 'linux', 'Darwin': 'macos', 'Windows': 'windows'}[platform.system()]
    arch = {'AMD64': 'x86_64', 'aarch64': 'arm64'}.get(platform.machine(), platform.machine())
    if arch not in ('x86_64', 'arm64'):
        parser.error(f'Unsupported architecture: {arch}')
    if not args.skip_build:
        subprocess.run(['cargo', 'build', '--locked', '--profile', 'native', '-p', 'zugriff', '-p', 'zugriff_server'], cwd=ROOT, check=True)
    target = args.target_dir
    if target is None:
        meta = subprocess.check_output(['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'], cwd=ROOT)
        target = Path(json.loads(meta)['target_directory'])
    target = target.resolve()
    name = f'zugriff-{system}-{arch}'
    output = ROOT / 'dist'
    stage = output / name
    if stage.exists():
        shutil.rmtree(stage)
    stage.mkdir(parents=True)
    if system == 'macos':
        app = stage / 'ZUGRIFF.app' / 'Contents'
        binaries, resources = app / 'MacOS', app / 'Resources'
        binaries.mkdir(parents=True)
        resources.mkdir(parents=True)
        with (app / 'Info.plist').open('wb') as f:
            plistlib.dump({'CFBundleName': 'ZUGRIFF', 'CFBundleDisplayName': 'ZUGRIFF',
                          'CFBundleIdentifier': 'com.birkmann.zugriff', 'CFBundleExecutable': 'zugriff',
                          'CFBundlePackageType': 'APPL', 'CFBundleShortVersionString': tomllib.loads((ROOT / 'game/Cargo.toml').read_text())['package']['version'],
                          'NSHighResolutionCapable': True}, f)
    else:
        binaries = resources = stage
    suffix = '.exe' if system == 'windows' else ''
    for binary in ('zugriff', 'zugriff-server'):
        shutil.copy2(target / 'native' / (binary + suffix), binaries / (binary + suffix))
    shutil.copytree(ROOT / 'assets', resources / 'assets')
    for filename in ('README.md', 'THIRD_PARTY.md'):
        shutil.copy2(ROOT / filename, stage / filename)
    shutil.copytree(ROOT / 'docs', stage / 'docs')
    shutil.copytree(ROOT / 'vendor/bevy_pbr-0.19.1', stage / 'licenses/bevy_pbr',
                    ignore=shutil.ignore_patterns('src', 'Cargo*', '.cargo*', 'README.md', 'PATCHED.md'))
    if system == 'macos':
        subprocess.run(['codesign', '--force', '--deep', '--sign', '-', str(stage / 'ZUGRIFF.app')], check=True)
    # Record exactly what is distributed, independent of the machine that compiled it.
    files = sorted(p for p in stage.rglob('*') if p.is_file())
    (stage / 'SHA256SUMS').write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(stage).as_posix()}\n' for p in files))
    if system == 'macos':
        archive = output / f'{name}.dmg'
        subprocess.run(['hdiutil', 'create', '-volname', 'ZUGRIFF', '-srcfolder', str(stage), '-ov', '-format', 'UDZO', str(archive)], check=True)
    elif system == 'windows':
        archive = output / f'{name}.zip'
        with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED, compresslevel=6) as z:
            for path in stage.rglob('*'):
                if path.is_file():
                    z.write(path, path.relative_to(output))
    else:
        archive = output / f'{name}.tar.gz'
        with tarfile.open(archive, 'w:gz') as tar:
            tar.add(stage, arcname=name)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + '.sha256').write_text(f'{digest}  {archive.name}\n')
    print(f'Built {archive} ({archive.stat().st_size / 1024**2:.1f} MiB)')

if __name__ == '__main__':
    main()
