# Decision records

## 01. Native engine with a thin HTTP adapter

Rust owns legality, clocks and search. JavaScript owns presentation and
sessions. A shared WASM target would simplify static hosting but would
introduce a second runtime profile and constrain room authority. The
desktop preview uses a native child configured by path.

## 02. Complete turns at every engine boundary

The capture generator stores all landings and a victim mask. A jump-by-jump
search would need an extra side-to-move convention and different depth
units. The complete route also makes ambiguous endpoint notation detectable.

## 03. Capture victim blockers

The production generator keeps captured pieces in the blocker mask until
turn end and subtracts them from the available target mask. The independent
array oracle marks them with a sentinel. Clearing the moving piece's origin
from blockers permits a legal route back to it.

## 04. Separate board key and draw context

A board hash alone cannot distinguish claim eligibility. The game retains
an exact reversible suffix. Search restricts bound reuse to a fresh
suffix or explicit board-only mode. This sacrifices some king-endgame
transposition hits in exchange for a simple invariant.

## 05. Replayable monotonic time

Game operations receive a monotonic millisecond counter and a wall-clock
event timestamp separately. Replaying wall time directly could produce
negative elapsed time after a system adjustment. Clock arithmetic is
tested by passing controlled counters.

## 06. Conservative search by default

The preview enables safe alpha-beta and PVS. The reduction profile is
experimental because a failed low-depth probe can miss a move. Minimax
and selective ablation use the same full-turn depth unit and terminal
scores, which isolates the effect of the search optimization.

## 07. Canonicalization by color rotation

A 180-degree rotation preserves dark-square parity and reverses the
men's movement direction. Swapping colors restores the original rule.
Plain left-right reflection changes playable parity on an 8x8 grid, so
that operation is not used for proof ranking.

## 08. Witness ranks instead of distance optimization

The queue can assign a finite decreasing certificate as soon as WDL is
decided. Seeking the shortest forced win would impose ordering across
same-slice and lower-slice candidates. The witness viewer displays
a legal certified defense line with an explicit bound.

## 09. Explicit open frontier

A capped five/six-piece exploration stores discovered positions and an
expansion cursor. It does not infer draws from an unexplored neighbor.
The verifier rebuilds the prefix using independent transitions and
the checkpoint's declared start and bound.

## 10. Shared scorer with two perspectives

The network maps each side into the same feature space and subtracts
the two scores. Independent output heads would not guarantee sign
symmetry. The lane accumulator updates changed square features and adds
phase and interaction features at inference time.

## 11. Opening-family grouping and paired games

Adjacent variations share opening structure. Assign the first two turns
to one dataset group and play each test prefix in both colors. The
bootstrap resamples whole opening families. The same groups appear
across model variants and cannot be pooled as independent outcomes.

## 12. Integrity and semantic replay

Signed hashes authenticate the campaign files and commit. They cannot
establish that a winner or proof is correct. A separate verifier
recomputes legal replays, WDL recurrences and interval statistics.
Negative controls re-sign changed evidence and require a semantic rejection.

## 13. Cancellation by process isolation

An asynchronous analysis child can be terminated without losing a
live game. The persistent worker still uses a bounded request queue,
revision guards and native per-move deadlines for bot turns.

## 14. Explicit PDN dialect

The supported PDN subset uses numeric GameType 21, one record, header
FEN, full captures and recursive alternatives. Unsupported movement
notations and conflicting move numbers produce an offset. Partial
mode returns a legal prefix with the error attached.
