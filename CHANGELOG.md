## v2.3.0 - 2021-04-20

- Fix: Workspaces no longer cause build failures
- Enhancement: Added CARGO_NDK_CMAKE_TOOLCHAIN_PATH and CARGO_NDK_ANDROID_TARGET environment variable exports

## v2.2.0 - 2021-02-02

- Fix: Return support for Rust-style triples to the target command line argument (the new behaviour also remains)

## v2.1.0 - 2021-01-12

- Fix: Handle --manifest-path correctly (thanks @ubamrein)
- Enhancement: Update some help text phrasing and general ergonomics of output

## v2.0.0 - 2021-01-09

- **Breaking change**: most command line parameters have changed in some way, see the README for current usage.
- Feature: Added auto-detection of NDK where available
- Feature: Specify all build targets at once
- Feature: Output built libraries to `jniLibs`-formatted directory layout
- Enhancement: Better error handling in general, better messages

## v1.0.0 - 2020-03-15

- No changes, just guaranteeing stability of the command line interface. :)

## v0.6.2 — 2020-03-05

- Add `CXX` environment variables (thanks @remyers)

