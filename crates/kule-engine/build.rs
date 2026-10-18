fn main() {
    println!("cargo:rerun-if-changed=src/listeners.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/listeners.c")
            .warnings(true)
            .compile("kulelisteners");
    }
}
