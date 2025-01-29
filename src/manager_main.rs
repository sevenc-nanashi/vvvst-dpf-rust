//! エンジン管理。
//! TCP通信でVSTインスタンスとのやり取りを行い、エンジンのArc的なものを提供する。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod common;
mod manager;

use crate::manager::EngineStatus;
use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result};
use fs4::fs_err3_tokio::AsyncFileExt;
use manager::pack;
use serde::{Deserialize, Serialize};
use tap::prelude::*;
use tokio::{io::AsyncBufReadExt, sync::Mutex};
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
    common::data_dir().join("engine_manager")
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

#[cached::proc_macro::once(result)]
async fn get_engine_port() -> Result<u16> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    Ok(port)
}

static ENGINE_PROCESS: LazyLock<Arc<Mutex<Option<tokio::process::Child>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(None)));
static LAST_USE_GPU: LazyLock<tokio::sync::Mutex<bool>> =
    LazyLock::new(|| tokio::sync::Mutex::new(false));
static CURRENT_CONNECTIONS: LazyLock<tokio::sync::Mutex<CurrentConnections>> =
    LazyLock::new(|| {
        tokio::sync::Mutex::new(CurrentConnections {
            num: 0,
            last_connection: std::time::Instant::now(),
        })
    });
#[tokio::main]
async fn main() -> Result<()> {
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
        init_log(true)?;

        // ロック成功時 = 他のプロセスが起動していない時
        info!("lock success: starting server");
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let state = State {
            manager_port: server.local_addr()?.port(),
            process_id: std::process::id(),
        };
        fs_err::tokio::write(&state_path, bincode::serialize(&state)?).await?;
        println!("{}", state.manager_port);
        let mut store = load_store().await?;

        if fs_err::tokio::metadata(&store.engine_path).await.is_err() {
            let engine_path = voicevox_engine_path();
            if fs_err::tokio::metadata(&engine_path).await.is_ok() {
                store.engine_path = engine_path;
            } else {
                let engine_path = ask_engine_path().await?;
                store.engine_path = engine_path;
            }
            save_store(&store).await?;
        }

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

        let result = tokio::select! {
            result = run_server(server) => {
                result
            }
            _ = no_connections => {
                info!("no connections for 10 seconds");
                Ok(())
            }
            _ = tokio::signal::ctrl_c() => {
                info!("ctrl-c received");
                Ok(())
            }
        };

        {
            let mut engine_process_lock = ENGINE_PROCESS.lock().await;
            if let Some(mut engine_process) = engine_process_lock.take() {
                if engine_process.try_wait()?.is_none() {
                    info!("killing engine process");
                    engine_process.kill().await?;
                }
            }
        }
        lock_file.unlock()?;
        drop(lock_file);
        std::fs::remove_file(&lock_path)?;

        result?;
    } else {
        init_log(false)?;

        // ロック失敗時 = 他のプロセスが起動している時。
        // stateが書き込まれるまで待つ
        info!("lock failed: waiting for state file");
        let mut state: Option<State> = None;
        for _ in 0..10 {
            if fs_err::tokio::metadata(&state_path).await.is_ok() {
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

fn init_log(is_host: bool) -> Result<()> {
    let log_dir = common::log_dir();
    if !log_dir.exists() {
        fs_err::create_dir_all(&log_dir)?;
    }
    let log_dest = log_dir.join(format!(
        "{}-engine-manager-{}.log",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        if is_host { "host" } else { "client" }
    ));
    let writer = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_dest)?;
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(if cfg!(debug_assertions) {
            tracing::Level::DEBUG
        } else {
            tracing::Level::INFO
        })
        .with_ansi(false)
        .with_writer(writer)
        .init();

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
                        info!("Hello sent");
                    }
                    manager::ToManagerMessage::ChangeEnginePath => {
                        info!("ChangeEnginePath received");
                        tokio::spawn(async {
                            if let Err(e) = change_engine_path().await {
                                error!("ChangeEnginePath failed: {:?}", e);
                            }
                        });
                    }
                    manager::ToManagerMessage::Ping => {
                        let mut writer_inner = writer.lock().await;
                        if let Err(e) =
                            pack(manager::ToClientMessage::Pong, &mut *writer_inner).await
                        {
                            break Err(e);
                        }
                        drop(writer_inner);

                        let mut last_ping_inner = last_ping.lock().await;
                        *last_ping_inner = std::time::Instant::now();
                    }
                    manager::ToManagerMessage::Start {
                        use_gpu,
                        force_restart,
                    } => {
                        info!("Start received");
                        start_engine(use_gpu, force_restart).await?;
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
            // それほど問題にはならないと思われる。あとシンプルに面倒
            loop {
                let engine_status = {
                    if let Some(engine_process) = ENGINE_PROCESS.lock().await.as_mut() {
                        if let Ok(Some(code)) = engine_process.try_wait() {
                            EngineStatus::Exited {
                                exit_code: code.code().unwrap_or(-1),
                            }
                        } else {
                            EngineStatus::Running {
                                port: get_engine_port().await?,
                            }
                        }
                    } else {
                        EngineStatus::NotRunning
                    }
                };
                if engine_status != previous_engine_status {
                    info!("Engine status changed: {:?}", engine_status);
                    match engine_status {
                        EngineStatus::Running { port } => {
                            info!("Sending EnginePort: {}", port);
                            let mut writer_inner = writer.lock().await;
                            if let Err(e) = pack(
                                manager::ToClientMessage::EnginePort(port),
                                &mut *writer_inner,
                            )
                            .await
                            {
                                break Err(e);
                            }
                        }
                        EngineStatus::Exited { exit_code } => {
                            if exit_code != 0 {
                                info!("Engine exited with code: {}", exit_code);
                                rfd::AsyncMessageDialog::new()
                                    .set_title("音声合成エンジンエラー")
                                    .set_description("音声合成エンジンが異常終了しました。エンジンを再起動してください。")
                                    .set_level(rfd::MessageLevel::Error)
                                    .set_buttons(rfd::MessageButtons::Ok)
                                    .show()
                                    .await;
                            }
                        }
                        _ => {}
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
async fn change_engine_path() -> Result<()> {
    let engine_path = ask_engine_path().await?;
    let mut store = load_store().await?;
    store.engine_path = engine_path;
    save_store(&store).await?;

    let last_use_gpu = { *LAST_USE_GPU.lock().await };
    start_engine(last_use_gpu, true).await?;

    Ok(())
}

async fn load_store() -> Result<Store> {
    let store_path = manager_path().join("store");
    if store_path.exists() {
        let store = bincode::deserialize(&fs_err::tokio::read(&store_path).await?)?;
        Ok(store)
    } else {
        Ok(Store::default())
    }
}

async fn save_store(store: &Store) -> Result<()> {
    let store_path = manager_path().join("store");
    fs_err::tokio::write(&store_path, bincode::serialize(store)?).await?;
    Ok(())
}

async fn ask_engine_path() -> Result<std::path::PathBuf> {
    let engine_path = if cfg!(target_os = "linux") {
        loop {
            let appimage_or_run = rfd::AsyncFileDialog::new()
                .pick_file()
                .await
                .context("failed to pick engine file")?;
            let appimage_or_run = appimage_or_run.path().to_path_buf();
            rfd::AsyncMessageDialog::new()
                .set_title("エンジンまたはAppImageが見つかりません")
                .set_description(
                    "エンジンまたはAppImageが見つかりませんでした。VOICEVOXのAppImageまたはrunを選択し直してください。",
                )
                .set_buttons(rfd::MessageButtons::Ok)
                .show()
                .await;
            if appimage_or_run.extension().unwrap() == "AppImage"
                || appimage_or_run.file_name() == Some(std::ffi::OsStr::new("run"))
            {
                break appimage_or_run;
            }
        }
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

    Ok(engine_path)
}

#[tracing::instrument(skip_all)]
async fn start_engine(use_gpu: bool, force_restart: bool) -> Result<()> {
    info!("starting engine");
    let port = get_engine_port().await?;
    let store = load_store().await?;
    let engine_path = store.engine_path;
    info!("engine_path: {:?}", engine_path);
    info!("port: {}", port);
    info!("use_gpu: {}", use_gpu);
    info!("force_restart: {}", force_restart);

    {
        let mut engine_process_lock = ENGINE_PROCESS.lock().await;
        if let Some(engine_process) = engine_process_lock.as_mut() {
            if engine_process.try_wait()?.is_none() {
                if force_restart {
                    info!("killing previous engine process");
                    engine_process.kill().await?;
                } else {
                    info!("engine already running");
                    return Ok(());
                }
            }
        }
    }
    {
        let mut last_use_gpu = LAST_USE_GPU.lock().await;
        *last_use_gpu = use_gpu;
    }
    let mut engine_process = tokio::process::Command::new(engine_path)
        .arg("--port")
        .arg(port.to_string())
        .pipe(|cmd| if use_gpu { cmd.arg("--use_gpu") } else { cmd })
        .tap(|cmd| info!("starting engine: {:?}", cmd))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .pipe(|cmd| {
            #[cfg(target_os = "windows")]
            let cmd = cmd.creation_flags(common::WINDOWS_CREATE_NO_WINDOW);

            cmd
        })
        .spawn()?;
    log_stdout_stderr(
        engine_process.stdout.take().unwrap(),
        engine_process.stderr.take().unwrap(),
    )?;
    {
        let mut engine_process_lock = ENGINE_PROCESS.lock().await;
        *engine_process_lock = Some(engine_process);
    }
    info!("engine started");

    Ok(())
}

fn log_stdout_stderr(
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
) -> Result<()> {
    let mut stdout = tokio::io::BufReader::new(stdout).lines();
    let mut stderr = tokio::io::BufReader::new(stderr).lines();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                line = stdout.next_line() => {
                    match line {
                        Ok(Some(line)) => debug!("engine stdout: {}", line),
                        Err(e) => error!("failed to read engine stdout: {:?}", e),
                        Ok(None) => break,
                    }
                }
                line = stderr.next_line() => {
                    match line {
                        Ok(Some(line)) => error!("engine stderr: {}", line),
                        Err(e) => error!("failed to read engine stderr: {:?}", e),
                        Ok(None) => break,
                    }
                }
            }
        }
    });

    Ok(())
}
