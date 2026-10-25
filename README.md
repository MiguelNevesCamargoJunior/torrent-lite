# torrent-lite

A lightweight, local-first BitTorrent client in Rust, designed for Windows and portable to Linux/macOS.

**Early development. Not a usable torrent downloader yet. No releases or performance claims.**

The approved design targets one download service with a CLI, a local browser UI and an MCP stdio adapter. No advertisements, bundled offers, telemetry or automatic execution of downloaded files.

## Development status

The first milestone establishes input validation and an offline inspection CLI, plus an explicit compatibility/security gate for the proposed `librqbit` engine. Downloading, persistence, MCP and the web UI are subsequent milestones; they are not implemented by the initial foundation.

- [Design](docs/superpowers/specs/2026-10-25-torrent-design.md) (Portuguese)
- [Initial implementation plan](docs/superpowers/plans/2026-10-25-foundation.md)
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

The library validates portable path components and rejects file/directory collisions. These are lexical checks, not filesystem confinement. Actual downloads are not available yet.

## License

Apache-2.0. Dependencies retain their respective licenses and attribution.

