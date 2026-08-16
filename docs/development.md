# Development

Run the local validation gate from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

For a faster client-only check:

```sh
cargo test -p canvas-client --lib --locked --offline
cargo clippy -p canvas-client --all-targets --all-features --locked --offline -- -D warnings
```

Use `RUST_LOG=info cargo sketchi` for startup diagnostics. The desktop client
and collaboration server are separate workspace packages; run the server with
`cargo run --package canvas-server --bin sketchi-server` when testing it on its
own.
