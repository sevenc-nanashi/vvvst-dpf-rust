//! エンジン管理。
//! TCP通信でVSTインスタンスとのやり取りを行い、エンジンのArc的なものを提供する。

mod manager;

use crate::manager::EngineStatus;
use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result};
use fs4::fs_err3_tokio::AsyncFileExt;
use manager::pack;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct State {
    process_id: u32,
    manager_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Store {
    engine_path: std::path::PathBuf,
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

fn voicevox_engine_path() -> std::path::PathBuf {
    // Windows: %LOCALAPPDATA%/Programs/VOICEVOX/vv-engine/run.exe
    // macOS: /Applications/VOICEVOX.app/Contents/Resources/vv-engine/run
    // Linux: ~/.voicevox/VOICEVOX.AppImage
    if cfg!(target_os = "windows") {
        std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap())
            .join("Programs")
            .join("VOICEVOX")
            .join("vv-engine")
            .join("run.exe")
    } else if cfg!(target_os = "macos") {
        std::path::PathBuf::from("/Applications/VOICEVOX.app/Contents/Resources/vv-engine/run")
    } else {
        dirs::home_dir()
            .unwrap()
            .join(".voicevox")
            .join("VOICEVOX.AppImage")
    }
}

async fn get_random_port() -> Result<u16> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    Ok(port)
}

