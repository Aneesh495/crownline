#!/usr/bin/env python3
"""Build a clean published revision and retain signed, bounded campaign evidence."""

import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import time

from evidence_common import (
    FRONTIER_FEN, MODEL_SEEDS, ROOT, VARIANTS, WITNESS_FEN, command, digest, lines, load, save, seal,
    source_inventory, storage,
)
from verify_campaign import paired_metrics


class Campaign:
    def __init__(self, output, source):
        self.output = output
        self.source = source
        self.log = output / "COMMANDS.jsonl"
        self.resources = output / "RESOURCES.jsonl"
        (output / "logs").mkdir()

    @staticmethod
    def append(path, row):
        with path.open("a", encoding="utf-8") as file:
            file.write(json.dumps(row, sort_keys=True, allow_nan=False) + "\n")

    def run(self, name, argv, artifact=None, timeout=120, required=True):
        before = storage(ROOT)
        self.append(self.resources, {"stage": name, "when": "before", **before})
        start = time.monotonic()
        stdout = self.output / (artifact or f"logs/{name}.stdout")
        stderr = self.output / f"logs/{name}.stderr"
        args = [str(x) for x in argv]
        env = dict(os.environ)
        env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join([
            "--remap-path-prefix", str(self.source) + "=/crownline",
            "--remap-path-prefix", str(Path.home()) + "=/build-home",
        ])
        env.pop("CARGO_TARGET_DIR", None)
        with stdout.open("xb") as out, stderr.open("xb") as err:
            try:
                run = subprocess.run(args, stdout=out, stderr=err, cwd=self.source, timeout=timeout, env=env)
                status = run.returncode
            except subprocess.TimeoutExpired:
                status = 124
        # Retain the command and diagnostics without publishing host-specific paths.
        def portable(text):
            for local, token in [(self.output, "{EVIDENCE}"), (self.source, "{SOURCE}"), (ROOT, "{PROJECT}"), (Path.home(), "{HOME}")]:
                text = text.replace(str(local), token)
            return text

        for path in [stdout, stderr]:
            text = path.read_text(encoding="utf-8", errors="replace")
            normalized = portable(text)
            if text != normalized:
                path.write_text(normalized, encoding="utf-8")
        display = [portable(item) for item in args]
        self.append(self.log, {"stage": name, "argv": display, "exit": status, "required": required,
                               "elapsed_s": time.monotonic() - start, "stdout": stdout.relative_to(self.output).as_posix(),
                               "stderr": stderr.relative_to(self.output).as_posix()})
        self.append(self.resources, {"stage": name, "when": "after", **storage(ROOT)})
        if status:
            print(f"{name} exited {status}", flush=True)
            if required:
                raise ValueError(f"stage {name} failed. See {stderr}")
        else:
            print(f"{name} complete", flush=True)
        return status


def copy_arena(source, target):
    shutil.copy2(source / "seed-101/model.json", target / "model.json")
    name = "101-blend-nodes768" if (source / "arena/101-blend-nodes768").exists() else "diagnostic"
    shutil.copytree(source / "arena" / name, target / "arena")


