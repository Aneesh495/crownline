#!/usr/bin/env python3
"""Independently recompute campaign results from authenticated raw observations."""

import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
import subprocess
import sys

from evidence_common import (
    MODEL_SEEDS, ROOT, VARIANTS, WITNESS_FEN, check_seal, command, digest, lines, load, source_inventory,
)


def replay(binary, *args):
    run = subprocess.run([str(binary), *(str(a) for a in args)], capture_output=True, text=True, timeout=120)
    if run.returncode:
        raise ValueError(run.stderr.strip() or f"failed replay: {args}")
    return json.loads(run.stdout)


def paired_metrics(path):
    groups = defaultdict(list)
    wins = draws = losses = caps = technical = 0
    for game in lines(path):
        end = game["ending"]
        who = end["winner"]
        is_win = who == game["challenger"]
        if who is None:
            draws += 1
        elif is_win:
            wins += 1
        else:
            losses += 1
        caps += end["reason"] == "ply_cap"
        technical += end["reason"] in {"illegal", "timeout", "crash"}
        groups[game["opening"]["group"]].append(0 if who is None else (1 if is_win else -1))
    if not groups:
        raise ValueError("empty arena")
    families = [groups[key] for key in sorted(groups)]
    totals = [(sum(group), len(group)) for group in families]
    rng = 0xfde3f21b7ac599ed
    mask = (1 << 64) - 1
    estimates = []
    for _ in range(5000):
        up = down = 0
        for _ in totals:
            rng ^= (rng << 13) & mask
            rng ^= rng >> 7
            rng ^= (rng << 17) & mask
            numerator, count = totals[rng % len(totals)]
            up += numerator
            down += count
        estimates.append(up / (2 * down))
    estimates.sort()
    mean = (wins - losses) / (2 * (wins + draws + losses))
    return {
        "games": wins + draws + losses, "wins": wins, "draws_including_caps": draws, "losses": losses,
        "ply_cap_adjudications": caps, "technical_outcomes": technical, "gain_over_equal_score": mean,
        "cluster_bootstrap_95": [estimates[125], estimates[4874]], "independent_opening_families": len(groups),
        "promotion_eligible": technical == 0 and len(groups) >= 8 and mean >= 0.02 and estimates[125] > 0,
    }


def arena_check(lab, model, directory, expected_pairs):
    raw = replay(lab, "verify-arena", model, directory)
    calculated = paired_metrics(directory / "matches.jsonl")
    expected = load(directory / "summary.json")
    if raw["pairs"] != expected_pairs or raw["turns_replayed"] <= 0:
        raise ValueError("arena workload did not complete")
    for key, value in calculated.items():
        observed = expected[key]
        if isinstance(value, float):
            matches = math.isclose(value, observed, abs_tol=1e-12)
        else:
            matches = value == observed
        if not matches:
            raise ValueError(f"arena statistic {key} does not match independent recomputation")
    return calculated


def verify_case(directory, case, binary, lab):
    if case in {"historical-target", "historical-score"}:
        return replay(lab, "verify-historical", ROOT / "config/historical-v1.json", directory / "sources",
                      directory / "data.jsonl", directory, directory / "historical")
    if case == "completed-winner":
        return replay(lab, "verify-playthroughs", directory / "playthroughs.jsonl")
    if case in {"winner", "route", "corruption", "missing", "failed_replay"}:
        return replay(lab, "verify-arena", directory / "model.json", directory / "arena")
    if case == "frontier":
        return replay(binary, "verify-frontier", directory / "table", directory / "partition.json")
    if case == "model":
        return replay(lab, "verify-model", directory / "data.jsonl", directory / "model")
    raise ValueError(f"unrecognized verification case {case}")


