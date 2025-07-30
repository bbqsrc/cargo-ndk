## Changelog

### v4.0.0-beta.2 - 2025-07-30

This version of `cargo-ndk` is much closer to a thin passthrough to `cargo` than previous versions. Please take note of the breaking changes.

It introduces a new command, `cargo ndk-test`. This will push your tests to a device via adb and run them on-device. It currently has 
a limitation that doctests can still only be run on the host.

It also introduces a `cargo ndk-runner` subcommand (which is used by `ndk-test` under the hood) and can be specified as a `runner` in Cargo's `config.toml` to enable `cargo run` to work with Android binaries.

> [!IMPORTANT]
> New MSRV: **1.86**. This means that **to build `cargo-ndk`** you need at least 1.86. You can still build projects targetting older versions of Rust with this release of `cargo-ndk`.

- **Breaking change**: Replaced gumdrop CLI parsing with clap. This is functionally equivalent but may cause some minor behavioral differences in edge cases
- **Breaking change**: No longer strips build output by default, and `--no-strip` option is removed
- **Breaking change**: Removed Cargo.toml-based configuration property handling
- **Breaking change**: Bump minimum supported Rust version (MSRV) to 1.86 and use Rust edition 2024.
- **Breaking change**: `--bindgen` has been removed, and relevant environment variables are set by default
- Feature: Clang builtins can now be linked automatically by adding `--link-builtins` or its equivalent environment variable
- Feature: `cargo ndk-test` for running tests on Android devices.
- Feature: `cargo ndk-runner` for running binaries on Android devices.
- Enhancement: Can now be built for an Android host (i.e. Termux) if built with `CARGO_NDK_ON_ANDROID` environment variable set
- Enhancement: CLI flags can now be used in any order (e.g., `cargo ndk -t x86 build` and `cargo ndk build --target x86` are equivalent).
- Enhancement: Added environment variable support for configuration flags with `CARGO_NDK_` prefix (e.g., `CARGO_NDK_TARGET`, `CARGO_NDK_PLATFORM`, `CARGO_NDK_OUTPUT_DIR`)
- Enhancement: Target flag now supports comma-delimited lists for specifying multiple targets
- Enhancement: Added cargo-ndk version information to very verbose (`-vv`) output.
- Fix: Fixed issue where `--manifest-path` and other flags using `--flag=value` format could be passed to cargo twice.

### v3.5.7 - 2024-08-19

- Fix: canonicalize output directory path for `CARGO_NDK_OUTPUT_PATH`, fixes build scripts not at workspace root 
- Enhancement: output directory creation error now prints error message instead of panicking

### v3.5.6 - 2024-05-20

- Fix: a type was optional, then wasn't, and now is optional again.

### v3.5.5 - 2024-05-15

- Fix: use correct path on Linux
- Fix: only copy libraries being built

### v3.5.4 - 2024-04-13

- Fix: add compile error if attempted to build for unsupported target OSes (please stop trying to build cargo-ndk *for* Android. Makes no sense.)
- Fix: remove underscore prefixed env vars from `ndk-env`

### v3.5.3 - 2024-04-12

- Enhancement: add usage instructions to the ndk-env output
- Enhancement: add `--powershell` flag to ndk-env

### v3.5.2 - 2024-04-11

- Fix: make `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER` correct

### v3.5.1 - 2024-04-11

- Fix: make the exports from `ndk-env` use underscores

### v3.5.0 - 2024-04-10

- Enhancement: added `ndk-env` command for printing env vars for use with things like rust-analyzer.


Please note the MSRV for _building_ `cargo-ndk` is now 1.73 due to dependency churn.

### v3.4.0 - 2023-09-23

- Enhancement: add additional environment variables for sysroot directories (see README.md)

### v3.3.0 - 2023-08-20

- Enhancement: console output now uses `cargo`'s formatter (and looks prettier)
- Enhancement: panics will print a dump for simplifying bug reports

`RUST_LOG` is therefore now ignored. Use `cargo`'s usual `-v` and `-vv` for verbosity control.

### v3.2.2 - 2023-08-14

- Fix: pass CFLAGS, CXXFLAGS and related variables as per `cc` crate behaviour

### v3.2.1 - 2023-07-31

- Fix: pass CFLAGS and CXXFLAGS to cargo correctly (thanks @rib)

### v3.2.0 - 2023-05-24

- Fix: linker workaround made more robust and fixing too many args issues (thanks @rib)

### v3.1.2 - 2023-05-12

- Enhancement: use `OUT_DIR` to hold the `cargo-ndk` wrapper executables on Windows (thanks @ScSofts)