def controls(campaign, key, commit, bin_dir, full):
    output = campaign.output
    temp = campaign.source.parent / "controls"
    temp.mkdir()
    cases = ["winner", "route", "model", "corruption", "missing", "source", "failed_replay"]
    if full:
        cases.append("frontier")
    records = []
    for case in cases:
        directory = temp / case
        directory.mkdir()
        expected = {
            "winner": "recorded winner or result",
            "route": "illegal full-turn replay",
            "model": "model training dataset identity mismatch",
            "corruption": "artifact hash or missing-file mismatch",
            "missing": "artifact hash or missing-file mismatch",
            "source": "signed evidence source identity mismatch",
            "failed_replay": "recorded winner or result",
            "frontier": "independently replayed frontier",
        }[case]
        if case == "model":
            shutil.copy2(output / "data/selfplay.jsonl", directory / "data.jsonl")
            shutil.copytree(output / "seed-101", directory / "model")
            model_path = directory / "model/model.json"
            model = load(model_path)
            model["data_sha256"] = "0" * 64
            save(model_path, model)
        elif case == "frontier":
            shutil.copytree(output / "table", directory / "table")
            record = load(output / "partitions/resume.json")
            record["internal_edges"] += 1
            save(directory / "partition.json", record)
        else:
            copy_arena(output, directory)
            ledger = directory / "arena/matches.jsonl"
            rows = list(lines(ledger))
            if case == "winner":
                game = next(g for g in rows if g["ending"]["winner"] is not None)
                game["ending"]["winner"] = "Black" if game["ending"]["winner"] == "White" else "White"
            elif case == "route":
                game = next(g for i, g in enumerate(rows) if i % 11 != 0 and len(g["moves"]) > 2)
                game["moves"][1] = "1x32"
            elif case == "failed_replay":
                game = next(g for i, g in enumerate(rows) if i % 11 != 0 and g["ending"]["winner"] is not None and len(g["moves"]) > 2)
                game["moves"].pop()
            if case in {"winner", "route", "failed_replay"}:
                ledger.write_text("".join(json.dumps(row, separators=(",", ":")) + "\n" for row in rows), encoding="utf-8")
        seal(directory, key, "control", "f" * 40 if case == "source" else commit)
        if case == "corruption":
            with (directory / "model.json").open("ab") as file:
                file.write(b"x")
        elif case == "missing":
            (directory / "model.json").unlink()
        args = [sys.executable, str(campaign.source / "tools/verify_campaign.py"), str(directory),
                "--checkout", str(campaign.source), "--expected", commit, "--binary-dir", str(bin_dir), "--case", case]
        check = subprocess.run(args, cwd=campaign.source, capture_output=True, text=True, timeout=120)
        records.append({
            "case": case, "exit": check.returncode, "expected_reason": expected,
            "stderr": check.stderr.strip(), "stdout": check.stdout.strip(),
            "argv": [Path(args[0]).name, "tools/verify_campaign.py", f"controls/{case}", "--case", case, "--expected", commit],
        })
        if check.returncode != 1 or expected not in check.stderr:
            save(output / "NEGATIVE_CONTROLS.json", records)
            raise ValueError(f"control {case} did not reject for its intended semantic reason: {check.stderr}")
    save(output / "NEGATIVE_CONTROLS.json", records)


