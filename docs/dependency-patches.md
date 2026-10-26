# Dependency patches

## `librqbit-utp` 0.7.0

The upstream crate currently requires `ringbuf` 0.4.x, which contains the panic-safety issue documented in [RUSTSEC-2026-0293](https://rustsec.org/advisories/RUSTSEC-2026-0293). The crate is vendored at `vendor/librqbit-utp` under its Apache-2.0 license, with only its `ringbuf` requirement changed to 0.5.3. The latter includes the upstream fix. Its runtime source is otherwise unchanged.

The vendored dependency exists so the application can retain librqbit's engine without accepting the affected ring buffer version while the upstream crate updates its requirement. The root lockfile pins the resolved ring buffer version. `cargo tree --features service-core -i ringbuf` and the dependency-policy workflow verify that the engine resolves to ringbuf 0.5.3.

The engine also uses librqbit's `rust-tls` feature and its `sha1-ring` backend. This avoids the unmaintained `crypto-hash` dependency; the selected backend is provided by AWS-LC. `deny.toml` and the dependency workflow check both the application and fuzz harness for advisories, yanked versions, licenses, wildcard requirements and unexpected registries.
