#!/usr/bin/env python3
import gzip
import platform
from pathlib import Path
import re
import sys
import tarfile
import tempfile
import shutil

from evidence_common import ROOT, digest, storage

version = sys.argv[1] if len(sys.argv) == 2 else ""
if not re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9._-]{0,39}", version):
    raise SystemExit("Specify a version identifier")
storage(ROOT)
base = f"crownline-{version}-{platform.system().lower()}-{platform.machine()}"
dist = ROOT / "dist"
dist.mkdir(exist_ok=True)
archive = dist / f"{base}.tar.gz"
tempfile_path = archive.with_suffix(".tmp")

with tempfile.TemporaryDirectory(dir=dist) as temporary:
    folder = Path(temporary) / base
    (folder / "bin").mkdir(parents=True)
    for name in ("crownline", "crownline-lab"):
        shutil.copy2(ROOT / "target/release" / name, folder / "bin" / name)
    for name in ("docs", "examples", "config"):
        shutil.copytree(ROOT / name, folder / name)
    (folder / "web").mkdir()
    shutil.copytree(ROOT / "web/public", folder / "web/public")
    for name in ("server.mjs", "native.mjs"):
        shutil.copy2(ROOT / "web" / name, folder / "web" / name)
    shutil.copy2(ROOT / "README.md", folder)
    launcher = folder / "run-board"
    launcher.write_text(
        '#!/bin/sh\nset -eu\ncd "$(dirname "$0")"\n'
        'CROWNLINE_ENGINE="$PWD/bin/crownline" exec node web/server.mjs\n'
    )
    launcher.chmod(0o755)

    def normalize(item):
        item.uid = item.gid = item.mtime = 0
        item.uname = item.gname = ""
        return item

    with tempfile_path.open("wb") as raw, gzip.GzipFile(fileobj=raw, mtime=0, filename="") as gz:
        with tarfile.open(fileobj=gz, mode="w|") as bundle:
            bundle.add(folder, arcname=base, filter=normalize)
tempfile_path.replace(archive)
checksum = archive.with_suffix(".gz.sha256")
checksum.write_text(f"{digest(archive)}  {archive.name}\n")
print(archive)
print(checksum)
