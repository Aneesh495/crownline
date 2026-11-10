#!/usr/bin/env python3
import json
from pathlib import Path
import subprocess
import tempfile

from evidence_common import ROOT, digest

engine = ROOT / "target/release/crownline"


def invoke(*arguments):
    return subprocess.run([engine, *arguments], capture_output=True, text=True, timeout=30)


with tempfile.TemporaryDirectory(dir=ROOT / "target") as directory:
    table = Path(directory) / "table"
    assert invoke("solve", table, "2").returncode == 0
    assert invoke("verify-table", table).returncode == 0
    shard = table / "pieces-1.bin"
    inventory = table / "inventory.json"
    content = bytearray(shard.read_bytes())
    meta = json.loads(inventory.read_bytes())
    first = next(i for i in range(0, len(content), 4) if content[i + 3] == 0x40)
    content[first + 3] = 0x80
    shard.write_bytes(content)
    check = invoke("verify-table", table)
    assert check.returncode == 1 and "checksum" in check.stderr
    meta["shards"][0]["sha256"] = digest(shard)
    meta["shards"][0]["win"] -= 1
    meta["shards"][0]["loss"] += 1
    inventory.write_text(json.dumps(meta))
    check = invoke("verify-table", table)
    assert check.returncode == 1 and "incorrect terminal proof" in check.stderr
print("Table verifier rejected corruption and a rehashed false terminal proof")
