# Vendored timer source

This directory contains `wasmtimer` 0.4.3 from its published crate archive:

- Archive: <https://static.crates.io/crates/wasmtimer/wasmtimer-0.4.3.crate>
- SHA-256: `1c598d6b99ea013e35844697fc4670d08339d5cda15588f193c6beedd12f644b`
- Upstream commit: `fecc90e04b1884ac6298af2cdcd2ef3f8e0141de`
- License: MIT; `LICENSE` and source copyright notices are retained.

`Interval.sleep` and `Timeout.future`/`delay` use `pin-project-lite` structural
projections. The timer API, scheduling logic, features and upstream test bodies
are preserved. The additive projection tests exercise a future that is not
`Unpin`, timeout completion, ownership and the existing accessor/`Unpin` contract.

The replacement follows [RUSTSEC-2026-0441](https://rustsec.org/advisories/RUSTSEC-2026-0441.html),
which identifies `pin-utils` as unmaintained and recommends structural projection
libraries for these field projections.

The application root `Cargo.lock` governs this path dependency and retains
`pin-project-lite` 0.2.17. The registry archive's standalone development lock is
not an application input and is not copied into this directory. That archived
lock also includes an older `futures-util` with a `pin-utils` dependency; its
source digest remains in the change provenance.

The root workspace explicitly excludes this directory. The platform-specific
dependency can still resolve through the root `[patch.crates-io]` entry; the
vendored package is not added to ordinary native workspace test selection.

`tests/web.rs` is retained unchanged. The `pin_projection` test target requires
`tokio-test-util`, matching its use of the timer's paused-clock API. WebAssembly
validation runs both integration targets with `tokio-test-util,serde` enabled.