static ENGINE_STATUS: LazyLock<Arc<Mutex<EngineStatus>>> =
    LazyLock::new(|| Arc::new(Mutex::new(EngineStatus::NotRunning)));
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
        let mut store: Store = if tokio::fs::metadata(&store_path).await.is_ok() {
            bincode::deserialize(&tokio::fs::read(&store_path).await?)?
        } else {
            Store::default()
        };
        if !store.engine_path.exists() {
            let engine_path = voicevox_engine_path();
            if engine_path.exists() {
                tokio::fs::write(&store_path, bincode::serialize(&Store { engine_path })?).await?;
            } else {
                let engine_path = if cfg!(target_os = "linux") {
                    let appimage_or_run = rfd::AsyncFileDialog::new()
                        .pick_file()
                        .await
                        .context("failed to pick engine file")?;
                    appimage_or_run.path().to_path_buf()
                } else {
                    loop {
                        let voicevox_dir = rfd::AsyncFileDialog::new()
                            .set_title("VOICEVOXのフォルダを選択してください")
                            .pick_folder()
                            .await
                            .context("failed to pick engine file")?
                            .path()
                            .to_path_buf();
                        let run_name = if cfg!(target_os = "windows") {
                            "run.exe"
                        } else {
                            "run"
                        };
                        if voicevox_dir.join("vv-engine").join(run_name).exists() {
                            break voicevox_dir.join("vv-engine").join(run_name);
                        }
                        rfd::AsyncMessageDialog::new()
                            .set_title("エンジンが見つかりません")
                            .set_description(
                                "エンジンが見つかりませんでした。VOICEVOXのフォルダを選択し直してください。",
                            )
                            .set_buttons(rfd::MessageButtons::Ok)
                            .show()
                            .await;
                    }
                };

                store.engine_path = engine_path;
                tokio::fs::write(&store_path, bincode::serialize(&store)?).await?;
            }
        }

        let engine_path = store.engine_path;
        let random_port = get_random_port().await?;
        {
            let engine_status = EngineStatus::Running { port: random_port };
            let mut engine_status_lock = ENGINE_STATUS.lock().await;
            *engine_status_lock = engine_status;
        }
        let mut engine_process = tokio::process::Command::new(engine_path)
            .arg("--port")
            .arg(random_port.to_string())
            .spawn()?;
        let no_connections = async {
            loop {
                let current_connections = CURRENT_CONNECTIONS.lock().await;
                if current_connections.num == 0
                    && current_connections.last_connection.elapsed().as_secs() > 10
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        };
        tokio::select! {
            result = run_server(server) => {
                result?;
            }
            _ = no_connections => {
                info!("no connections for 10 seconds");
            }
            _ = engine_process.wait() => {
                let mut engine_status = ENGINE_STATUS.lock().await;
                *engine_status = EngineStatus::Dead;
                info!("engine process exited");
            }
            _ = tokio::signal::ctrl_c() => {
                info!("ctrl-c received");
            }
        };
        if engine_process.try_wait()?.is_none() {
            info!("killing engine process");
            engine_process.kill().await?;
        }
        lock_file.unlock()?;
        drop(lock_file);
        std::fs::remove_file(&lock_path)?;
    } else {
        // ロック失敗時 = 他のプロセスが起動している時。
        // stateが書き込まれるまで待つ
        info!("lock failed: waiting for state file");
        let mut state: Option<State> = None;
        for _ in 0..10 {
            if tokio::fs::metadata(&state_path).await.is_ok() {
                state = Some(bincode::deserialize(&std::fs::read(&state_path)?)?);
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
            bincode::deserialize(&std::fs::read(&state_path)?)?
        }
        let state = state.ok_or_else(|| anyhow::anyhow!("state file not found"))?;

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

#[tracing::instrument(skip_all)]
async fn handle_connection(mut stream: tokio::net::TcpStream) -> Result<()> {
    stream.set_nodelay(true)?;
    info!("Connection established");
    let (mut reader, writer) = stream.split();
    {
        let mut current_connections = CURRENT_CONNECTIONS.lock().await;
        current_connections.num += 1;
        current_connections.last_connection = std::time::Instant::now();
    }
    let writer = Arc::new(tokio::sync::Mutex::new(writer));
    let last_ping = Arc::new(tokio::sync::Mutex::new(std::time::Instant::now()));

    let response_handler = {
        let writer = Arc::clone(&writer);
        let last_ping = Arc::clone(&last_ping);
        async move {
            loop {
                let unpacked: manager::ToManagerMessage = match manager::unpack(&mut reader).await {
                    Ok(unpacked) => unpacked,
                    Err(e) => {
                        break Err(e);
                    }
                };
                match unpacked {
                    manager::ToManagerMessage::Hello => {
                        info!("Hello received");
                        let mut writer_inner = writer.lock().await;
                        if let Err(e) =
                            pack(manager::ToClientMessage::Hello, &mut *writer_inner).await
                        {
                            break Err(e);
                        }
                    }
                    manager::ToManagerMessage::Ping => {
                        info!("Ping received");
                        let mut writer_inner = writer.lock().await;
                        if let Err(e) =
                            pack(manager::ToClientMessage::Pong, &mut *writer_inner).await
                        {
                            break Err(e);
                        }

                        let mut last_ping_inner = last_ping.lock().await;
                        *last_ping_inner = std::time::Instant::now();
                    }
                    manager::ToManagerMessage::Exit => {
                        info!("Exit received");
                        break Ok(());
                    }
                }
            }
        }
    };
    let engine_status_watcher = {
        let writer = Arc::clone(&writer);
        async move {
            let mut previous_engine_status = EngineStatus::NotRunning;
            // NOTE: 1秒ごとにエンジンの状態を確認する
            // 本来はcrossbeamとかを使うべきだが、エンジンの起動は長いのでポーリングによる実装でも
            // それほど問題にはならないと思われる
            loop {
                let engine_status = ENGINE_STATUS.lock().await.clone();
                if engine_status != previous_engine_status {
                    info!("Engine status changed: {:?}", engine_status);
                    let mut writer_inner = writer.lock().await;
                    if let Err(e) = pack(
                        manager::ToClientMessage::EngineStatus(engine_status),
                        &mut *writer_inner,
                    )
                    .await
                    {
                        break Err(e);
                    }
                    if engine_status == EngineStatus::Dead {
                        break Ok(());
                    }
                    previous_engine_status = engine_status;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    };
    let exit_handler = {
        let last_ping = Arc::clone(&last_ping);
        async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let last_ping_inner = last_ping.lock().await;
                if last_ping_inner.elapsed().as_secs() > 10 {
                    break;
                }
            }
        }
    };
    let result = tokio::select! {
        result = response_handler => {
            result
        }
        _ = exit_handler => {
            info!("Connection timed out");
            Ok(())
        }
        result = engine_status_watcher => {
            result
        }
    };
    {
        let mut current_connections = CURRENT_CONNECTIONS.lock().await;
        current_connections.num -= 1;
    }
    result?;

    Ok(())
}
