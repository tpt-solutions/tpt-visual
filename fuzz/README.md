# Fuzzing

Targets for [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz):

- `parse_cube` — the `.cube` LUT parser (`tpt-av-visual-color::luts::parse_cube`).
- `session_json` — `Session` JSON deserialization, with a lossless
  re-serialize/re-parse assertion on success.

## Running

On Linux/macOS:

```sh
cargo install cargo-fuzz
cd fuzz
cargo +nightly fuzz run parse_cube -- -max_total_time=60
```

## Windows note

cargo-fuzz's ASAN runtime is not bundled with Windows MSVC toolchains; fuzz
binaries build but fail with `STATUS_DLL_NOT_FOUND` unless the LLVM ASAN
runtime DLL is installed and on `PATH`. Run fuzz sessions on Linux (or the
`fuzz-smoke` CI job), or install the MSVC "C++ Clang tools for Windows"
component.
