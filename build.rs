fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-lib=static=windows.0.52.0");
    }
}
