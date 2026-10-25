# Foundation implementation plan

> For agentic workers: execute with superpowers:executing-plans, using TDD for each behavior. User explicitly authorized repository creation, publication and development on 2026-10-25; no additional execution approval is needed.

**Goal:** Publish an honest development repository and implement a small tested offline CLI foundation while assessing the download engine before exposing network/file writes.

**Architecture:** Rust library owns validated inputs and portable path policy; CLI consumes that library. Engine evaluation is separate from production download integration. Later phases add the single-owner service, SQLite, HTTP, MCP and web UI described in the spec.

**Tech Stack:** Rust 2024, clap, url, data-encoding, serde/serde_json, thiserror; candidate librqbit 9.0.1 assessed independently.

**Spec:** ../specs/2026-10-25-torrent-design.md

## Global constraints

- Windows first, no WSL/runtime Node requirement.
- Magnet limit 16 KiB; metainfo limit 8 MiB; up to 10,000 files.
- No automatic network activity, file execution, physical deletion or silent unsupported format acceptance.
- Path lexical checks alone never authorize storage access; filesystem race/reparse protection remains a gate for the storage implementation.
- No downloader/release/performance claim before the complete integration criteria pass.
- Work on `feat/foundation`, preserve `main` as the design baseline.

## Review focus

- Percent-encoded or repeated magnet topics could bypass identity checks: task 1 tests conflicts and encoded topics.
- Unsupported v2/hybrid magnets must fail explicitly: task 1 tests btmh, with/without btih.
- Windows aliases and nested names may escape or collide: task 2 tests drives, ADS, DOS devices, trailing spaces and path components.
- CLI errors could echo sensitive tracker credentials: task 3 tests errors do not contain supplied secret.
- A command may succeed without doing the advertised work: task 3 tests only documented offline commands and nonzero errors for unsupported commands.

## Task 1: Validated magnet identity

Files: Cargo.toml, src/lib.rs, src/magnet.rs, tests/magnet.rs.

Produces `Magnet::parse(&str) -> Result<Magnet, InputError>` with normalized hex `info_hash()` and optional display name; raw tracker queries are not emitted by default.

- [x] RED: tests for literal 40-digit hexadecimal and equivalent 32-character base32 identity, percent-encoded xt, missing/invalid hash, conflicting/repeated xt, unsupported btmh, wrong scheme and 16 KiB boundary. Assertions use hand-derived identity `000102030405060708090a0b0c0d0e0f10111213` and base32 `AAAQEAYEAUDAOCAJBIFQYDIOB4IBCEQT`.
- [x] Run `cargo test --test magnet`; verify behavior fails with a minimal unimplemented seam, not a compiler error.
- [x] Implement parsing with the URL parser, exact topic validation and bounded input. Reject conflicting identities and v2/hybrid topics explicitly.
- [x] Run test and full suite, commit.

## Task 2: Portable torrent path policy

Files: src/path_policy.rs, tests/path_policy.rs.

Produces `validate_components(&[&str]) -> Result<(), InputError>` and `validate_file_set(&[Vec<String>]) -> Result<(), InputError>`. Component length max 255 UTF-16 units, depth max 32; conservative case-insensitive normalized collision detection. No filesystem writes in this module.

- [x] RED: allow ordinary nested Unicode paths; reject empty, dot/dotdot, separators, absolute/drive/UNC, control characters, NTFS ADS, DOS device names (including extensions), trailing dots/spaces, depth/length excess.
- [x] RED: duplicate/case/Unicode-normalization collisions, file-vs-directory prefix conflicts and >10,000 entries rejected.
- [x] Run `cargo test --test path_policy`; verify assertion failures, implement smallest validation and bounded collision structure.
- [x] Run full suite and commit.

## Task 3: Offline CLI and public development baseline

Files: src/main.rs, tests/cli.rs, README.md, CONTRIBUTING.md, SECURITY.md, LICENSE, .github/workflows/ci.yml, docs/engine-assessment.md, docs/development-log.md.

Consumes `Magnet::parse`; command `torrent-lite inspect-magnet <MAGNET> [--json]` is explicitly offline and provisional, avoiding conflict with the eventual service `inspect <id>`. JSON schema: `{info_hash, display_name}`; no trackers or original URI. Codes: 0 success, 2 invalid input/usage. Unknown commands fail.

- [x] RED: launch real binary with valid magnet and inspect parsed JSON; invalid secret-bearing input returns 2, stderr explains category without echoing input; `download` is rejected rather than pretending to work.
- [x] Run `cargo test --test cli`, implement command, verify tests.
- [x] Add CI for Windows/Linux/macOS, fmt and clippy. Document exact current capability, development commands, security reporting limitations and remaining engine blockers.
- [x] Evaluate fixed candidate source/API: record supported and unverified behaviors, version/license/features, required experiments. Do not label source inspection as a successful live transfer test.
- [x] Verify `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`, release build and a real invocation. Fresh independent code review, fix material findings with RED/GREEN, publish branch and draft PR.

## Subsequent plans (not completion claims)

1. Engine qualification: controlled swarm, hash recovery, crash restart, storage confinement and destination filtering. If a binding security gate cannot be implemented, bring the concrete finding to the user before changing the design.
2. Service/SQLite/CLI: locks, intent reconciliation, transactional removal and bounded queues.
3. Authenticated API and MCP: schemas, authorization, startup race and recovery tests.
4. Browser UI: mockup, accessible controls, reconnection, full workflows.
5. Benchmarks/release: actual memory/throughput measurements, SBOM, provenance, full platform acceptance.
