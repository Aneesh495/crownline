# Historical and complete-game followup

Executable source `c9e3fed1769300cedaf5309fd62eeaf483614d4c`. Run `followup-v5`.
[Preview and signed evidence](https://github.com/Aneesh495/crownline/releases/tag/v0.1.1-preview)

## Historical holdout

The independent checker replayed all 174 WCDF games and 9,459 moves. The frozen sampling procedure retained 414 positions after excluding 0 boards found in the original self-play stream and 48 repeated historical boards. It checked 3,726 learned scores against the saved targets and exported inference.

| Seed | Classical RMSE | Linear | Network | Blend |
|---|---:|---:|---:|---:|
| 101 | 70.32 | 56.34 | 65.90 | 56.60 |
| 211 | 70.32 | 57.44 | 66.34 | 57.93 |
| 307 | 70.32 | 56.76 | 67.96 | 57.71 |

These values measure agreement with the fixed 4,096-node teacher. The preview models retain their original identities. Historical results did not select or refit them. [Frozen manifest and method](HISTORICAL.md)

## Completed product paths

The complete-game verifier checked 44 recorded sessions and 3,128 turns. The sessions cover local player seats, invite rooms, both human/engine seat assignments, engine-versus-engine play, all three engine levels, and the four clock settings. Human-seat actions are scripted through the ordinary command interface. Chromium additionally completed the 42-turn fixture through the board and captured its final position. Authenticated room participants can export the completed match in the PDN panel. [Workload](PLAYTHROUGHS.md)

## Acceptance and reproduction

[Linux CI and extracted-package replay](https://github.com/Aneesh495/crownline/actions/runs/43045249929) passed for the executable source above. All three published archives were downloaded again and checksum checked. The public evidence passed independent replay from a fresh checkout of the release tag.

G09 and G12 passed in this source-bound run. The verifier rejected all 11 altered or corrupted control bundles, including changed historical targets, model error summaries and a finished-game winner. The designated paired strength gate retains its failed result and the preview continues to use classical evaluation.

    git checkout v0.1.1-preview
    make historical-data
    make verify EVIDENCE=/path/to/crownline-evidence INTEGRITY_ONLY=1

The historical archives are acquired from their pinned official URLs and verified by length and hash. The evidence archive includes the derived records and observations. Full acceptance remains incomplete. Room persistence across service restarts remains a documented limitation.