def execute(options):
    if not re.fullmatch("[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}", options.run_id):
        raise ValueError("run ID must use 1 to 64 letters, digits, hyphens or underscores")
    full = options.profile == "full"
    commit = command(["git", "rev-parse", "HEAD"])
    subprocess.run(["git", "diff-index", "--quiet", "HEAD", "--"], cwd=ROOT, check=True)
    remote = None
    if full:
        remote = command(["git", "ls-remote", "origin", "refs/heads/main"]).split()[0]
        if remote != commit:
            raise ValueError("full campaign must run from the exact hosted main revision")
    key = Path(os.environ.get("CROWNLINE_EVIDENCE_KEY", ROOT / ".agent-local/evidence-ed25519.pem")).resolve(strict=True)
    public = command(["openssl", "pkey", "-in", str(key), "-pubout"])
    if public != (ROOT / "config/evidence-ed25519.pub").read_text().strip():
        raise ValueError("signing key does not match the checked-in project public key")
    storage(ROOT)
    output = ROOT / "evidence" / options.run_id
    output.mkdir(parents=True, exist_ok=False)
    stage = ROOT / ".agent-local" / "campaigns" / options.run_id
    stage.mkdir(parents=True, exist_ok=False)
    source = stage / "source"
    command(["git", "clone", "--quiet", "--shared", "--no-hardlinks", str(ROOT), str(source)])
    command(["git", "checkout", "--quiet", "--detach", commit], source)
    runner = Campaign(output, source)
    save(output / "SOURCE_INPUTS.json", {"commit": commit, "remote_main": remote, "files": source_inventory(source)})
    save(output / "ENVIRONMENT.json", {
        "system": platform.system(), "release": platform.release(), "machine": platform.machine(),
        "cpu_count": os.cpu_count(), "rust": command(["rustc", "--version"], source),
        "cargo": command(["cargo", "--version"], source), "node": command(["node", "--version"], source),
        "python": platform.python_version(), "numeric": "CPU f64 fitting, fixed-point i16/i32 inference",
        "signer": command(["openssl", "version"], source),
    })
    runner.run("build", ["cargo", "build", "--locked", "--release"], timeout=300)
    runner.run("tests", ["cargo", "test", "--release", "--locked", "--", "--nocapture"], timeout=300)
    runner.run("api", ["node", "--test", "web/tests/session.test.mjs"])
    runner.run("verifier-smoke", [sys.executable, "tools/smoke_verifier.py"])
    runner.run("package", [sys.executable, "tools/package_release.py", "0.1.0-preview"])
    package = source / "dist" / f"crownline-0.1.0-preview-{platform.system().lower()}-{platform.machine()}.tar.gz"
    runner.run("package-replay", [sys.executable, "tools/test_package.py", package])
    save(output / "PACKAGE.json", {"file": package.name, "sha256": digest(package), "bytes": package.stat().st_size})
    bin_dir = source / "target/release"
    binary = bin_dir / "crownline"
    lab = bin_dir / "crownline-lab"
    identities = output / "identities"
    identities.mkdir()
    shutil.copy2(binary, identities / "crownline")
    shutil.copy2(lab, identities / "crownline-lab")
    table = output / "table"
    runner.run("solve", [binary, "solve", table, 4 if full else 3], "solve.json")
    runner.run("verify-table", [binary, "verify-table", table], "table-verify.json")
    runner.run("tactics", [lab, "tactics"], "tactics.json")
    runner.run("pdn", [binary, "pdn", "examples/branch-study.pdn"], "branch-export.pdn")
    runner.run("pdn-roundtrip", [binary, "pdn", output / "branch-export.pdn"], "branch-roundtrip.pdn")
    if (output / "branch-export.pdn").read_bytes() != (output / "branch-roundtrip.pdn").read_bytes():
        raise ValueError("PDN round trip changed canonical bytes")
    runner.run("review", [binary, "review", "examples/branch-study.pdn", table], "review.json")
    browser_ok = False
    if full:
        (output / "partitions").mkdir()
        resumed = output / "partitions/resume.json"
        direct = output / "partitions/direct.json"
        for name, file, n in [("frontier-first", resumed, 1000), ("frontier-resume", resumed, 8000), ("frontier-direct", direct, 8000)]:
            runner.run(name, [binary, "explore", table, file, n, FRONTIER_FEN])
        if resumed.read_bytes() != direct.read_bytes():
            raise ValueError("resume and one-shot expansions differ")
        runner.run("frontier-verify", [binary, "verify-frontier", table, resumed], "frontier-verify.json")
        runner.run("witness", [binary, "witness", table, WITNESS_FEN], "witness.json")
        runner.run("proof-search", [binary, "search-proof", table, 12, 20000, WITNESS_FEN], "proof-search.json")
        runner.run("plain-search", [binary, "search", 12, 20000, WITNESS_FEN], "plain-search.json")
        if os.environ.get("CROWNLINE_CHROME"):
            browser_ok = runner.run("browser", ["node", "web/tests/browser.mjs", output / "ui", table], timeout=100, required=False) == 0
    runner.run("selfplay", [lab, "selfplay", stage / "selfplay", 384 if full else 96], "selfplay.json")
    data = output / "data"
    data.mkdir()
    ledger = data / "selfplay.jsonl"
    shutil.copy2(stage / "selfplay/selfplay.jsonl", ledger)
    save(output / "DATASETS.json", {"selfplay_sha256": digest(ledger), "source": "original deterministic engine games",
                                     "opening_groups": "first two turns, hash bucket, cross-split canonical board exclusion"})
    runner.run("verify-data", [lab, "verify-data", ledger], "data-verify.json")
    for seed in MODEL_SEEDS:
        path = output / f"seed-{seed}"
        runner.run(f"train-{seed}", [lab, "train", ledger, path, seed, 8 if full else 3, 100])
        runner.run(f"verify-model-{seed}", [lab, "verify-model", ledger, path])
    (output / "arena").mkdir()

    def arena(name, seed, variant, budget, split, pairs):
        model = output / f"seed-{seed}/model.json"
        path = output / "arena" / name
        runner.run(name, [lab, "arena", model, path, variant, budget, split, pairs])
        runner.run(f"verify-{name}", [lab, "verify-arena", model, path])

    if full:
        for fraction in (25, 50):
            runner.run(f"ablation-{fraction}", [lab, "train", ledger, output / f"ablation-{fraction}", 101, 8, fraction])
        arena("promotion", 101, "blend", "nodes768", "validation", 56)
        for seed in MODEL_SEEDS:
            for variant in VARIANTS:
                for budget in ("nodes192", "nodes768"):
                    arena(f"{seed}-{variant}-{budget}", seed, variant, budget, "test", 56)
        for variant in VARIANTS:
            arena(f"101-{variant}-time1ms", 101, variant, "time1ms", "test", 24)
    else:
        arena("diagnostic", 101, "blend", "nodes192", "test", 12)
    reference = os.environ.get("CROWNLINE_MARCHER_SOURCE")
    strong_ok = False
    if full and reference:
        external = identities / "marcher-bridge"
        built = runner.run("marcher-build", ["sh", "tools/build-marcher.sh", str(Path(reference).resolve()), external], required=False)
        if built == 0:
            run = runner.run("marcher-arena", [lab, "reference", external, output / "reference", 24],
                             "reference-run.json", timeout=180, required=False)
            if run == 0:
                strong_ok = runner.run("marcher-verify", [lab, "verify-reference", external, output / "reference"],
                                       "reference-verify.json", required=False) == 0
    controls(runner, key, commit, bin_dir, full)
    gates = {f"G{i:02}": "unverified" for i in range(1, 17)}
    for index in [1, 2, 3, 4, 5, 6, 10, 13, 15]:
        gates[f"G{index:02}"] = "passed"
    if full:
        for index in [7, 8, 14, 16]:
            gates[f"G{index:02}"] = "passed"
        primary = paired_metrics(output / "arena/101-blend-nodes768/matches.jsonl")
        promotion = paired_metrics(output / "arena/promotion/matches.jsonl")
        gates["G11"] = "passed" if primary["promotion_eligible"] and promotion["promotion_eligible"] and strong_ok else "failed"
        gates["G12"] = "unverified" if browser_ok else "blocked"
    save(output / "ACCEPTANCE.json", {
        "profile": options.profile, "status": "incomplete" if full else "diagnostic",
        "gates": gates, "strong_reference_replayed": strong_ok, "browser_replayed": browser_ok,
        "limitations": ["external tournament training holdout absent", "full product acceptance outstanding"],
    })
    seal(output, key, options.profile, commit)
    verify = subprocess.run([
        sys.executable, str(source / "tools/verify_campaign.py"), str(output),
        "--checkout", str(source), "--expected", commit, "--binary-dir", str(bin_dir),
        "--integrity-only",
    ], check=True, cwd=source)
    if verify.returncode:
        return verify.returncode
    if full:
        print("Full acceptance incomplete. Signed evidence replayed.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("run_id")
    parser.add_argument("profile", choices=["full", "diagnostic"])
    try:
        sys.exit(execute(parser.parse_args()))
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as error:
        print(f"campaign: {error}", file=sys.stderr)
        sys.exit(1)
