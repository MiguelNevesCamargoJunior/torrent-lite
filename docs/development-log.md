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

## Trunk development after PR #1

- User requested direct commits on main. GitHub main already contained merge commit 225f90a for PR #1; local main fast-forwarded to it without overwriting remote work.
- Added `librqbit-core = 9.0.1` only for bounded, offline v1 metainfo parsing. Three tests first failed against a rejecting seam, then passed for known infohash, unsafe Windows name, symlink attribute and 8 MiB bound.
- Added `inspect-torrent` CLI with bounded read and sanitized errors. Process test for synthetic .torrent first failed because command was absent, then passed.
- Found the published v1 parser accepts hybrid metainfo with a `meta version` field. A regression test first returned a summary for the hybrid; an explicit metadata marker parse now rejects v2 and hybrid torrents before treating them as v1. A synthetic multi-file fixture confirms decoded file paths and the private flag.
- A malformed integer greater than signed 64-bit range triggered a panic in upstream `atoi` during metainfo parsing. A bounded preflight token scan now rejects invalid integers before deserialization; a checked sum rejects legal per-file lengths whose total overflows `u64`. Tests cover both and bytes containing an integer marker.
- Hosted CI run 40205928188 passed the local-peer transfer and restart-after-corruption experiment on Windows, Ubuntu and macOS. This is one synthetic single-file scenario, not production engine qualification.
- A new `SafeRoot` creates files only through a capability-scoped open directory and `create_new`; tests were written first and initially failed on the missing module. It rejects lexical traversal, existing files and a Windows junction to an outside directory. Eight competing creates yield one winner. The default suite passes 35 tests. The local sandbox denies junction creation, so the full Windows suite was run outside the sandbox; CI still needs to validate on hosted systems.
- This storage building block is not yet wired to librqbit. Existing-file resume, file identity, symlink races and outbound tracker controls remain open before exposing real downloads.
- Hosted CI run 40206759591 passed the custom storage test on all three OS targets. The adapter remains test-only.
- `FileIdentity` records volume/device and file ID from a handle, and `SafeRoot::open_existing` compares after opening without following the final symlink. It rejects hard links and changed identity. Tests first failed on missing API, then passed on native Windows. Hosted CI run 40207177088 passed the engine restart through this adapter on Windows, Ubuntu and macOS.
- On stable Rust 1.91, `std::os::windows::fs::MetadataExt` by-handle IDs are unstable. The first `cap-fs-ext` path could panic if a filesystem omitted identity fields. Windows now uses the fallible `winx` wrapper for link count and `GetFileInformationByHandleEx(FileIdInfo)` for the full 128-bit ID, returning an OS error if that query fails. Local tests pass; actual ReFS qualification remains outstanding. Source: https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info.

## Transactional state work

- Plan: `docs/superpowers/plans/2026-10-25-state-store.md`. The first SQLite tests cover idempotent add, destination conflicts, the 100-record cap, durable pause/resume, tombstone recovery, future schema rejection and v1 `.torrent` source bytes.
- The first local RED attempt cannot compile the bundled SQLite amalgamation: this machine's Visual Studio C toolchain lacks `stdarg.h`. The default Rust suite still runs; the feature-gated SQLite suite must be exercised by hosted CI until the local C headers are repaired.
- The store is not wired to CLI or engine. A process-held file lock prevents a second `StateStore` owner. Run 40208945614 passed the first lock tests on all three hosted OSes; run 40209013996 exposed an intermittent `Busy` reopening on Ubuntu, so an explicit unlock when the owner drops was added.
- Schema v2 records file identities and preserves v1 registration and intent through a `VACUUM INTO` backup before migration. A pre-existing backup blocks migration instead of being overwritten. Tests cover identity round-trips, conflicting identities and backup preservation. [CI run 40209504799](https://github.com/MiguelNevesCamargoJunior/torrent-lite/actions/runs/40209504799) passed the full matrix on Windows, Ubuntu and macOS, including the durable state and local-peer engine tests.
- A validated state-directory ACL, production service integration and outbound network policy remain open. The feature gate prevents presenting this as a functional downloader.

## Persistent engine storage qualification

- A feature-gated `DurableStorageFactory` now uses `SafeRoot` and the SQLite file identities together. It prevalidates file paths and stored metadata, opens existing files only with matching IDs, records IDs for exclusively created files, bounds file I/O by declared size and never deletes content through the engine hooks.
- The synthetic loopback test closes and reopens the state database between two engine sessions, corrupts one piece and verifies repaired bytes. A separate replacement test proves the identity gate rejects a different file. Full [CI run 40210167629](https://github.com/MiguelNevesCamargoJunior/torrent-lite/actions/runs/40210167629) passed Windows, Ubuntu and macOS, including new feature lints and persistent engine tests.
- The test runs with public discovery disabled and does not establish safe production network use, crash recovery, or resource limits. A crash after file creation but before the SQLite identity commit leaves an unclaimed file and fails closed on restart.

## Outbound network boundary

- The network plan is in `docs/superpowers/plans/2026-10-25-network-boundary.md`. Source inspection of librqbit 9.0.1 showed one SOCKS option for TCP peers and reqwest HTTP(S) trackers, but UDP trackers resolve and send directly. The production integration must disable UDP trackers, DHT, uTP and local discovery until every route has an enforced policy.
- A pure destination policy test first failed to compile because its module was absent. It now passes four cases covering public and special IPv4/IPv6 addresses plus tracker URL schemes and credentials. `cargo clippy --locked --test network_policy -- -D warnings` passed locally. The policy is not a network enforcement layer by itself.
- The policy gained an explicit LAN mode for private and loopback addresses; link-local and documentation addresses remain denied. A separate SOCKS5 proxy test first failed to compile because the proxy module was absent. After implementation, four local tests pass: authenticated relay in LAN mode, wrong credential rejection, direct private IP rejection in public mode, and DNS resolution of `localhost` rejected before any destination socket is opened. The proxy binds only to loopback, uses 32 bytes of OS randomness for its password, pins the checked IP for TCP connections, caps concurrent sessions at 150 and has bounded handshake/connect times. This is still not connected to a production engine session.
- A combined qualification test configures the engine's outbound TCP connector with this proxy and the persisted storage factory. It asserts the proxy accepted a connection and the synthetic payload completed. This test is feature-gated for hosted CI because local bundled SQLite compilation lacks Windows C headers; it still does not enable public discovery or production CLI downloads.
