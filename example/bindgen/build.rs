use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-link-lib=m");

    let bindings = bindgen::Builder::default()
        .header_contents("math.h", "#include <math.h>")
        .allowlist_function("sin")
        .allowlist_function("cos")
        .allowlist_function("sqrt")
        .allowlist_function("pow")
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
