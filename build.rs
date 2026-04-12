fn main() {
    let mut build = cc::Build::new();
    build
        .file("miniflac_impl.c")
        .include(".")
        .opt_level(2)
        .warnings(false);

    // -mlongcalls is only valid for Xtensa targets (ESP32)
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("xtensa") {
        build.flag("-mlongcalls");
    }

    build.compile("miniflac");

    println!("cargo:rerun-if-changed=miniflac_impl.c");
    println!("cargo:rerun-if-changed=miniflac/miniflac.h");
}
