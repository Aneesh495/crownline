# Reproduction and recovery

Build with Rust 1.88.0, Node 24.21.0, Python 3.11 or newer, OpenSSL 3 and
Graphviz. Run \`make doctor\` for actual local device, tool and storage
details. The solver and training run on CPU.

Run \`make test\`, \`make solve\`, and \`make review-demo\`. Start the board
with \`make demo\` and open \`http://127.0.0.1:8767\`. To reproduce the
Chromium walkthrough, set \`CROWNLINE_CHROME\` to a Chromium executable and
run \`node web/tests/browser.mjs evidence/ui evidence/local-table\`.

A signed campaign needs the private key matching the checked-in public key.
Use \`make acceptance RUN_ID=<unused-id> PROFILE=full\` from the exact hosted
main tip. Set \`CROWNLINE_MARCHER_SOURCE\` to a clean checkout of the pinned
reference and \`CROWNLINE_CHROME\` to Chromium. The harness uses a new
isolated clone and saves exits and bounded resource observations. The
diagnostic profile uses separate workloads and cannot pass full acceptance.

For a published bundle, check out the \`SOURCE_INPUTS.json\` commit and
run \`make verify EVIDENCE=/path/to/bundle INTEGRITY_ONLY=1\`.
Remove \`INTEGRITY_ONLY=1\` to enforce full acceptance and obtain a failing
exit when mandatory requirements remain open.

Solver shards commit in piece-count order. Re-run the same \`solve\` command
after interruption. Remove a stale \`.build-lock\` only after confirming the
writer has exited. The inventory tracks completed shards. Keep a copy before
manual repair and verify every completed shard before using it. Open frontier
checkpoints verify the independently recomputed prefix before advancing.
Self-play checkpoints one game at a time. Training checkpoints the epoch,
configuration, RNG and all trainable weights. An arena stage currently
starts in a fresh directory.

Competitive rooms require a trusted private network or TLS reverse proxy.
Set \`CROWNLINE_HOST\` and \`CROWNLINE_ORIGIN\` to the intended listener and
exact public origin. Forward the original Host header. The room API stores
only digests of secrets and keeps state in memory. Keep the invite out of
public logs. Rooms disappear after service restart.
