# Local download service core

Date: 2026-10-25. This plan connects the qualified engine, persistence and network boundary before exposing CLI, MCP or web control.

One foreground service owns the SQLite store, engine session, policy proxy and open download root. Each torrent has a stable application ID and a capability-scoped child directory under the chosen root. The state store is authoritative for desired running/paused/tombstone intent. On startup, restore active records; sanitize the stored original source again under the current network policy, disable DHT, UDP trackers and local discovery, and force full piece rechecking. A maximum of three downloads run concurrently; queue the rest. All engine TCP peer and HTTP(S) tracker traffic must use the authenticated proxy. Engine file I/O must use the persistent storage factory. The service never deletes downloaded content.

Mutations persist intent first. Adding records the validated source before launching the engine; an engine error remains an observable retryable state. Pausing commits paused intent before pausing the handle. Resuming commits running intent before restarting. Removing records the tombstone before detaching from the engine. Recovery must never resurrect a tombstoned item. Read-only status comes from state plus live engine metrics; do not claim bytes verified until piece hashes have been checked.

TDD sequence: capability child directory tests; local synthetic add/pause/reopen/recover test; corrupted-piece repair; forced-process termination; concurrent add/remove; then loopback API, CLI, MCP and UI. Validate Windows, Ubuntu and macOS before exposing a command as usable.

Source review found a blocker in librqbit 9.0.1: `Session::delete` currently calls `metadata.load_full().expect("TODO")`, which could panic if a magnet is removed before metadata is resolved. Do not call that path for unresolved magnets. Qualify or patch this behavior before enabling remove in the service.
