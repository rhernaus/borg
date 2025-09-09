# To check for problems, run

```bash
cargo fmt --all && \
        cargo clippy --all-targets --all-features -- -D warnings -W clippy::cognitive_complexity && \
        cargo test --all --locked && \
        cargo audit --deny warnings && \
        cargo build --release --locked --all-features
```