def verify(directory, checkout, commit, binary, lab, integrity_only, historical_sources):
    manifest = check_seal(directory, commit, checkout / "config/evidence-ed25519.pub")
    source = load(directory / "SOURCE_INPUTS.json")
    if source["commit"] != commit or source["files"] != source_inventory(checkout):
        raise ValueError("trusted checkout does not match frozen source inputs")
    if manifest["profile"] not in {"full", "diagnostic"}:
        raise ValueError("not a campaign evidence bundle")
    acceptance = load(directory / "ACCEPTANCE.json")
    if acceptance["profile"] != manifest["profile"]:
        raise ValueError("profile inconsistent across manifest and acceptance")
    data = directory / "data/selfplay.jsonl"
    if load(directory / "DATASETS.json")["selfplay_sha256"] != digest(data):
        raise ValueError("dataset manifest disagrees with ledger bytes")
    table = replay(binary, "verify-table", directory / "table")
    expected_positions = 7361090 if manifest["profile"] == "full" else 268316
    if manifest["profile"] == "diagnostic":
        inventory = load(directory / "table/inventory.json")
        if len(inventory["shards"]) != 3:
            raise ValueError("diagnostic missing three-piece closure")
        expected_positions = sum(s["win"] + s["loss"] + s["draw"] for s in inventory["shards"])
    if table["positions"] != expected_positions:
        raise ValueError("closed table workload differs")
    if manifest["profile"] == "full":
        frontier = replay(binary, "verify-frontier", directory / "table", directory / "partitions/resume.json")
        if frontier["expanded"] != 8000 or frontier["unresolved"] <= 0 or (directory / "partitions/resume.json").read_bytes() != (directory / "partitions/direct.json").read_bytes():
            raise ValueError("open-partition coverage or resume equivalence mismatch")
    regenerated_review = replay(binary, "review", checkout / "examples/branch-study.pdn", directory / "table")
    observed_review = load(directory / "review.json")
    if regenerated_review != observed_review:
        raise ValueError("walkthrough review failed replay")
    if manifest["profile"] == "full":
        flags = [r["classification"] for r in regenerated_review["rows"]]
        if flags != ["heuristic_drop", "board_only_lost_draw"]:
            raise ValueError("walkthrough classifications differ")
        if replay(binary, "witness", directory / "table", WITNESS_FEN) != load(directory / "witness.json"):
            raise ValueError("endgame witness failed replay")
    counts = replay(lab, "verify-data", data)
    if counts["games"] != (384 if manifest["profile"] == "full" else 96):
        raise ValueError("self-play workload differs")
    playthroughs = replay(lab, "verify-playthroughs", directory / "playthroughs.jsonl")
    if playthroughs != load(directory / "playthrough-verify.json") or playthroughs["sessions"] != 44:
        raise ValueError("complete-game replay matrix differs")
    if manifest["profile"] == "full":
        if historical_sources is None:
            raise ValueError("provide --historical-sources after acquiring the pinned WCDF inputs")
        if load(directory / "DATASETS.json")["historical"] != load(checkout / "config/historical-v1.json")["sources"]:
            raise ValueError("historical input provenance mismatch")
        historical = replay(lab, "verify-historical", checkout / "config/historical-v1.json", historical_sources,
                            data, directory, directory / "historical")
        if historical != load(directory / "historical-verify.json") or historical["accepted"] != 174:
            raise ValueError("historical holdout coverage differs")
        if acceptance["gates"]["G09"] != "passed":
            raise ValueError("G09 outcome contradicts historical replay")
        if acceptance["gates"]["G12"] == "passed":
            observed = load(directory / "ui/browser-observations.json")
            if not any(row.get("full_game_turns") == 42 for row in observed):
                raise ValueError("G12 complete browser game observation missing")
    for seed in MODEL_SEEDS:
        path = directory / f"seed-{seed}"
        check = replay(lab, "verify-model", data, path)
        if check["seed"] != seed or check["probes_checked"] != 24:
            raise ValueError("seed or frozen model probe coverage differs")
    tactics = load(directory / "tactics.json")
    if len(tactics) != 220 or len({r["fen"] for r in tactics}) != 220:
        raise ValueError("adversarial corpus incomplete")
    if any(r["baseline_score"] != r["oracle_score"] for r in tactics):
        raise ValueError("conservative tactics disagreement")
    if tactics != replay(lab, "tactics"):
        raise ValueError("tactical source replay differs")
    profile = manifest["profile"]
    results = {}
    if profile == "full":
        for seed in MODEL_SEEDS:
            for variant in VARIANTS:
                for budget in ("nodes192", "nodes768"):
                    tag = f"{seed}-{variant}-{budget}"
                    results[tag] = arena_check(lab, directory / f"seed-{seed}/model.json", directory / "arena" / tag, 56)
        for variant in VARIANTS:
            tag = f"101-{variant}-time1ms"
            results[tag] = arena_check(lab, directory / "seed-101/model.json", directory / "arena" / tag, 24)
        promotion = arena_check(lab, directory / "seed-101/model.json", directory / "arena/promotion", 56)
        primary = results["101-blend-nodes768"]
        gate = "passed" if promotion["promotion_eligible"] and primary["promotion_eligible"] and acceptance["strong_reference_replayed"] else "failed"
        if acceptance["gates"]["G11"] != gate:
            raise ValueError("G11 outcome contradicts independent paired statistics")
        if acceptance["strong_reference_replayed"]:
            report = replay(lab, "verify-reference", directory / "identities/marcher-bridge", directory / "reference")
            if report != load(directory / "reference-verify.json") or report["games"] != 48:
                raise ValueError("strong-reference workload or legality check differs")
    else:
        results["diagnostic"] = arena_check(lab, directory / "seed-101/model.json", directory / "arena/diagnostic", 12)
    controls = load(directory / "NEGATIVE_CONTROLS.json")
    required = {"winner", "route", "frontier", "model", "corruption", "missing", "source", "failed_replay"}
    if profile == "diagnostic":
        required.remove("frontier")
    else:
        required.update({"historical-target", "historical-score", "completed-winner"})
    if {row["case"] for row in controls} != required or any(row["exit"] != 1 or row["expected_reason"] not in row["stderr"] for row in controls):
        raise ValueError("semantic negative-control execution is incomplete")
    executed = list(lines(directory / "COMMANDS.jsonl"))
    if not executed or any(row["exit"] != 0 for row in executed if row["required"]):
        raise ValueError("required campaign execution failed or missing")
    print(json.dumps({
        "profile": profile, "source": commit, "proof_positions": table["positions"], "selfplay_games": counts["games"],
        "arena_games_replayed": sum(row["games"] for row in results.values()),
        "g11": acceptance["gates"]["G11"], "acceptance": acceptance["status"], "negative_controls": len(controls),
    }, indent=2))
    if profile == "full" and acceptance["status"] != "passed" and not integrity_only:
        raise ValueError("full acceptance incomplete; signed observations replayed")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    parser.add_argument("--checkout", type=Path, default=ROOT)
    parser.add_argument("--expected", default=None)
    parser.add_argument("--binary-dir", type=Path)
    parser.add_argument("--case")
    parser.add_argument("--historical-sources", type=Path)
    parser.add_argument("--integrity-only", action="store_true")
    args = parser.parse_args()
    checkout = args.checkout.resolve()
    commit = command(["git", "rev-parse", "HEAD"], checkout)
    if args.expected and args.expected != commit:
        raise ValueError("trusted checkout commit differs from expected")
    subprocess.run(["git", "diff-index", "--quiet", "HEAD", "--"], cwd=checkout, check=True)
    path = args.binary_dir or (checkout / "target/release")
    binary = path.resolve() / "crownline"
    lab = path.resolve() / "crownline-lab"
    if args.case:
        check_seal(args.directory, commit, checkout / "config/evidence-ed25519.pub")
        print(json.dumps(verify_case(args.directory, args.case, binary, lab)))
    else:
        verify(args.directory, checkout, commit, binary, lab, args.integrity_only, args.historical_sources)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as error:
        print(f"verify: {error}", file=sys.stderr)
        sys.exit(1)
