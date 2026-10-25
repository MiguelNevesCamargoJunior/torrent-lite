# Contributing

This project is in experimental development. Read the design, implementation plans and current engine assessment before proposing download features. Passing tests alone is not evidence of a secure downloader.

Use Rust 1.91.1 or later with rustfmt and clippy. No Node runtime is required for the CLI, MCP adapter or browser dashboard.

```sh
cargo test --locked
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --features service-core --test service_core
cargo test --locked --features mcp --test mcp
cargo test --locked --features browser-ui --test browser_ui
```

Use small changes with one clear purpose. For behavior changes, add a test, observe its expected failure, implement the minimum, then refactor while green. Keep integration fixtures synthetic and local; tests must not contact public trackers or download third-party content. Record relevant commands and results in the pull request. Avoid tests that only inspect source text.

Do not commit credentials, downloaded content, runtime state or build output. Keep Cargo.lock versioned. Discuss dependency additions and security boundaries. Keep new network and file operations behind tests for their security boundaries. Document unsupported protocols and failed tests explicitly.

Be respectful and constructive. Critique ideas and code rather than people. Participation must be free from harassment and discrimination. Submit contributions under Apache-2.0; preserve third-party attribution.

