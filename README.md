# cargo-ndk

Use `cargo` with the NDK without too much hassle. Handles finding the correct linkers and converting between
the triples used in the Rust world to the triples used in the Android world.

For a more manual approach, see [this blog from Mozilla](https://mozilla.github.io/firefox-browser-architecture/experiments/2017-09-21-rust-on-android.html).

**NOTE: Minimum supported NDK version is r19c.**

### Supported triples

- `aarch64-linux-android`
- `armv7-linux-androideabi`
- `i686-linux-android`
- `x86_64-linux-android`

### Supported hosts

- Linux
- macOS
- Windows

## Usage

First you'll need to install all the toolchains you intend to use. Simplest way is with the following:

```
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android
```

You'll also need the NDK installed somewhere, and the path to it exported as the `NDK_HOME` environment variable. On macOS with Android Studio, this is usually `$HOME/Library/Android/sdk/ndk-bundle`. On Linux, it's somewhere in `$HOME/.android` (pull requests accepted with actual location).

Install the plugin with `cargo install cargo-ndk`.

Then, simply run your usual cargo commands prefixed with `cargo ndk --target <Android triplet> --android-platform <API> --`, where
API is the Android API platform version to target (for example, 16). Note the `--`, it is required to pass commands to `cargo` and not to the `cargo-ndk` plugin.

So, to do an ordinary release build for `aarch64` against API 25, you'd run:

```
cargo ndk --target aarch64-linux-android --android-platform 25 -- build --release 
```

## License

This project is licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
