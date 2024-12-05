//! エンジン管理。
//! TCP通信でVSTインスタンスとのやり取りを行い、エンジンのArc的なものを提供する。

mod manager;
use anyhow::Result;
use fs2::FileExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct State {
    process_id: u32,
    server_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Store {
    engine_path: String,
}

fn manager_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap()
        .join("voicevox_vst_engine_manager")
}

#[tokio::main]
async fn main() -> Result<()> {
    let state_path = manager_path().join("state");
    let state_file = std::fs::OpenOptions::new()
        .read(true)
        .create(true)
        .open(&state_path)?;
    if state_file.try_lock_exclusive().is_ok() {
        // ロック成功時 = 他のプロセスが起動していない時
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let state = State {
            server_port: server.local_addr()?.port(),
            process_id: std::process::id(),
        };
        std::fs::write(&state_path, bincode::serialize(&state)?)?;
        println!("{}", state.server_port);
        run_server(server).await?;
        state_file.unlock()?;
        drop(state_file);
        std::fs::remove_file(&state_path)?;
    } else {
        // ロック失敗時 = 他のプロセスが起動している時
        let state: State = bincode::deserialize(&std::fs::read(&state_path)?)?;
        println!("{}", state.server_port);
    }
    Ok(())
}

async fn run_server(server: tokio::net::TcpListener) -> Result<()> {
    loop {
        let (stream, _) = server.accept().await?;
    }
}
