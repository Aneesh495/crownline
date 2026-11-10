# Architecture

The native library owns the English draughts model. The browser receives
legal routes and committed state, and draws a capture preview from the
selected route prefix. Node handles process lifetime, credentials and
HTTP. It submits typed commands through the native CL01 stream. The web
service binds to loopback unless configured with an explicit public origin
behind a TLS reverse proxy.

| Area | Executable mechanism | Diagram |
|---|---|---|
| System context | [Native API](../src/wire.rs) and [HTTP adapter](../web/server.mjs) | [SVG](diagrams/system.svg) · [DOT](diagrams/system.dot) |
| Full-turn capture | [Bitboard geometry and route assembly](../src/moves.rs) | [SVG](diagrams/capture.svg) · [DOT](diagrams/capture.dot) |
| History scope | [Position and repetition suffix](../src/game.rs) | [SVG](diagrams/search.svg) · [DOT](diagrams/search.dot) |
| Exact solving | [Reverse CSR and queue](../src/tablebase/retrograde.rs) | [SVG](diagrams/retrograde.svg) · [DOT](diagrams/retrograde.dot) |
| Learned models | [Original SGD](../src/lab/train.rs) and [sparse fixed-point inference](../src/lab/model.rs) | [SVG](diagrams/model.svg) · [DOT](diagrams/model.dot) |
| Pairing | [Native arena](../src/lab/arena.rs) | [SVG](diagrams/arena.svg) · [DOT](diagrams/arena.dot) |
| Review | [PDN record](../src/pdn.rs) and [candidate analysis](../src/review.rs) | [SVG](diagrams/review.svg) · [DOT](diagrams/review.dot) |
| Ownership | [Room authorization](../web/server.mjs) | [SVG](diagrams/ownership.svg) · [DOT](diagrams/ownership.dot) |
| Lifecycle | [Events and clock transitions](../src/game.rs) | [SVG](diagrams/lifecycle.svg) · [DOT](diagrams/lifecycle.dot) |
| Concurrency | [Bounded child session](../web/native.mjs) | [SVG](diagrams/concurrency.svg) · [DOT](diagrams/concurrency.dot) |
| Persistence | [Shard loading and commit order](../src/tablebase/mod.rs) | [SVG](diagrams/persistence.svg) · [DOT](diagrams/persistence.dot) |
| Evidence | [Campaign driver](../tools/run_campaign.py) and [verifier](../tools/verify_campaign.py) | [SVG](diagrams/evidence.svg) · [DOT](diagrams/evidence.dot) |
| Deployment | [Release packing](../tools/package_release.py) | [SVG](diagrams/deployment.svg) · [DOT](diagrams/deployment.dot) |
| Test structure | [Native oracle](../src/oracle.rs) and [browser checks](../web/tests/browser.mjs) | [SVG](diagrams/testing.svg) · [DOT](diagrams/testing.dot) |

The per-room worker owns the mutable game. A stale revision cannot update
it. Each opaque session credential grants one role, and the join secret
is consumed exactly once. For replay analysis the server starts a separate
bounded child. Cancel closes that child while the game process stays live.
Rooms are in memory and expire after two idle hours. The local browser
stores an unclocked unfinished record in its own storage.

The board-only solver canonicalizes White-to-move positions by 180-degree
rotation plus color swap. A colexicographic occupancy rank and two-bit
piece tags address each slice. Promotion stays within a piece-count
slice. Capture edges point only to lower slices. A sparse slice omits
unpromoted men on their own king row. The independent verifier decodes
all slots, recomputes legal moves using coordinate arrays and checks a
strictly decreasing witness on each decisive chain. Distances are proof
ranks. They are not shortest mate distances.
