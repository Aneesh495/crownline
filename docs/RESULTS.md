# Preview campaign results

Executable source `c155a4c65650fdb4037bbad73375d5e797a6c5a5`. Run `preview-final`. [Release and signed raw artifacts](https://github.com/Aneesh495/crownline/releases/tag/v0.1.0-preview)

## Rules and proof

The native checks replayed 100,000 seeded transitions and matched the independently published perft counts through depth eight. The full solver verified 7,361,090 canonical positions and 24,691,897 full-turn routes. Of these routes, 1,247,720 entered lower piece-count slices. The four-piece shard alone contains 3,016,033 wins, 2,358,819 losses and 1,717,922 draws. Counts are from the side to move. The proof assumes board-only rules.

The six-piece start reached 18,199 distinct diagrams after 8,000 expansions. 10,199 remain unresolved. The resumed and direct runs produced identical bytes. The selected depth-five tactical corpus contains 220 distinct cases. Selective search visited 83,170 nodes versus 82,953 for conservative search. 0 scores differed.

## Training and matches

The independent replay checked 384 training-source games, 41,850 turns and 3,965 extracted labels. It excluded 489 overlapping rows. The unique train, validation and test sets contain 2,001, 618 and 635 examples respectively.

| Seed | Classical test RMSE | Linear | Network | Blend |
|---|---:|---:|---:|---:|
| 101 | 111.65 | 81.72 | 79.80 | 75.24 |
| 211 | 111.65 | 81.29 | 80.59 | 75.36 |
| 307 | 111.65 | 81.71 | 80.38 | 75.66 |

The arena replay covered 2,016 fixed-node test games and 144 fixed-time test games. These comparisons share opening families. Another 112 games were reserved for validation.

| Designated blend at 768 nodes | W-D-L | Score gain | Paired 95% interval |
|---|---:|---:|---:|
| Validation | 46-31-35 | +4.9 pp | -0.5 to +10.5 pp |
| Test | 46-36-30 | +7.1 pp | -1.3 to +14.2 pp |

The improvement gate failed. Both intervals include zero. The preview keeps the classical evaluator.

Against pinned Marcher, Crownline classical scored 2-14-32 over 48 games. The nominal budget was 4 ms. Crownline and Marcher used different clock APIs, with 0 and 2 turns over 8 ms respectively. 1 games were capped. The small sample gives context at this budget.

## Replaying the evidence

The separate verifier rejected all 8 corrupted or re-signed control bundles. They include a changed winner, illegal capture, forged frontier and changed model identity. Chromium observations cover keyboard play, reload, mobile layout, capture continuity, cancel and the integrated proof walkthrough.

    git checkout v0.1.0-preview
    make verify EVIDENCE=/path/to/crownline-evidence INTEGRITY_ONLY=1

Full acceptance remains incomplete. The external tournament holdout, full human playthrough matrix and the designated strength gate remain open. See [build status](BUILD_STATUS.md).
