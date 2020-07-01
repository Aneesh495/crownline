# Dependencies

Rust 1.88 builds the native implementation. The lockfile pins dependency
identities. Serde handles typed interchange, serde_json handles JSON, and
sha2 implements artifact checksums. No library supplies draughts rules,
search, training, or retrograde analysis. No external engines or model
weights are included.

Node 24 provides the browser server and native process adapter through
its standard library. Python orchestration uses only the standard
library. Graphviz renders repository-authored diagrams and OpenSSL
verifies evidence signatures. Chromium is optional for browser tests.
Serde and serde_json are dual MIT/Apache-2.0. RustCrypto sha2 offers the
same licenses. Dependency source and license notices are available from
the registry records pinned in [Cargo.lock](../Cargo.lock).

The optional external Marcher comparator is MIT-licensed and pinned in
the experiment plan. Its original code, networks and databases are kept
outside the product archive and normal game runtime.

Pinned historical inputs are acquired with the system curl command and its
configured TLS trust store. The acquisition recipe checks both exact length
and SHA-256 before saving bytes.
