fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_NDK_ON_ANDROID");

    if std::env::var("CARGO_NDK_ON_ANDROID").is_ok() {
        println!("cargo:rustc-cfg=cargo_ndk_on_android");
    }

    println!("cargo:rustc-check-cfg=cfg(cargo_ndk_on_android)");
}
