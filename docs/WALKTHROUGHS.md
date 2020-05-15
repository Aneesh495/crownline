# Walkthroughs

## A legal full turn

In local study, use FEN \`B:BK14:W10,11,18,19\`. Click squares 14, 7, 16,
23 and 14. The browser fades each victim but holds the same turn until the
path completes. Export to PDN and inspect \`14x7x16x23x14\`.
[Route generation](../src/moves.rs) · [Browser preview](../web/public/board.js)

## A proof-backed review

Run \`make review-demo\` or paste [the study PDN](../examples/branch-study.pdn)
into the browser with the table configured. The opening offers \`11x2\`
and \`11x18x25\`. At White's turn, \`15-10\` keeps the board-only draw.
The played \`15-11\` permits a proven win for Black. Open the witness from
\`B:BK2,K8:W11,K22\`. The opening's heuristic scores and direct proof
coverage are separately marked. [Review](../src/review.rs)

## An invite-only game

Start a room from the Play tab and open the one-use invitation in a second
browser profile. Each seat can submit turns only for its bound role.
Reload either profile to reconnect. Invite redemption and revision handling
are exercised by the [real HTTP integration tests](../web/tests/session.test.mjs).

## A recoverable parse error

Import \`1. 9-13 21-17 2. {unterminated\` in strict mode to get a byte
diagnostic. The Read valid prefix action keeps both legal turns and
exports an unfinished canonical record. Repair the input in the notebook
and import it again. [PDN lexer and parser](../src/pdn.rs)

## A solver checkpoint

Run \`make solve PIECES=3 SOLVE_DIR=evidence/resumed\`, then rerun with
\`PIECES=4\`. The solver validates completed shard hashes before adding
the next piece count. The separate verifier traverses every slot and route.
[Shard commit order](../src/tablebase/mod.rs)