### v3.1.1 - 2023-05-10

- Fix: use `raw_args` for the Windows workaround subprocesses because random quotation marks still leak in

### v3.1.0 - 2023-05-09

- Workaround: NDK r25 on Windows does not work with Rust (https://github.com/bbqsrc/cargo-ndk/issues/92, https://github.com/android/ndk/issues/1856). `cargo-ndk` works around this by filtering the arguments before being passed to the NDK build scripts.

I wish everyone a very good day, except Google.

### v3.0.1 - 2023-03-24

- Fix: specifying `--profile dev` will now look in the `debug` target directory as expected.

### v3.0.0 - 2023-03-11

`libgcc` will no longer be linked against resultant libraries, and the workaround code in `cargo-ndk` has been removed.

See https://blog.rust-lang.org/2023/01/09/android-ndk-update-r25.html for more information.

- **Breaking change**: minimum supported version of Rust is now 1.68. 
- Enhancement: added `RANLIB` environment variables

### v2.12.6 - 2023-02-01

- Fix: stop `cargo_metadata` from downloading the entire world for no reason.

### v2.12.5 - 2023-02-01

- Fix: Handle bindgen clang arguments on Windows.

### v2.12.4 - 2023-01-24

- Fix: Handle `CARGO_ENCODED_RUSTFLAGS` and `RUSTFLAGS` correctly.

### v2.12.3 - 2023-01-22

- Fix: add missing Cargo.lock file.

### v2.12.2 - 2022-10-12

- Fix: Revert `__ANDROID_API__` changes from v2.12.0.

### v2.12.1 - 2022-09-27

- Fix: `build --profile <foo>` is now supported.

### v2.12.0 - 2022-09-27

This release clarifies that the MSRV is `1.56`. This is confirmed by CI.

- Fix: removed use of format strings in order to support older Rust releases
- Enhancement: define `__ANDROID_API__` in `CFLAGS` and `CXXFLAGS` (thanks @Zoxc)
- Enhancement: updated dependencies

### v2.11.0 - 2022-08-04

- Fix: remove automatic adding of bindgen flags (use `--bindgen` as expected)
- Enhancement: consider all widely-used NDK/SDK env vars (thanks @rib)

### v2.10.1 - 2022-07-24

- Enhancement: updated dependencies

### v2.10.0 - 2022-07-24

- Fix: support NDK 23 and higher with libgcc workaround (thanks @rib)

### v2.9.0 - 2022-05-24

- Fix: better bindgen handling (thanks @lattice0)

### v2.8.0 - 2022-05-07

- Fix: missing NDK now exits with exit code 1 (thanks @complexspaces)
- Enhancement: more intelligent handling of manifest context (thanks @complexspaces)

### v2.7.0 - 2022-03-22

- Fix: now works with NDK 23 and maybe up. Maybe. Google do be that company, yo.

### v2.6.0 - 2022-03-10

- Enhancement: added `--bindgen` flag for adding relevant environment variables for bindgen. (thanks @mkpowers and @x3ro)

### v2.5.0 - 2021-11-09

- Fix: `-v` shows version now.

### v2.4.1 - 2021-07-19

- Fix: Expose `CARGO_NDK_ANDROID_PLATFORM` to subprocesses. (thanks @DoumanAsh)

### v2.4.0 - 2021-07-19

- Fix: `ANDROID_NDK_HOME` will now try to resolve the highest version in the given directory before falling back to literal path. (thanks @dnaka91)

### v2.3.0 - 2021-04-20

- Fix: Workspaces no longer cause build failures
- Enhancement: Added `CARGO_NDK_CMAKE_TOOLCHAIN_PATH` and `CARGO_NDK_ANDROID_TARGET` environment variable exports

### v2.2.0 - 2021-02-02

- Fix: Return support for Rust-style triples to the target command line argument (the new behaviour also remains)

### v2.1.0 - 2021-01-12

- Fix: Handle --manifest-path correctly (thanks @ubamrein)
- Enhancement: Update some help text phrasing and general ergonomics of output

### v2.0.0 - 2021-01-09

- **Breaking change**: most command line parameters have changed in some way, see the README for current usage.
- Feature: Added auto-detection of NDK where available
- Feature: Specify all build targets at once
- Feature: Output built libraries to `jniLibs`-formatted directory layout
- Enhancement: Better error handling in general, better messages

### v1.0.0 - 2020-03-15

- No changes, just guaranteeing stability of the command line interface. :)

### v0.6.2 - 2020-03-05

- Add `CXX` environment variables (thanks @remyers)

