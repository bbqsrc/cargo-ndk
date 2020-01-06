# cargo-ndk - Build Rust code for Android

This cargo extension handles all the environment configuration needed for successfully building libraries
for Android from a Rust codebase.

## Installing

```
cargo install cargo-ndk
```

You'll also need to install all the toolchains you intend to use. Simplest way is with the following:

```
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android 
```

## Usage

You'll also need the NDK installed somewhere, and the path to it exported as the `ANDROID_NDK_HOME` environment variable. If you use
Android Studio and have `ANDROID_SDK_HOME` defined, `cargo ndk` is smart enough to detect the `ndk-bundle` subdirectory as well. 

Building is very similar to any other Rust project, with the addition of the `--platform` flag for selecting which
Android API platform to target. By default, `21` is a good choice.

```
cargo ndk --platform 21 --target x86_64-linux-android build
```

Add `--release` for a release build, as per usual.

**NOTE: Minimum supported NDK version is r19c. Has been tested up to r21.**

### Supported triples

- `aarch64-linux-android`
- `armv7-linux-androideabi`
- `i686-linux-android`
- `x86_64-linux-android`

### Supported hosts

- Linux
- macOS
- Windows

## Similar projects

* [cargo-lipo](https://github.com/TimNN/cargo-lipo) - for building iOS/macOS universal Rust libraries

## License

This project is licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
