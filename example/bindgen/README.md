# Bindgen Example for Android

Minimal example demonstrating `bindgen` with `cargo-ndk` to generate Rust bindings for standard C math functions.

## What this shows

- Using `bindgen` to bind against standard C library functions (`sin`, `cos`, `sqrt`, `pow`)
- Building a library crate with bindgen for Android targets
- Linking against system libraries (`libm`)

## Building

```bash
# Build for Android target
cargo ndk --bindgen --target arm64-v8a build

# Run tests
cargo test
```
