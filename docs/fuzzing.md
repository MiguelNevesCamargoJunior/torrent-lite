# Untrusted input fuzzing

The `untrusted_inputs` libFuzzer target feeds arbitrary bytes to `.torrent` inspection and policy sanitization, or UTF-8 text to magnet parsing and policy sanitization. When sanitization succeeds, it checks that the output remains parseable and retains the original content hash and file list (or magnet hash). The checked-in corpus contains a valid v1 torrent and a tracker magnet. Crashes and invariant failures become reproducers under `fuzz/artifacts/`.

The normal CI runs a deterministic mutation regression test on Windows, Linux and macOS. The [fuzz workflow](https://github.com/MiguelNevesCamargoJunior/torrent-lite/actions/workflows/fuzz.yml) runs libFuzzer for two minutes on a hosted Linux runner every week and on demand. This is a limited smoke campaign, not proof of parser safety or an independent security review.

To run the same target locally on Linux with a nightly Rust toolchain and `cargo-fuzz` 0.13.2:

```sh
cargo +nightly fuzz run untrusted_inputs -- -max_total_time=120 -max_len=1048576
```

For a found input, reproduce it with `cargo +nightly fuzz run untrusted_inputs fuzz/artifacts/untrusted_inputs/<file>`; add a minimal nonsecret regression case, fix the cause, then rerun the target. Do not submit third-party torrent data or private tracker URLs as corpus files.

The 1 MiB fuzz cap explores a subset of the 8 MiB `.torrent` input limit. Network, file system, peer protocol and service state are outside this target; they require separate tests and review.
