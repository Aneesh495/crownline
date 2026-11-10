# Dependencies

Rust 1.88 builds the native implementation. The lockfile pins dependency
identities. Serde handles typed interchange, serde_json handles JSON, and
sha2 implements artifact checksums. No library supplies draughts rules,
search, training, or retrograde analysis. No external engines or model
weights are included.
