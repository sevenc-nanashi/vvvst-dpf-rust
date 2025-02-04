use std::sync::{LazyLock, Mutex};
pub static RUNTIME: LazyLock<Mutex<Option<tokio::runtime::Runtime>>> =
    LazyLock::new(|| Mutex::new(Some(tokio::runtime::Runtime::new().unwrap())));

pub static NUM_CHANNELS: u8 = 64;
