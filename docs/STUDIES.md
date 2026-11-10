# Engineering and research notes

## English-rule corner cases

A full jump is chosen after compulsory capture filters out every quiet
move. English draughts allows choosing a shorter completed branch.
A king can change direction and land on its origin again. The capture
state must remember victims even when the moving piece returns to a
square it already occupied. The slow array marks jumped pieces as
blockers and compares the entire route, so an endpoint collision does
not hide a missing branch. [Rules evidence](RULES_MATRIX.md)

## Exact coverage and weak solving

Weak solving establishes the game-theoretic result from the standard
start. Schaeffer et al. established that perfect play there draws. The
Crownline proof covers diagrams up to four total pieces under the
board-only abstraction. Each decisive entry has a finite ranking proof.
Every residual draw has a draw successor and no losing successor for the
opponent. This proves that neither side can force a finite win from
those residual states under the modeled rules. Tournament histories
are outside that proof. [Checkers Is Solved](https://pubmed.ncbi.nlm.nih.gov/17641166/)

## Endgame coverage beyond the closed slice

Five- and six-piece forward exploration retains an explicit unresolved
frontier. The existence of a published larger database does not change
that frontier. The external comparator has its optional database
disabled. [Kingsrow endgame work](https://edgilbert.org/EnglishCheckers/KingsRowEnglish.htm)

## Linear versus network evaluation

The linear model summarizes hand-defined quantities. The network
sees the square distribution and coarse interaction buckets. The test
teacher RMSE assesses imitation; matched games assess decisions inside
search. These endpoints can disagree when a calibrated prediction does
not improve move ordering, tactical horizon or defensive play.
[Model card](MODEL_CARD.md)

## Pruning and search safety

Alpha-beta with PVS can prune subtrees once a bound proves they cannot
change the score. A speculative late reduction has a different premise:
the shallower tree may miss a move. The conservative profile serves as
the comparison. History-dependent draw rights constrain even valid
board transpositions. [Search](../src/search.rs)

## Arena uncertainty

Color pairing removes some color bias within a prefix. Opening families
remain the resampling units because several prefixes share their first
two turns. Fixed-time results on one host include allocation, clock and
model latency. Ply-cap adjudications score one half in the declared
primary endpoint but remain a distinct raw fact. [Experiment plan](EXPERIMENT_PLAN.md)

## Review reliability

A heuristic drop needs comparable budgets from the same history and a
margin in both shallow and deeper searches. A direct board-only lost
draw needs WDL values before and after the recorded turn. The review
identifies those scopes and always shows the opponent's continuation.
[Branch fixture](../examples/branch-study.pdn)
