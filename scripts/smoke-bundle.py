#!/usr/bin/env python3
"""Launch packaged executables from an unrelated working directory and check the server maps."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
from urllib.request import urlopen

bundle = Path(sys.argv[1]).resolve()
app = bundle / 'ZUGRIFF.app/Contents'
base = app / 'MacOS' if app.exists() else bundle
assets = app / 'Resources/assets' if app.exists() else bundle / 'assets'
suffix = '.exe' if os.name == 'nt' else ''
# Force lookup to the distribution; no successful fallback into the source checkout can mask missing files.
env = dict(os.environ, ZUGRIFF_ASSETS=str(assets))
with tempfile.TemporaryDirectory(prefix='zugriff-smoke-') as cwd:
    subprocess.run([str(base / ('zugriff' + suffix)), '--version'], cwd=cwd, env=env, check=True, timeout=20)
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    with tempfile.TemporaryFile() as log:
        server = subprocess.Popen([str(base / ('zugriff-server' + suffix)), '--port', str(port), '--room', 'zg_hafen:Smoke'], cwd=cwd, env=env, stdout=log, stderr=log)
        try:
            for attempt in range(100):
                if server.poll() is not None: raise RuntimeError('Server exited early')
                try:
                    with urlopen(f'http://127.0.0.1:{port}/rooms', timeout=1) as response:
                        rooms = json.load(response)
                    assert 'zg_hafen' in json.dumps(rooms), rooms
                    print('Packaged server loaded Hafen and served its room from outside the checkout.')
                    break
                except OSError:
                    time.sleep(.1)
            else: raise RuntimeError('Server never became ready')
        except Exception:
            log.seek(0)
            print(log.read().decode(errors='replace'))
            raise
        finally:
            server.terminate()
            try: server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
