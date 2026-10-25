# Engine assessment — librqbit 9.0.1

Date: 2026-10-25. Status: **candidate; not qualified for production integration**.

Evidence: `cargo info librqbit@9.0.1` fetched the published package. Its declared license is Apache-2.0. Default features include default-tls and http-api-client; disable unnecessary features when adding the engine. The engine is intentionally not yet a production dependency of the offline foundation.

## Inspected source, not runtime proof

| Requirement | Observation in published 9.0.1 source | Remaining qualification |
|---|---|---|
| Pause/resume, metadata and state | Session methods expose these concepts | Controlled transfer, crash and recovery tests |
| Forced integrity on restart | SessionOptions exposes fastresume | Prove existing-piece validation and selective repair |
| Per-torrent peer cap | Session peer_limit feeds torrent options | Prove bounded buffers and implement a global cap |
| Private torrents | session.rs disables DHT/LSD for known-private metadata; live state checks private before PEX | Magnet metadata transition and tracker failover tests; private torrents rejected until qualified |
| Storage confinement | filesystem/fs.rs init builds paths, creates parent directories and opens using std::fs::OpenOptions | `SafeRoot` creates and reopens files relative to an open capability, with identity and link checks; the feature-gated test uses a custom StorageFactory, but production persistence and adversarial race qualification remain required |
| Outbound filtering | live peer path uses blocklist/allowlist; session builds its own reqwest client | These observations do not establish tracker DNS/redirect filtering; connector strategy or upstream extension required |

This is a compatibility assessment against this project's stricter design, **not a vulnerability claim about upstream**. A public API inventory alone cannot prove the absence of protection elsewhere.

## Next experiments

The first transfer experiment lives in `tests/engine_local.rs`, gated by `engine-qualification`. It creates a synthetic file and runs two sessions on loopback with public discovery disabled. Hosted Windows CI run [40205597701](https://github.com/MiguelNevesCamargoJunior/torrent-lite/actions/runs/40205597701) passed the initial transfer test. Hosted [run 40205928188](https://github.com/MiguelNevesCamargoJunior/torrent-lite/actions/runs/40205928188) passed the restart-after-corruption extension on Windows, Ubuntu and macOS. Hosted [run 40206759591](https://github.com/MiguelNevesCamargoJunior/torrent-lite/actions/runs/40206759591) passed the first transfer through a custom capability-scoped storage adapter on all three. The next change exercises identity-checked reopening through the same adapter and still needs CI. Local Windows compilation is blocked before the test runs because this machine's Visual Studio C++ installation lacks `vcruntime.h` (AWS-LC native dependency). These tests prove recovery for one single-file loopback fixture, not crash recovery or selective piece fetching under production conditions.

1. Create a test-only adapter with discovery disabled and explicit loopback peers. Transfer a synthetic payload and assert bytes and piece hashes.
2. Stop/restart with existing data; corrupt a cross-file piece, verify and assert only invalid data is fetched again.
3. Persist identities in an application database, then qualify the custom StorageFactory with Windows junction replacement and POSIX symlink races before allowing engine writes to untrusted paths. ReFS uses 128-bit file IDs; the current 64-bit `cap-fs-ext` identity is insufficient there and must fail closed or be replaced.
4. Prove all tracker/peer connection routes apply the destination policy after DNS and each redirect. A peer blocklist alone is insufficient.
5. Bound metadata parsing, buffers, init concurrency and aggregate peers; verify settings under load.
6. Review the selected dependency graph and licenses; build with fixed features; then measure actual resource usage.

If the engine cannot satisfy the binding design without substantial changes, document the exact gap and evaluate upstream contribution or another engine with the user. Do not silently weaken the design or claim download functionality.

Sources: https://docs.rs/librqbit/9.0.1/librqbit/ and the published crate's src/session.rs, src/storage/filesystem/fs.rs, src/torrent_state/live/mod.rs. Upstream: https://github.com/ikatson/rqbit.

