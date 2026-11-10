#!/usr/bin/env python3
import gzip
import json
import platform
from pathlib import Path
import re
import sys
import subprocess
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
    for name in ("docs", "examples", "config", "src", "tests", "tools", "web"):
        shutil.copytree(ROOT / name, folder / name, ignore=shutil.ignore_patterns("__pycache__"))
    for name in ("README.md", "CONTRIBUTING.md", "Cargo.lock", "Cargo.toml", "Makefile", "rust-toolchain.toml"):
        shutil.copy2(ROOT / name, folder)
    notices = folder / "third-party-notices"
    notices.mkdir()
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--offline", "--format-version", "1",
    ], cwd=ROOT, timeout=30))
    rows = []
    for dep in metadata["packages"]:
        if dep["source"] is None:
            continue
        identity = dep["name"] + "-" + dep["version"]
        source = Path(dep["manifest_path"]).parent
        group = notices / identity
        group.mkdir()
        files = [file for file in source.iterdir() if file.is_file() and file.name.upper().startswith(("LICENSE", "COPYING", "NOTICE"))]
        if not files:
            raise SystemExit(f"No bundled upstream notice found for {identity}")
        for file in files:
            shutil.copy2(file, group / file.name)
        rows.append(f"{identity}  {dep['license']}")
    rust = Path(subprocess.check_output(["rustc", "--print", "sysroot"], cwd=ROOT, text=True, timeout=15).strip())
    rust_notices = rust / "share/doc/rust/licenses"
    if rust_notices.is_dir():
        shutil.copytree(rust_notices, notices / "rust-toolchain")
    (notices / "index.txt").write_text("\n".join(sorted(rows)) + "\n")
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

    with tempfile_path.open("wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", mtime=0, filename="") as gz:
        with tarfile.open(fileobj=gz, mode="w|") as bundle:
            bundle.add(folder, arcname=base, filter=normalize)
tempfile_path.replace(archive)
checksum = archive.with_suffix(".gz.sha256")
checksum.write_text(f"{digest(archive)}  {archive.name}\n")
print(archive)
print(checksum)
