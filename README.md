# torrent-lite

A lightweight, local-first BitTorrent client in Rust, designed for Windows and portable to Linux/macOS.

**Early development. Not a usable torrent downloader yet. No releases or performance claims.**

The approved design targets one download service with a CLI, a local browser UI and an MCP stdio adapter. No advertisements, bundled offers, telemetry or automatic execution of downloaded files.

## Development status

The current foundation establishes input validation and an offline inspection CLI, plus an explicit compatibility/security gate for the proposed `librqbit` engine. A capability-scoped helper opens files inside a chosen root, and a feature-gated local-peer test exercises the engine. A feature-gated SQLite store persists torrent intent, tombstones and file identities; it can migrate its v1 schema after creating a backup. Downloading through the CLI, a running service, MCP and the web UI are subsequent milestones.

- [Design](docs/superpowers/specs/2026-10-25-torrent-design.md) (Portuguese)
- [Initial implementation plan](docs/superpowers/plans/2026-10-25-foundation.md)
- [State store plan](docs/superpowers/plans/2026-10-25-state-store.md)
- [Engine assessment](docs/engine-assessment.md)
- [Contributing](CONTRIBUTING.md)
- [Security](SECURITY.md)

## Principles

Integrity before fast resume; bounded memory before unlimited concurrency; tests before implementation. Torrent hashes validate content against metadata, not against malware. Performance targets are hypotheses until measured under published conditions.

## Try the current offline CLI

Requires Rust 1.91.1 or later. This command parses a synthetic magnet and makes no network connection:

```sh
cargo run --locked -- inspect-magnet "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213&dn=Example" --json
```

Inspect a local v1 metainfo file without starting a download:

```sh
cargo run --locked -- inspect-torrent path/to/file.torrent --json
```

The library validates portable path components, rejects file/directory collisions and opens files relative to an opened root directory. Existing files require a recorded file identity before reopening. The SQLite store persists those identities, but the engine adapter has not been connected to it yet. The local-peer experiment and store remain separate building blocks; there is no production network destination policy. Actual downloads through the CLI are not available yet.

## License

Apache-2.0. Dependencies retain their respective licenses and attribution.

