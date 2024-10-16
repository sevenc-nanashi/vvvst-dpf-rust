fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows"
        && std::env::var("CARGO_FEATURE_STATIC_LINK_WINDOWS").is_ok()
    {
        println!("cargo:rustc-link-lib=static=windows.0.52.0");
    }
}
