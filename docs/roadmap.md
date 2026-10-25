# Roadmap and release gates

This repository is experimental. A green CI run confirms the covered behaviors on hosted Windows, Linux and macOS runners; it does not establish production security or speed. Each milestone needs tests and a measured result before its checkbox is checked.

## Implemented foundations

- [x] Rust CLI for offline magnet and `.torrent` inspection.
- [x] Transactional SQLite intent, process lock, file identity checks and full piece recheck on service reopen.
- [x] Experimental v1 `.torrent` and HTTP(S) tracker magnet download through a bounded outbound policy proxy.
- [x] Authenticated loopback API and protected service discovery; CLI add/list/pause/resume/remove.
- [x] MCP stdio adapter and ephemeral read-only browser dashboard, subject to CI qualification.
- [x] Explicit piece recheck for `.torrent` records, subject to CI qualification.
- [x] Bounded persistent download queue and optional global bandwidth caps at service startup, subject to CI qualification.

## Required before a stable release

- [x] Store validated resolved metainfo for magnets so restart and explicit verification do not depend on a tracker being online, subject to CI qualification.
- [ ] Complete lifecycle controls: start/stop/status, queueing, concurrency and bandwidth limits, selected files, seeding policy.
- [ ] Expand browser UI to add, pause, resume and verify through a session handshake with CSRF protection; add keyboard and browser tests.
- [ ] Show measured speed, peers, error states and ETA without inventing values; provide machine-readable API errors.
- [ ] Qualify DHT, UDP trackers, private torrents and peer discovery under the outbound network policy, or document narrower compatibility.
- [ ] Test low disk space, locked files, service crash during each mutation, migration interruption and concurrent clients on native Windows.
- [ ] Fuzz untrusted inputs and obtain an independent security review.
- [ ] Publish reproducible benchmark procedure and measurements of memory, CPU and throughput on Windows, Linux and macOS.
- [ ] Publish signed or clearly marked unsigned artifacts with checksums, SBOM, dependency attribution and provenance.

Unsupported features return errors or stay disabled. There are no release binaries yet.
