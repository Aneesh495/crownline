# Rules-to-test matrix

| Rule | Contract | Evidence |
|---|---|---|
| 1.11 and 1.13 | Twelve starting pieces per side, Black first in PDN | [Published start perft](../src/oracle.rs) |
| 1.15 to 1.18 | Men move and jump forward, kings both ways | [Coordinate array oracle](../src/oracle.rs) |
| 1.19 | Crown ends a man's turn. Victims remain in place until completion | [Directed crown and return-loop fixtures](../src/oracle.rs) |
| 1.20 | Capture is compulsory. Select any maximal continuation along its route | [Unequal branch fixture](../examples/branch-study.pdn) |
| 1.30 to 1.31 | No legal turn loses. Resignation and flag are separate causes | [Game tests](../tests/game.rs), [room tests](../web/tests/session.test.mjs) |
| 1.32.1 | Claim an intended legal turn with two prior occurrences | [Repetition divergence test](../tests/game.rs) |
| 1.32.2 | Forty prior turns for each side with neither man progress nor capture | [79 vs 80 ply test](../tests/game.rs) |
| PDN 3 numeric | Resolve all turns against exact legality and replay alternatives | [PDN tests](../tests/pdn.rs) |

The transition campaign compares 100,000 seeded legal plays with an
independent coordinate generator and transition function. For every position
it checks generated alternatives, incremental hash recomputation,
make/unmake and color rotation. The adversarial campaign samples 44 distinct
positions in each of five motifs and checks conservative search against
the independent minimax tree. The tactical sample is constructed and does
not estimate the frequency of such positions in human games.
