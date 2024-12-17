use std::sync::LazyLock;
pub static RUNTIME: LazyLock<tokio::runtime::Runtime> =
    LazyLock::new(|| tokio::runtime::Runtime::new().unwrap());

pub static NUM_CHANNELS: u8 = 64;

pub fn data_dir() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap()
        .join("voicevox_vst")
}
