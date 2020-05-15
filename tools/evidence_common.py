"""Shared encoding and size contracts for Crownline campaign artifacts."""

from hashlib import sha256
import json
import os
from pathlib import Path
import re
import subprocess
import time

SCHEMA = "crownline-campaign-v1"
ROOT = Path(__file__).resolve().parents[1]
PUBKEY = ROOT / "config/evidence-ed25519.pub"
EXCLUDED = {"MANIFEST.json", "MANIFEST.sig"}
MODEL_SEEDS = (101, 211, 307)
VARIANTS = ("linear", "network", "blend")
REF_COMMIT = "1fa785edbe8003445163a63f00f04a4c0b77254b"
FRONTIER_FEN = "B:BK2,K4,K8:W11,15,K22"
WITNESS_FEN = "B:BK2,K8:W11,K22"


def canonical(value):
    return (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=False, allow_nan=False) + "\n").encode()


def save(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    with temporary.open("xb") as file:
        file.write(canonical(value))
        file.flush()
        os.fsync(file.fileno())
    temporary.replace(path)


def load(path, cap=8_000_000):
    if path.stat().st_size > cap:
        raise ValueError(f"JSON exceeds cap: {path.name}")
    return json.loads(path.read_bytes(), parse_constant=lambda s: (_ for _ in ()).throw(ValueError(f"nonfinite {s}")))


def lines(path):
    if path.stat().st_size > 192_000_000:
        raise ValueError(f"ledger exceeds cap: {path.name}")
    with path.open("rb") as stream:
        for line in stream:
            if not line.endswith(b"\n") or len(line) > 65_536:
                raise ValueError(f"invalid ledger boundary: {path.name}")
            yield json.loads(line)


def digest(path):
    with path.open("rb") as stream:
        return sha256_stream(stream)


def sha256_stream(stream):
    hasher = sha256()
    for block in iter(lambda: stream.read(1024 * 1024), b""):
        hasher.update(block)
    return hasher.hexdigest()


def command(args, cwd=ROOT, timeout=45):
    return subprocess.run(args, cwd=cwd, capture_output=True, text=True, check=True, timeout=timeout).stdout.strip()


def source_inventory(checkout):
    result = {}
    for name in subprocess.check_output(["git", "ls-files", "-z"], cwd=checkout).decode().split("\0"):
        if not name:
            continue
        path = checkout / name
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"source file is missing or a symlink: {name}")
        result[name] = digest(path)
    return result


def artifact_inventory(directory):
    result = {}
    for path in sorted(directory.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"symlink artifact: {path}")
        if not path.is_file():
            continue
        name = path.relative_to(directory).as_posix()
        if name in EXCLUDED:
            continue
        result[name] = {"bytes": path.stat().st_size, "sha256": digest(path)}
        if len(result) > 5000 or sum(entry["bytes"] for entry in result.values()) > 500_000_000:
            raise ValueError("evidence bundle exceeds file or byte cap")
    return result


def seal(directory, key, profile, commit):
    manifest = {"schema": SCHEMA, "profile": profile, "commit": commit, "artifacts": artifact_inventory(directory)}
    save(directory / "MANIFEST.json", manifest)
    subprocess.run([
        "openssl", "pkeyutl", "-sign", "-rawin", "-inkey", str(key),
        "-in", str(directory / "MANIFEST.json"), "-out", str(directory / "MANIFEST.sig"),
    ], check=True, capture_output=True, timeout=10)


def check_seal(directory, commit, pubkey=PUBKEY):
    manifest = load(directory / "MANIFEST.json", 3_000_000)
    if manifest["schema"] != SCHEMA or manifest["commit"] != commit or not re.fullmatch("[a-f0-9]{40}", commit):
        raise ValueError("signed evidence source identity mismatch")
    signed = subprocess.run([
        "openssl", "pkeyutl", "-verify", "-rawin", "-pubin", "-inkey", str(pubkey),
        "-in", str(directory / "MANIFEST.json"), "-sigfile", str(directory / "MANIFEST.sig"),
    ], capture_output=True, timeout=10)
    if signed.returncode:
        raise ValueError("manifest signature does not match the project key")
    if manifest["artifacts"] != artifact_inventory(directory):
        raise ValueError("artifact hash or missing-file mismatch")
    return manifest


def storage(checkout):
    used = int(command(["du", "-sk", str(checkout)]).split()[0]) * 1024
    host = None
    if Path("/usr/sbin/diskutil").exists():
        output = command(["/usr/sbin/diskutil", "apfs", "list"])
        containers = [int(n) for n in re.findall(r"Capacity In Use By Volumes:\s+(\d+) B", output)]
        if containers:
            host = max(containers)
    free = os.statvfs(checkout)
    available = free.f_bavail * free.f_frsize
    if used > 5_000_000_000 or (host is not None and host > 400_000_000_000) or available < 1_000_000_000:
        raise ValueError(f"storage ceiling hit: project {used}, host {host}, available {available}")
    return {"unix_seconds": time.time(), "project_allocated_bytes": used, "apfs_allocated_bytes": host, "available_bytes": available}
