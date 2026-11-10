# Contributing

Run \`make ci\` before proposing a change. Changes to legal moves need an
independent oracle check. Changes to clocks need deterministic inputs.
Changes to evidence formats need a malformed or semantically altered
artifact test. Preserve raw runs when an experiment changes.

Keep generated shards, browser profiles, signing keys and private room
records out of Git. Report exact source commits, workload sizes, seeds
and observed outcomes when comparing performance.
