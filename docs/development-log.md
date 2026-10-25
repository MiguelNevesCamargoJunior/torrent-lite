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
