# Artifact formats

CL01 requests and responses have four ASCII bytes \`CL01\`, a 32-bit
little-endian payload length, and UTF-8 JSON. Input frames are limited to
384 KiB and output frames to 4 MiB. A response has either \`ok: true\`
and \`value\`, or \`ok: false\` and \`error\`. Malformed framing terminates
the stream. Square paths are one-based on the wire. Search reports mover
scores, nodes and completed-depth statistics. A game revision increases
on committed events and across rematches. See [the typed API](../src/wire.rs).

A table shard is an array of little-endian 32-bit words. The top two bits
encode invalid, win, loss or draw. The low bits hold a finite witness rank
for decisive entries. A draw has a zero rank field. The inventory contains
scope, counts and SHA-256 for each byte sequence. The format includes
one-color diagrams for terminal certificates.

The campaign records \`SOURCE_INPUTS.json\`, \`DATASETS.json\`,
\`ENVIRONMENT.json\`, \`COMMANDS.jsonl\`, \`RESOURCES.jsonl\`, raw ledgers,
proof shards, model checkpoints, probes and \`ACCEPTANCE.json\`. The
Ed25519 signature binds the canonical byte inventory to the public key
pinned with the source. Local signing credentials are kept out of Git.
Verify an extracted bundle against its published source commit. See
[reproduction and recovery](REPRODUCIBILITY.md).

Match rows preserve colors, prefix, model identity, exact moves, terminal
reason, nodes and elapsed microseconds. Termination codes distinguish rule
draws, caps, technical losses and decisive board outcomes. Evaluation scores
use hundredths of a man, while terminal search scores and board-only proof
sentinels sit outside the heuristic range.

Completed room snapshots expose canonical PDN to authenticated participants.
The record becomes available after a terminal game event. A local session
can use the import and analysis commands. Competitive sessions retain
their route authorization after completion.

The complete-game JSONL artifact contains one row per planned session.
Each includes scenario metadata and the full canonical PDN. The native
\`verify-playthroughs\` command checks matrix completeness, all moves,
the final board and history-sensitive house draws.
[Complete-game coverage](PLAYTHROUGHS.md)
