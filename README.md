# torrent-lite

A lightweight, local-first BitTorrent client in Rust, designed for Windows and portable to Linux/macOS.

**Early development. Downloads and integrations are experimental; there are no releases or performance claims.**

The approved design targets one download service with a CLI, a local browser UI and an MCP stdio adapter. No advertisements, bundled offers, telemetry or automatic execution of downloaded files.

## Development status

The current foundation establishes input validation, an offline inspection CLI and a qualified `librqbit` engine. A capability-scoped helper opens files inside a chosen root. A feature-gated SQLite store persists torrent intent, tombstones and file identities; it can migrate its v1 schema after creating a backup. An experimental foreground service uses those identities, rechecks pieces on restart, and routes outbound TCP through a local authenticated policy proxy. Experimental CLI commands download local `.torrent` files and v1 magnets with HTTP(S) trackers. A loopback-only API and owner-scoped endpoint discovery support commands for adding, listing, pausing, resuming and removing torrents. An MCP stdio adapter and local browser dashboard are now experimental.

- [Design](docs/superpowers/specs/2026-10-25-torrent-design.md) (Portuguese)
- [Initial implementation plan](docs/superpowers/plans/2026-10-25-foundation.md)
- [State store plan](docs/superpowers/plans/2026-10-25-state-store.md)
- [Engine assessment](docs/engine-assessment.md)
- [Contributing](CONTRIBUTING.md)
- [Security](SECURITY.md)
- [Roadmap and release gates](docs/roadmap.md)

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

The library validates portable path components, rejects file/directory collisions and opens files relative to an opened root directory. Existing files require a recorded file identity before reopening. The feature-gated service passed a synthetic pause, restart and tracker download test on Windows, Linux and macOS. Forced-process crash testing, bandwidth limits and public swarm performance qualification are pending.

## Experimental foreground download

Create an output directory, then run this command with a local v1 `.torrent` file and a state database path. It stays open until the torrent completes. Interrupting it leaves recorded intent for a later run. This interface is under active development and has no release build yet.

```sh
cargo run --locked --features service-core -- download-torrent path/to/file.torrent --output path/to/downloads --state path/to/state.db
```

For a v1 magnet containing an HTTP(S) tracker:

```sh
cargo run --locked --features service-core -- download-magnet "magnet:?xt=urn:btih:HASH&tr=https%3A%2F%2Ftracker.example%2Fannounce" --output path/to/downloads --state path/to/state.db
```

Files for each torrent are placed under a directory named by the service ID inside the output root. Only HTTP(S) trackers and proxied TCP peers are supported at this stage; DHT, UDP trackers and local discovery are disabled.

The experimental foreground service can also be started separately with:

```sh
cargo run --locked --features local-api -- serve --output path/to/downloads --state path/to/state.db --max-active 3
```

Add `--download-limit-kib 512` or `--upload-limit-kib 128` to cap global throughput. Omit either flag for unlimited speed in that direction. Values must be at least 16 KiB/s; see [bandwidth behavior](docs/bandwidth-spec.md). The service queues downloads beyond `--max-active` in registration order. Completed downloads release a slot and are paused; see [queue behavior](docs/queue-spec.md).

If a torrent cannot activate, its record stays visible as `retrying` with a bounded retry delay. Other queued torrents can still start. The API exposes a generic error code and the delay without revealing tracker URLs or local paths.

The local API returns stable JSON error codes for failed requests; see the [error contract](docs/error-contract.md). Errors do not echo supplied magnets, file paths or authorization tokens.

It listens only on `127.0.0.1`, publishes a protected endpoint record beside the state database, and stops on Ctrl+C. In another terminal, use the same state database path:

```sh
cargo run --locked --features api-client -- add "magnet:?xt=urn:btih:HASH&tr=https%3A%2F%2Ftracker.example%2Fannounce" --state path/to/state.db
cargo run --locked --features api-client -- add path/to/file.torrent --state path/to/state.db
cargo run --locked --features api-client -- list --state path/to/state.db
cargo run --locked --features api-client -- files TORRENT_ID --state path/to/state.db
cargo run --locked --features api-client -- select-files TORRENT_ID 0 2 --state path/to/state.db
cargo run --locked --features api-client -- select-files TORRENT_ID --all --state path/to/state.db
cargo run --locked --features api-client -- pause TORRENT_ID --state path/to/state.db
cargo run --locked --features api-client -- resume TORRENT_ID --state path/to/state.db
cargo run --locked --features api-client -- verify TORRENT_ID --state path/to/state.db
cargo run --locked --features api-client -- remove TORRENT_ID --state path/to/state.db
```

`add` accepts `--paused`. `files` shows zero-based file indices; `select-files` requires a paused torrent and available metainfo. Choices survive restarts, and `--all` restores every file; see the [selection contract](docs/selected-files-spec.md). `verify` fully rechecks registered `.torrent` data and magnets whose metainfo has resolved, then restores the prior pause/running intent. Resolved magnet metainfo is cached in the state database so a restarted service can recheck without recontacting a tracker. Removing a record preserves downloaded files. For a local dashboard, leave the service running and start:

```sh
cargo run --locked --features browser-ui -- ui --state path/to/state.db
```

Open the local URL printed by `ui`. The dashboard refreshes every two seconds, shows verified progress and available engine speed/peer estimates, and lets you add magnets or `.torrent` files, choose files while paused, pause, resume, verify and remove records. Removing a record preserves downloaded files. Unknown values appear as unavailable. Closing `ui` does not stop downloads. The URL gives access to local controls while the `ui` process runs, so treat it as private. Browser mutations require a separate temporary CSRF token and exact same-origin requests; see [UI security design](docs/ui-plan.md).

For MCP clients that launch a stdio command:

```sh
cargo run --locked --features mcp -- mcp --state path/to/state.db --read-only
```

Omit `--read-only` to expose add-magnet, file selection, pause, resume and remove tools. File listing works in read-only mode. Standard output is reserved for MCP protocol messages. The service must already be running.

## License

Apache-2.0. Dependencies retain their respective licenses and attribution.

