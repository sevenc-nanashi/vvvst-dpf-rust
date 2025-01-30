#![allow(dead_code)]

/// Voicevox VSTのデータを置くディレクトリのパスを返す
pub fn data_dir() -> std::path::PathBuf {
    dirs::config_dir().unwrap().join("voicevox_vst")
}

pub fn debug_log_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR").to_string()).join("logs")
}

pub fn release_log_dir() -> std::path::PathBuf {
    data_dir().join("logs")
}

/// ログを出力するディレクトリのパスを返す。
/// もし存在しなかったらディレクトリを作成する。
pub fn log_dir() -> std::path::PathBuf {
    let output_log_in_workspace = option_env!("VVVST_LOG").map_or(false, |v| v.len() > 0);

    if output_log_in_workspace {
        debug_log_dir()
    } else {
        release_log_dir()
    }
}

// https://stackoverflow.com/a/75292572
pub const WINDOWS_CREATE_NO_WINDOW: u32 = 0x08000000;
