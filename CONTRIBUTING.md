# Contributing to tpt-visual

Thank you for contributing to `tpt-visual`! This document describes the
licensing agreement and the engineering rules every contribution must
follow.

## Licensing agreement

`tpt-visual` is dual-licensed under the MIT license and the Apache License
2.0 ([LICENSE-MIT](LICENSE-MIT) / [LICENSE-APACHE](LICENSE-APACHE)). By
contributing to tpt-visual, you agree that:

1. **Your code will be licensed under the same dual MIT OR Apache-2.0
   terms.** You retain copyright to your contributions; no copyright
   assignment is required.
2. **You will not introduce any dependency that is GPL, LGPL, AGPL, or MPL
   licensed** — anywhere in the dependency tree. This is enforced
   automatically by [`cargo-deny`](https://embarkstudios.github.io/cargo-deny/)
   (`deny.toml`, `cargo deny check` in CI); the allow-list is MIT,
   Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, and Zlib, and unlicensed
   crates are denied.
3. **All GPU shaders must be written in WGSL** (WebGPU Shading Language) —
   no HLSL/GLSL sources or cross-compilation toolchains.
4. **All color science code must be validated against reference
   implementations** (e.g. ACES Central specifications, OpenColorIO
   reference configs, ITU-R BT.2100 / SMPTE ST 2084 anchor values). New
   transfer functions, gamut conversions, or tone mappers must ship tests
   with published anchors or double-precision cross-checks.

## Dependency rules

- Prefer `std` and the existing workspace dependencies.
- New dependencies must be permissively licensed (see above), actively
  maintained, and pull their weight. Check the transitive tree with
  `cargo tree` before proposing.
- H.264/AV1 decoding lives behind features (`kinetix`) so the core stays
  buildable without patent-encumbered codecs.

## Engineering rules

- **Edition/Rust**: edition 2021, `rust-version = "1.75"`.
- **Correctness**: every crate ships unit tests (`src` `#[cfg(test)]`) and
  integration tests (`tests/`). GPU-dependent tests must skip gracefully
  when no adapter is available (see `GpuContext::headless`).
- **Quality gates** (all must pass before merge):

  ```sh
  cargo build --workspace
  cargo test --workspace
  cargo fmt --check
  cargo clippy --workspace -- -D warnings
  cargo deny check
  ```

  One-shot: `just gate` (see the `justfile`; `cargo lint` /
  `cargo check-fmt` aliases also work).

- **Style**: run `cargo fmt`; keep `clippy` clean with no allowed-by-default
  lint suppressions unless justified in a comment.
- **Docs**: public items get doc comments with examples where they clarify;
  keep `README.md` and `DESIGN.md` in sync with the implemented API surface.

## Submitting changes

1. Fork / branch from `master`.
2. Make your change with tests.
3. Run the quality gates above.
4. Open a pull request describing the *what* and the *why*.

For bug reports, please include a minimal reproduction (a test case is
best) and your GPU/driver/OS details for rendering issues.

## Code of Conduct

Be excellent to each other. Critique code, not people.
