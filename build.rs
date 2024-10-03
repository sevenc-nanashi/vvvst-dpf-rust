fn main() {
    // ダイアログのメッセージのカスタマイズに必要：https://docs.rs/rfd/latest/rfd/index.html#customize-button-texts-of-message-dialog-in-windows
    if cfg!(target_os = "windows") {
        embed_resource::compile("resources/manifest.rc", embed_resource::NONE);
        println!("cargo:rustc-link-lib=static=windows.0.52.0");
    }
}
