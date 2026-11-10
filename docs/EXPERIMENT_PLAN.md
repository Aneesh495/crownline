# Crownline experiment plan v1

## Rules and reproducibility

WCDF 2012 movement with PDN numeric game type 21. Arena adjudication uses
house repetition and no-progress rules. A 160-ply cap after the opening is
counted separately as an adjudication and scores one half in the primary
comparison. Search uses one thread, conservative PVS, 512 TT entries, and
the same quiescence for both seats. Proofs use the board graph with infinite
play as a draw. They cannot certify a position's tournament draw rights.

The opening generator enumerates all legal three-turn prefixes from the
standard position in numeric route order. SHA-256 of the first two complete
turns assigns the opening family to train, validation, or test. It yields
302 prefixes in 49 families, with 58 test openings in 11 families. Training
excludes any canonical board observed across multiple splits and deduplicates
by board plus reversible history. Reports expose exclusions and denominators.
Held-out games from external historical collections are a separate requirement.

## Training endpoint

Generate 384 original classical self-play games at 256 nodes with frozen
early-game diversity and deterministic scheduling. Label up to 12 positions
per trajectory with 2,048-node teacher searches and the mover's outcome.
A ply-cap score supplies no outcome label. Seed 101, 211, and 307 initialize
independent models. Run eight epochs at 100% of the training examples for
each seed. At seed 101, repeat with the frozen 25% and 50% subsamples. Compare
classical, learned linear, independent sparse network, blended, and disabled
predictions. Compute RMSE in hundredths of a man. Use only validation games
for promotion. Test results are descriptive across every preregistered
variant, including poor seeds.

## Arena endpoint

Run the linear, network and blended evaluators from all three seeds against
the untuned classical evaluator. Use the first 56 test prefixes, both colors
per prefix, with 192 and 768 nodes per move per side. The planned matrix has
2,016 games. These are 18 comparisons sharing test openings, rather than
2,016 independent samples. Compare fixed time separately with the same first
24 test prefixes, both colors, for seed 101 in all three variants at a 1 ms
hard search deadline per move. Retain nodes and elapsed time per seat. The
sole designated test profile is seed 101 blend at 768 nodes. Run its
validation-only promotion check before scoring the held-out games.

The null is expected paired score at most one half. Resample opening families
with replacement (5,000 draws, fixed xorshift seed, percentile 95% interval).
A candidate needs a lower interval bound greater than zero, an observed gain
of at least two percentage points, at least eight families, and no technical
outcomes. A failed validation check stops promotion. Games remain in the
ledger regardless of result. A strong reference comparison is a separate
matched-time experiment. The pinned comparator candidate is
[Marcher at 1fa785e](https://github.com/Stermere/Checkers-Engine/tree/1fa785edbe8003445163a63f00f04a4c0b77254b).
Report the exact binary, flags, caps and limitations if the adapter passes
independent legality checks.

## Engineering endpoints

Require 100,000 legal transition replays, at least 200 distinct tactical
positions, independent full WDL equations over the closed four-piece slice,
a bounded six-piece exploration with byte-equivalent resume, live browser
walkthrough, and replay checks for arena and model identity. Independent
semantic controls alter winner, illegal route, proof frontier and model
identity after rehashing the bundle.

## Bounded local diagnostic

Use 96 games and a small training/arena workload for resource estimation.
This is never the full campaign. Apple Silicon macOS and CPU-only Rust are
the local reference. Peak plan is under 1 GB transient memory for the solver,
under 500 MB generated evidence and under 5 GB total attributable local disk.
Abort when the project reaches 5,000,000,000 bytes or the Mac APFS container
reaches 400,000,000,000 bytes. Use atomic shard, game and epoch files as
checkpoint boundaries. Keep the raw diagnostic and clean source identities.
