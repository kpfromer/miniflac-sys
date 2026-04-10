fn main() {
    cc::Build::new()
        .file("miniflac_impl.c")
        .include(".") // so miniflac_impl.c can find miniflac/miniflac.h
        .opt_level(2)
        .warnings(false)
        .compile("miniflac");

    println!("cargo:rerun-if-changed=miniflac_impl.c");
    println!("cargo:rerun-if-changed=miniflac/miniflac.h");
}
