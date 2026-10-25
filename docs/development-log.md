# Development evidence

2026-10-25 — foundation; Windows native, Rust 1.91.1.

- Approved spec published on main; implementation isolated on feat/foundation.
- Magnet tests first: 6 expected assertion failures and 1 passing invalid-shape test against a rejecting seam. After implementation: 7/7 pass.
- Path policy tests first: 5 expected assertion failures and 1 passing normal-path test against a permissive seam. After implementation: 6/6 pass.
- CLI process tests first: 4/4 fail against an empty executable (missing JSON/output/error codes).
- `cargo test` after initial library implementation: all 13 pass. Incremental compilation emitted filesystem hard-link warnings in the sandbox; subsequent runs use CARGO_INCREMENTAL=0, avoiding this cache behavior without changing production code.

Ruling: implement the first foundation milestone under the user's explicit instruction to start development; no repeat plan approval requested. The full product remains incomplete, clearly labeled in README.

Ruling: offline lexical path policy is implemented before engine writes. It is necessary but insufficient for storage confinement and must never authorize filesystem access on its own.

Ruling: CLI command is explicitly named inspect-magnet, reserving inspect for eventual service IDs. This avoids presenting a partial command as the final service contract.

Ruling: no production engine dependency yet. Source inspection found qualification work for destination filtering and handle-safe storage; inspect-only code must not accidentally activate network discovery.

## Review and final validation

- Added aggregate 8 MiB path-name budget: regression first failed, then passed. Collision detection now sorts component vectors instead of duplicating every directory prefix.
- CLI implementation passed all 4 original subprocess tests; full suite reached 18 tests.
- Fresh independent review found two Important issues: clap usage errors echoed secret-bearing arguments, and Unicode collision keys needed NFC after casing.
- Both issues were reproduced with failing tests, corrected, and followed by a green full suite: **21/21 tests**, `cargo clippy --locked --all-targets -- -D warnings` exit 0.
- Final: minor (deferred): validate component count before creating temporary Vec<&str> in validate_file_set. Current code still rejects excess depth, but allocates that temporary first; future metadata integration must move the check before allocation. No filesystem escape was observed.
- Final: Ruling: actual filesystem confinement/OS-specific aliases, engine transfer/restart/network filtering and API/MCP/UI remain unverified future milestone work. Do not release or expose downloading based on these offline checks. The cost of treating lexical validation as confinement would be unsafe disk access.
- GitHub private vulnerability reporting confirmed enabled by GET API.
- Initial branch CI passed on Windows, Linux and macOS (runs 40201983977 and 40201941471); final correction commit will receive fresh CI.
- Initial optimized build failed only at a sandbox temporary archive rename (access denied). Repeating outside the sandbox succeeded; no compiler source change was needed.
- Corrected source: optimized build succeeded again; real release executable returned the expected JSON identity for the synthetic magnet. Formatting and diff whitespace checks passed.
