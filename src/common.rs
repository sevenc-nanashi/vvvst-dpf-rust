/// Voicevox VSTのデータを置くディレクトリのパスを返す
pub fn data_dir() -> std::path::PathBuf {
    dirs::config_dir().unwrap().join("voicevox_vst")
}

/// ログを出力するディレクトリのパスを返す。
/// もし存在しなかったらディレクトリを作成する。
pub fn log_dir() -> std::path::PathBuf {
    let output_log_in_workspace = option_env!("VVVST_LOG").map_or(false, |v| v.len() > 0);

    let parent = if output_log_in_workspace {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR").to_string()).join("logs")
    } else {
        data_dir().join("logs")
    };

    if !parent.exists() {
        fs_err::create_dir_all(&parent).unwrap();
    }

    parent
}

// https://stackoverflow.com/a/75292572
pub const WINDOWS_CREATE_NO_WINDOW: u32 = 0x08000000;
