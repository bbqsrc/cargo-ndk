# cargo-ndk - Build Rust code for Android

<table>
<tr><td>⚠️<td>The <code>main</code> branch currently only builds with rustc beta.
</table>

![CI](https://github.com/bbqsrc/cargo-ndk/actions/workflows/ci.yml/badge.svg)
![Minimum supported Rust version: 1.68](https://img.shields.io/badge/MSRV-1.68-informational)

This cargo extension handles all the environment configuration needed for successfully building libraries
for Android from a Rust codebase, with support for generating the correct `jniLibs` directory structure.

## Installing

```
cargo +beta install cargo-ndk
```

You'll also need to install all the toolchains you intend to use. Simplest way is with the following:

```
rustup +beta target add \
    aarch64-linux-android \
    armv7-linux-androideabi \
    x86_64-linux-android \
    i686-linux-android
```

Modify as necessary for your use case.

## Usage

If you have installed the NDK with Android Studio to its default location, `cargo ndk` will automatically detect
the most recent NDK version and use it. This can be overriden by specifying the path to the NDK root directory in
the `ANDROID_NDK_HOME` environment variable.

### Example: building a library for 32-bit and 64-bit ARM systems

```
cargo +beta ndk -t armeabi-v7a -t arm64-v8a -o ./jniLibs build --release -Zbuild-std
```

This specifies the Android targets to be built (ordinary triples are also supported), the output directory to use for placing the `.so` files in the layout
expected by Android, and then the ordinary flags to be passed to `cargo`.

![Example](./example/example.svg)

### Supported hosts

- Linux
- macOS (x86_64 and arm64)
- Windows

## Local development

`git clone` and then install the crate with `cargo`:

```bash
cargo +beta install --path .
```

## Similar projects

* [cargo-cocoapods](https://github.com/bbqsrc/cargo-cocoapods) - for building .a files for all Apple platforms, and bundling for CocoaPods

## License

This project is licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
