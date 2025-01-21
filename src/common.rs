/// Voicevox VSTのデータを置くディレクトリのパスを返す
pub fn data_dir() -> std::path::PathBuf {
    dirs::config_dir().unwrap().join("voicevox_vst")
}
