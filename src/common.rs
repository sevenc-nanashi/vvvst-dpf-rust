/// Voicevox VSTのデータを置くディレクトリのパスを返す
pub fn data_dir() -> std::path::PathBuf {
    dirs::config_dir().unwrap().join("voicevox_vst")
}

/// Voicevox VSTのエディタの設定ファイルのパスを返す
pub fn editor_config_path() -> std::path::PathBuf {
    data_dir().join("config.json")
}

/// Voicevox本家のconfig.jsonのパスを返す
pub fn original_config_path() -> std::path::PathBuf {
    // Windows: %APPDATA%/voicevox/config.json
    // macOS: ~/Library/Application Support/voicevox/config.json
    // Linux: ~/.config/voicevox/config.json
    if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA").unwrap();
        std::path::PathBuf::from(appdata).join("voicevox/config.json")
    } else if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").unwrap();
        std::path::PathBuf::from(home).join("Library/Application Support/voicevox/config.json")
    } else {
        let home = std::env::var("HOME").unwrap();
        std::path::PathBuf::from(home).join(".config/voicevox/config.json")
    }
}

/// 実際に使われるconfig.jsonのパスを返す
pub async fn config_path() -> std::path::PathBuf {
    if tokio::fs::metadata(editor_config_path()).await.is_ok() {
        editor_config_path()
    } else {
        original_config_path()
    }
}
