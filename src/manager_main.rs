//! エンジン管理。
//! TCP通信でVSTインスタンスとのやり取りを行い、エンジンのArc的なものを提供する。

mod manager;

use crate::manager::EngineStatus;
use std::sync::{LazyLock, OnceLock};

use anyhow::{Context, Result};
use fs4::fs_err3_tokio::AsyncFileExt;
use manager::pack;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct State {
    process_id: u32,
    manager_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Store {
    engine_path: String,
}

#[derive(Debug, Clone)]
struct CurrentConnections {
    num: u32,
    last_connection: std::time::Instant,
}

fn manager_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap()
        .join("voicevox_vst_engine_manager")
}

fn engine_path() -> std::path::PathBuf {
    // Windows: %APPDATA%/voicevox/config.json
    // macOS: ~/Library/Application Support/voicevox/config.json
    // Linux: ~/.config/voicevox/config.json
    let config_path = if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA")?;
        std::path::PathBuf::from(appdata).join("voicevox/config.json")
    } else if cfg!(target_os = "macos") {
        let home = std::env::var("HOME")?;
        std::path::PathBuf::from(home).join("Library/Application Support/voicevox/config.json")
    } else {
        let home = std::env::var("HOME")?;
        std::path::PathBuf::from(home).join(".config/voicevox/config.json")
    };
}

static ENGINE_STATUS: OnceLock<Option<EngineStatus>> = OnceLock::new();
static CURRENT_CONNECTIONS: LazyLock<tokio::sync::Mutex<CurrentConnections>> =
    LazyLock::new(|| {
        tokio::sync::Mutex::new(CurrentConnections {
            num: 0,
            last_connection: std::time::Instant::now(),
        })
    });

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let lock_path = manager_path().join("lock");
    let state_path = manager_path().join("state");
    let store_path = manager_path().join("store");
    if !manager_path().exists() {
        info!("creating manager directory");
        std::fs::create_dir_all(&manager_path())?;
    }
    debug!("lock_path: {:?}", lock_path);
    debug!("state_path: {:?}", state_path);
    debug!("store_path: {:?}", store_path);

    let lock_file = fs_err::tokio::OpenOptions::new()
        .write(true)
        .create(true)
        .open(&lock_path)
        .await
        .context("failed to open lock file")?;
    if lock_file.try_lock_exclusive().is_ok() {
        // ロック成功時 = 他のプロセスが起動していない時
        info!("lock success: starting server");
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let state = State {
            manager_port: server.local_addr()?.port(),
            process_id: std::process::id(),
        };
        tokio::fs::write(&state_path, bincode::serialize(&state)?).await?;
        println!("{}", state.manager_port);
        let store: Store = if tokio::fs::metadata(&store_path).await.is_ok() {
            bincode::deserialize(&tokio::fs::read(&store_path).await?)?
        } else {
            Store::default()
        };
        if store.engine_path.is_empty() {
            if tokio::fs::metadata("voicevox_vst_engine").await.is_err() {}
            rfd::AsyncFileDialog::new()
                .add_filter("Voicevox VST", "*.vst3")
                .pick_file()
                .await
                .context("failed to open file dialog")?;
        }

        let engine_process = tokio::process::Command::new("voicevox_vst_engine")
            .arg(format!("--manager-port={}", state.manager_port))
            .spawn()?;
        tokio::select! {
            result = run_server(server) => {
                result?;
            }
            _ = tokio::signal::ctrl_c() => {
                info!("ctrl-c received");
            }
        };
        lock_file.unlock()?;
        drop(lock_file);
        std::fs::remove_file(&lock_path)?;
    } else {
        // ロック失敗時 = 他のプロセスが起動している時。
        // stateが書き込まれるまで待つ
        info!("lock failed: waiting for state file");
        let mut state: Option<State> = None;
        for _ in 0..10 {
            if std::fs::metadata(&state_path).is_ok() {
                state = Some(bincode::deserialize(&std::fs::read(&state_path)?)?);
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
            bincode::deserialize(&std::fs::read(&state_path)?)?
        }
        if state.is_none() {
            return Err(anyhow::anyhow!("state file not found"));
        }
        let state: State = bincode::deserialize(&std::fs::read(&lock_path)?)?;
        println!("{}", state.manager_port);
    }
    Ok(())
}

async fn run_server(server: tokio::net::TcpListener) -> Result<()> {
    loop {
        let (stream, _) = server.accept().await?;
        debug!("new connection");
        tokio::spawn(async move {
            let result = handle_connection(stream).await;
            match result {
                Ok(_) => {
                    debug!("Connection successfully closed");
                }
                Err(e) => {
                    error!("Connection dead with error: {:?}", e);
                }
            }
        });
    }
}

#[tracing::instrument]
async fn handle_connection(mut stream: tokio::net::TcpStream) -> Result<()> {
    stream.set_nodelay(true)?;
    loop {
        let unpacked: manager::ToManagerMessage = manager::unpack(&mut stream).await?;
        match unpacked {
            manager::ToManagerMessage::Hello => {
                info!("Hello received");
                pack(manager::ToClientMessage::Hello, &mut stream).await?;
            }
            manager::ToManagerMessage::Exit => {
                info!("Exit received");
                break;
            }
        }
    }
    Ok(())
}
