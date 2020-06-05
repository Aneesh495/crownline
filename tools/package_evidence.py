#!/usr/bin/env python3
"""Package an intact signed bundle with required external attribution."""

import argparse
import gzip
from pathlib import Path
import subprocess
import tarfile

from evidence_common import ROOT, REF_COMMIT, check_seal, digest, load, storage

parser = argparse.ArgumentParser()
parser.add_argument("directory", type=Path)
parser.add_argument("destination", type=Path)
parser.add_argument("--marcher-source", type=Path)
args = parser.parse_args()
source = load(args.directory / "SOURCE_INPUTS.json")
check_seal(args.directory, source["commit"])
storage(ROOT)
license_path = None
if (args.directory / "identities/marcher-bridge").is_file():
    if args.marcher_source is None:
        raise SystemExit("Provide the pinned Marcher source for its redistribution notice")
    checkout = args.marcher_source
    revision = subprocess.check_output(["git", "-C", str(checkout), "rev-parse", "HEAD"], text=True).strip()
    if revision != REF_COMMIT:
        raise SystemExit("Marcher source identity mismatch")
    subprocess.run(["git", "-C", str(checkout), "diff-index", "--quiet", "HEAD", "--"], check=True)
    license_path = checkout / "LICENSE"


def normalize(item):
    item.uid = item.gid = item.mtime = 0
    item.uname = item.gname = ""
    return item


with args.destination.open("xb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", mtime=0, filename="") as gz:
    with tarfile.open(fileobj=gz, mode="w|") as bundle:
        bundle.add(args.directory, arcname="crownline-evidence", filter=normalize)
        if license_path:
            bundle.add(license_path, arcname="Marcher-LICENSE", filter=normalize)
checksum = args.destination.with_suffix(".gz.sha256")
checksum.write_text(f"{digest(args.destination)}  {args.destination.name}\n")
print(args.destination)
print(checksum)
