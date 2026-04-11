fn main() {
    cc::Build::new()
        .file("miniflac_impl.c")
        .include(".")
        .opt_level(2)
        .warnings(false)
        .flag("-mlongcalls") // Required for Xtensa: call8 range is limited to ~1MB
        .compile("miniflac");

    println!("cargo:rerun-if-changed=miniflac_impl.c");
    println!("cargo:rerun-if-changed=miniflac/miniflac.h");
}
