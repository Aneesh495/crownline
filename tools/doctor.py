#!/usr/bin/env python3
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess

from evidence_common import ROOT, storage


def output(*args):
    try:
        return subprocess.check_output(args, timeout=8, text=True, stderr=subprocess.STDOUT).strip()
    except (OSError, subprocess.SubprocessError):
        return None


def tool(name, flag="--version"):
    return {"path": shutil.which(name), "version": output(name, flag)}


mac = platform.system() == "Darwin"
cpu = output("sysctl", "-n", "machdep.cpu.brand_string") if mac else platform.processor()
gpu = output("system_profiler", "SPDisplaysDataType", "-detailLevel", "mini") if mac else output("lspci")
table = Path(os.environ.get("CROWNLINE_TABLE", ROOT / "evidence/local-table"))
print(json.dumps({
    "platform": platform.platform(), "machine": platform.machine(), "logical_cores": os.cpu_count(),
    "cpu": cpu, "display_hardware": gpu,
    "compute": "CPU only. f64 fit and bounded fixed-point i16/i32 inference.",
    "rust": tool("rustc"), "node": tool("node"), "openssl": tool("openssl", "version"),
    "graphviz": tool("dot", "-V"), "storage": storage(ROOT),
    "repository_writable": os.access(ROOT, os.W_OK), "table_inventory_available": (table / "inventory.json").is_file(),
    "marcher_checkout_configured": "CROWNLINE_MARCHER_SOURCE" in os.environ,
    "chromium_configured": "CROWNLINE_CHROME" in os.environ,
}, indent=2))
