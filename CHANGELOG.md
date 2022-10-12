## v2.12.2 - 2022-10-12

- Fix: Revert `__ANDROID_API__` changes from v2.12.0.

## v2.12.1 - 2022-09-27

- Fix: `build --profile <foo>` is now supported.

## v2.12.0 - 2022-09-27

This release clarifies that the MSRV is `1.56`. This is confirmed by CI.

- Fix: removed use of format strings in order to support older Rust releases
- Enhancement: define `__ANDROID_API__` in `CFLAGS` and `CXXFLAGS` (thanks @Zoxc)
- Enhancement: updated dependencies

## v2.11.0 - 2022-08-04

- Fix: remove automatic adding of bindgen flags (use `--bindgen` as expected)
- Enhancement: consider all widely-used NDK/SDK env vars (thanks @rib)

## v2.10.1 - 2022-07-24

- Enhancement: updated dependencies

## v2.10.0 - 2022-07-24

- Fix: support NDK 23 and higher with libgcc workaround (thanks @rib)

## v2.9.0 - 2022-05-24

- Fix: better bindgen handling (thanks @lattice0)

## v2.8.0 - 2022-05-07

- Fix: missing NDK now exits with exit code 1 (thanks @complexspaces)
- Enhancement: more intelligent handling of manifest context (thanks @complexspaces)

## v2.7.0 - 2022-03-22

- Fix: now works with NDK 23 and maybe up. Maybe. Google do be that company, yo.

## v2.6.0 - 2022-03-10

- Enhancement: added `--bindgen` flag for adding relevant environment variables for bindgen. (thanks @mkpowers and @x3ro)

## v2.5.0 - 2021-11-09

- Fix: `-v` shows version now.

## v2.4.1 - 2021-07-19

- Fix: Expose `CARGO_NDK_ANDROID_PLATFORM` to subprocesses. (thanks @DoumanAsh)

## v2.4.0 - 2021-07-19

- Fix: `ANDROID_NDK_HOME` will now try to resolve the highest version in the given directory before falling back to literal path. (thanks @dnaka91)

## v2.3.0 - 2021-04-20

- Fix: Workspaces no longer cause build failures
- Enhancement: Added `CARGO_NDK_CMAKE_TOOLCHAIN_PATH` and `CARGO_NDK_ANDROID_TARGET` environment variable exports

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

## v0.6.2 - 2020-03-05

- Add `CXX` environment variables (thanks @remyers)

