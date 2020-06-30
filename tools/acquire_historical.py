#!/usr/bin/env python3
"""Acquire the pinned public WCDF replay inputs with byte verification."""
import argparse
from hashlib import sha256
import json
from pathlib import Path
from urllib.request import urlopen

from evidence_common import ROOT, storage

parser = argparse.ArgumentParser()
parser.add_argument("destination", type=Path)
args = parser.parse_args()
manifest = json.loads((ROOT / "config/historical-v1.json").read_bytes())
storage(ROOT)
args.destination.mkdir(parents=True, exist_ok=True)
for source in manifest["sources"]:
    target = args.destination / source["file"]
    if target.exists():
        raw = target.read_bytes()
    else:
        with urlopen(source["url"], timeout=30) as response:
            raw = response.read(source["bytes"] + 1)
    if len(raw) != source["bytes"] or sha256(raw).hexdigest() != source["sha256"]:
        raise SystemExit(f"Historical source identity mismatch: {source['file']}")
    if not target.exists():
        temporary = target.with_suffix(".part")
        temporary.write_bytes(raw)
        temporary.replace(target)
    print(source["file"], len(raw), source["sha256"])
