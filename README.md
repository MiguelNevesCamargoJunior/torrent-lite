# torrent-lite

A lightweight, local-first BitTorrent client in Rust, designed for Windows and portable to Linux/macOS.

**Early development. The download command is experimental; there are no releases or performance claims.**

The approved design targets one download service with a CLI, a local browser UI and an MCP stdio adapter. No advertisements, bundled offers, telemetry or automatic execution of downloaded files.

## Development status

The current foundation establishes input validation, an offline inspection CLI and a qualified `librqbit` engine. A capability-scoped helper opens files inside a chosen root. A feature-gated SQLite store persists torrent intent, tombstones and file identities; it can migrate its v1 schema after creating a backup. An experimental foreground service uses those identities, rechecks pieces on restart, and routes outbound TCP through a local authenticated policy proxy. An experimental CLI command downloads local `.torrent` files. Magnet downloading, an MCP server and a web UI are subsequent milestones.

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

The library validates portable path components, rejects file/directory collisions and opens files relative to an opened root directory. Existing files require a recorded file identity before reopening. The feature-gated service passed a synthetic pause, restart and tracker download test on Windows, Linux and macOS. Forced-process crash testing, queue limits and public swarm performance qualification are pending.

## Experimental foreground download

Create an output directory, then run this command with a local v1 `.torrent` file and a state database path. It stays open until the torrent completes. Interrupting it leaves recorded intent for a later run. This interface is under active development and has no release build yet.

```sh
cargo run --locked --features service-core -- download-torrent path/to/file.torrent --output path/to/downloads --state path/to/state.db
```

Files for each torrent are placed under a directory named by the service ID inside the output root. Only HTTP(S) trackers and proxied TCP peers are supported at this stage; DHT, UDP trackers and local discovery are disabled.

## License

Apache-2.0. Dependencies retain their respective licenses and attribution.

