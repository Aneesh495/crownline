#!/usr/bin/env python3
"""Replay an extracted preview through the executable and browser API."""

import json
import os
from pathlib import Path
import selectors
import subprocess
import sys
import tarfile
import tempfile
from urllib.request import Request, urlopen

from evidence_common import ROOT, storage

archive = Path(sys.argv[1]).resolve()
storage(ROOT)
dist = ROOT / "dist"
dist.mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(dir=dist) as destination:
    with tarfile.open(archive, "r:gz") as bundle:
        bundle.extractall(destination, filter="data")
    roots = list(Path(destination).iterdir())
    assert len(roots) == 1, "expected one package root"
    root = roots[0]
    perft = subprocess.check_output([root / "bin/crownline", "perft", "8"], text=True, timeout=10)
    assert perft.strip() == "845931"
    assert (root / "third-party-notices/index.txt").is_file()
    env = dict(os.environ, CROWNLINE_PORT="0", CROWNLINE_HOST="127.0.0.1")
    env.pop("CROWNLINE_ORIGIN", None)
    server = subprocess.Popen([root / "run-board"], cwd=root, env=env, stdout=subprocess.PIPE, text=True)
    try:
        with selectors.DefaultSelector() as ready:
            ready.register(server.stdout, selectors.EVENT_READ)
            if not ready.select(timeout=8):
                raise RuntimeError("packaged board did not start")
            url = server.stdout.readline().strip().removeprefix("Crownline at ")
        with urlopen(url, timeout=5) as page:
            assert b"English draughts" in page.read(64 * 1024)
        request = Request(url + "/api/session", data=b'{"mode":"local"}', headers={"Content-Type": "application/json"})
        with urlopen(request, timeout=5) as response:
            session = json.load(response)
        assert session["state"]["side"] == "Black" and len(session["state"]["legal"]) == 7
    finally:
        server.terminate()
        try:
            server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait()
print("Packaged executable and real browser API replay passed")
