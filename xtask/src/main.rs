use clap::{Parser, Subcommand};
use colored::Colorize;
use notify::Watcher;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
    io::Write,
};

#[path = "../../src/common.rs"]
mod common;

macro_rules! green_log {
    ($subject:expr, $($args:tt)+) => {
        println!("{:>12} {}", $subject.bold().green(), &format!($($args)*));
    };
}
macro_rules! blue_log {
    ($subject:expr, $($args:tt)+) => {
        println!("{:>12} {}", $subject.bold().cyan(), &format!($($args)*));
    };
}
macro_rules! red_log {
    ($subject:expr, $($args:tt)+) => {
        println!("{:>12} {}", $subject.bold().red(), &format!($($args)*));
    };
}

fn print_cmd(command: &std::process::Command) -> std::io::Result<()> {
    blue_log!("Running", "{:?}", command);

    Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    subcommand: SubCommands,
}

#[derive(Subcommand, Debug)]
enum SubCommands {
    /// C++ <-> Rustのブリッジを生成する。
    #[command(version, about, long_about = None)]
    GenerateBridge,

    /// プラグインをビルドする。
    #[command(version, about, long_about = None)]
    Build(BuildArgs),

    /// licenses.jsonを生成する。
    #[command(version, about, long_about = None)]
    GenerateLicenses,

    /// Windows用のインストーラーを生成する。
    #[command(version, about, long_about = None)]
    GenerateInstaller,

    /// ログを確認する。
    #[command(version, about, long_about = None)]
    WatchLog(WatchLogArgs),
}

#[derive(Parser, Debug)]
struct BuildArgs {
    /// Releaseビルドを行うかどうか。
    #[clap(short, long)]
    release: bool,
    /// logs内にVST内のログを出力するかどうか。
    #[clap(short, long)]
    log: Option<bool>,
    /// 開発用サーバーのURL。デフォルトはhttp://localhost:5173。
    #[clap(short, long)]
    dev_server_url: Option<String>,
}

#[derive(Parser, Debug)]
struct WatchLogArgs {
    /// 製品版のログを確認するかどうか。
    #[clap(short, long)]
    release: bool,
}

fn generate_bridge() {
    blue_log!("Running", "cbindgen");
    let main_crate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let bindings = cbindgen::generate(&main_crate).unwrap();
    let mut cbindgen_binding = vec![];
    bindings.write(&mut cbindgen_binding);

    let message = "xtaskによって生成。手動で編集しないでください。";

    blue_log!("Generating", "rust_bridge.generated.hpp");
    let contents = std::str::from_utf8(&cbindgen_binding).unwrap();
    let re = lazy_regex::regex!(
        r#"EXPORT\s+(?<returns>[\w ]+\s+\*?)(?<name>\w+)\s*\((?<args>[^)]*)\);"#
    );
    let mut functions = vec![];
    for cap in re.captures_iter(&contents) {
        let returns = cap.name("returns").unwrap().as_str();
        let name = cap.name("name").unwrap().as_str();
        let args = cap.name("args").unwrap().as_str();
        let args = args
            .split(',')
            .map(|arg| {
                let arg = arg.trim();
                arg.split_whitespace()
                    .filter(|arg| !arg.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>();
        functions.push((returns, name, args));
    }

    let types = contents
        .lines()
        .skip_while(|line| !line.contains("namespace Rust {"))
        .skip(1)
        .take_while(|line| !line.contains("extern \"C\" {"))
        .collect::<Vec<_>>();
    let types = types.join("\n");
    assert!(!types.is_empty());

    let bridge_header_path = main_crate.join("src/rust_bridge.generated.hpp");
    let mut file = std::fs::File::create(&bridge_header_path).unwrap();
    writeln!(file, "// {}", message).unwrap();
    writeln!(file, "#pragma once").unwrap();
    writeln!(file, "#include <choc/platform/choc_DynamicLibrary.h>").unwrap();
    writeln!(file, "#include <cstdint>").unwrap();
    writeln!(file).unwrap();
    writeln!(file, "namespace Rust {{").unwrap();
    writeln!(file, "{}", types).unwrap();
    for (returns, name, args) in &functions {
        let args = args.join(", ");
        writeln!(file, "    {} {}({});", returns, name, args).unwrap();
        writeln!(file).unwrap();
    }
    writeln!(file, "}}").unwrap();

    blue_log!("Generating", "rust_bridge.generated.cpp");
    let bridge_path = main_crate.join("src/rust_bridge.generated.cpp");
    let mut file = std::fs::File::create(&bridge_path).unwrap();
    writeln!(file, "// {}", message).unwrap();
    writeln!(file, "#include \"rust_bridge.generated.hpp\"").unwrap();
    writeln!(file, "#include \"rust_bridge.hpp\"").unwrap();
    writeln!(file).unwrap();
    writeln!(file, "namespace Rust {{").unwrap();
    for (returns, name, args) in &functions {
        let args = args.join(", ");
        writeln!(file, "    typedef {} (*{}_t)({});", returns, name, args).unwrap();
        writeln!(file, "    {} {}({}) {{", returns, name, args).unwrap();
        writeln!(file, "        auto rust = Rust::loadRustDll();").unwrap();
        writeln!(
            file,
            "        auto fn = ({}_t)rust->findFunction(\"{}\");",
            name, name
        )
        .unwrap();

        let args_regex = lazy_regex::regex!(r"(?P<name>\w+)(?:,|$)");
        let mut arg_names = vec![];
        for cap in args_regex.captures_iter(&args) {
            let name = cap.name("name").unwrap().as_str();
            arg_names.push(name);
        }
        let arg_names = arg_names.join(", ");

        if *returns != "void" {
            writeln!(file, "        return fn({});", arg_names).unwrap();
        } else {
            writeln!(file, "        fn({});", arg_names).unwrap();
        }
        writeln!(file, "    }}").unwrap();
        writeln!(file).unwrap();
    }
    writeln!(file, "}}").unwrap();

    duct::cmd!("clang-format", "-i", &bridge_header_path)
        .before_spawn(|command| print_cmd(command))
        .run()
        .unwrap();
    duct::cmd!("clang-format", "-i", &bridge_path)
        .before_spawn(|command| print_cmd(command))
        .run()
        .unwrap();

    green_log!("Finished", "generated to:");
    green_log!("", "- {:?}", bridge_header_path);
    green_log!("", "- {:?}", bridge_path);
}

fn build(args: BuildArgs) {
    let main_crate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();

    let enable_log = args.log.unwrap_or(!args.release);
    if args.release {
        let editor_path = main_crate
            .join("resources")
            .join("editor")
            .join("index.html");
        if !editor_path.exists() {
            panic!("Editor resources not found at {:?}", editor_path);
        }

        if enable_log {
            panic!("Cannot enable logging in release mode");
        }
        if args.dev_server_url.is_some() {
            panic!("Cannot specify dev server URL in release mode");
        }
    }
    let mut envs = std::env::vars().collect::<std::collections::HashMap<_, _>>();
    if enable_log {
        envs.insert("VVVST_LOG".to_string(), "1".to_string());
    }
    if let Some(ref dev_server_url) = args.dev_server_url {
        envs.insert("VVVST_DEV_SERVER_URL".to_string(), dev_server_url.clone());
    }

    if colored::control::SHOULD_COLORIZE.should_colorize() {
        envs.insert("CLICOLOR_FORCE".to_string(), "1".to_string());
    }

    let build_name = if args.release { "release" } else { "debug" };
    green_log!(
        "Building",
        "log: {}, dev_server_url: {:?}, release: {}",
        enable_log,
        args.dev_server_url,
        args.release
    );

    duct::cmd!(
        "cargo",
        "build",
        "-p",
        "vvvst",
        "--profile",
        if args.release { "release" } else { "dev" }
    )
    .before_spawn(|command| print_cmd(command))
    .dir(main_crate)
    .full_env(envs.clone())
    .run()
    .unwrap();

    let destination_path = main_crate.join("build").join(build_name);

    let current = std::time::Instant::now();

    let build_type = format!(
        "-DCMAKE_BUILD_TYPE={}",
        if args.release { "Release" } else { "Debug" }
    );
    let build_dir = format!("-B{}", &destination_path.to_string_lossy());
    // なぜか_add_libraryが無限に再帰するので、vcpkgを無効化する。
    // https://github.com/microsoft/vcpkg/issues/11307
    if cfg!(windows) {
        duct::cmd!(
            "cmake",
            "-DCMAKE_TOOLCHAIN_FILE=OFF",
            &build_type,
            &build_dir
        )
    } else {
        duct::cmd!("cmake", &build_type, &build_dir)
    }
    .before_spawn(|command| print_cmd(command))
    .dir(main_crate)
    .run()
    .unwrap();
    duct::cmd!("cmake", "--build", &destination_path)
        .dir(main_crate)
        .before_spawn(|command| print_cmd(command))
        .full_env(envs)
        .run()
        .unwrap();

    let elapsed = current.elapsed();
    green_log!(
        "Finished",
        "built in {}.{:03}s",
        elapsed.as_secs(),
        elapsed.subsec_millis()
    );
    green_log!("", "destination: {:?}", &destination_path);
    green_log!("", "plugin: {:?}", destination_path.join("bin"),);
    if enable_log {
        green_log!("", "logs: {:?}", main_crate.join("logs"));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
struct License {
    name: String,
    version: String,
    license: String,
    text: String,
}
impl Hash for License {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}
impl PartialEq for License {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

fn generate_licenses() {
    let current = std::time::Instant::now();

    let main_crate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let destination_path = main_crate
        .join("resources")
        .join("editor_ext")
        .join("licenses.generated.json");
    let cargo_toml_path = main_crate.join("Cargo.toml");
    let krates = cargo_about::get_all_crates(
        &camino::Utf8Path::new(&cargo_toml_path.to_string_lossy()),
        false,
        false,
        vec![],
        false,
        &Default::default(),
    )
    .unwrap();
    let licenses = krates
        .krates()
        .map(|krate| {
            let license = krate
                .license
                .as_ref()
                .map(|license| license.to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            let mut license_text = format!("License: {}\n", license);
            let mut license_file_paths = vec!["license".to_string(), "copying".to_string()];
            if let Some(license_file) = krate.license_file.as_ref() {
                let license_file = license_file.to_string().to_lowercase();
                license_file_paths.insert(0, license_file);
            }
            let files = std::fs::read_dir(krate.manifest_path.parent().unwrap()).unwrap();
            for file in files {
                let file = file.unwrap();
                if let Some(file_name) = file.file_name().to_str() {
                    let file_name = file_name.to_lowercase();
                    if license_file_paths
                        .iter()
                        .any(|path| file_name.contains(path))
                    {
                        let text = std::fs::read_to_string(file.path()).unwrap();
                        license_text.push_str(&text);
                        break;
                    }
                }
            }
            License {
                name: krate.name.to_string(),
                version: krate.version.to_string(),
                license,
                text: license_text,
            }
        })
        .collect::<HashSet<_>>();

    let licenses_json = serde_json::to_string_pretty(&licenses).unwrap();
    std::fs::write(&destination_path, licenses_json).unwrap();

    green_log!(
        "Finished",
        "generated to {:?} in {}.{:03}s",
        destination_path,
        current.elapsed().as_secs(),
        current.elapsed().subsec_millis()
    );

    green_log!(
        "",
        "license count: {}/{}",
        licenses
            .iter()
            .filter(|license| license.text.split('\n').count() > 2)
            .count(),
        licenses.len()
    );
}

fn generate_installer() {
    let current = std::time::Instant::now();

    let main_crate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let main_cargo_toml = main_crate.join("Cargo.toml");
    let main_cargo_toml = cargo_toml::Manifest::from_path(&main_cargo_toml).unwrap();

    let version = main_cargo_toml.package.unwrap().version.unwrap();
    let version = semver::Version::parse(&version).unwrap();

    let installer_base = main_crate
        .join("resources")
        .join("installer")
        .join("installer_base.nsi");
    let installer_dist = main_crate.join("installer.nsi");

    let installer_base = std::fs::read_to_string(&installer_base).unwrap();
    std::fs::write(
        &installer_dist,
        installer_base.replace(
            "{version}",
            format!("{}.{}.{}", version.major, version.minor, version.patch).as_str(),
        ),
    )
    .unwrap();
    blue_log!("Building", "wrote nsis script to {:?}", installer_dist);

    duct::cmd!("makensis", &installer_dist, "/INPUTCHARSET", "UTF8")
        .dir(main_crate)
        .before_spawn(|command| print_cmd(command))
        .run()
        .unwrap();
    green_log!(
        "Finished",
        "built to {:?} in {}.{:03}s",
        installer_dist.with_extension("exe"),
        current.elapsed().as_secs(),
        current.elapsed().subsec_millis()
    );
}

#[derive(Debug)]
struct CrossbeamEventHandler(crossbeam_channel::Sender<notify::Result<notify::Event>>);

impl notify::EventHandler for CrossbeamEventHandler {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        self.0.send(event).unwrap();
    }
}

fn watch_log(args: WatchLogArgs) {
    let logs = if args.release {
        let main_crate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        main_crate.join("logs")
    } else {
        common::log_dir()
    };
    if !logs.exists() {
        panic!("Logs not found at {:?}", logs);
    }

    let (stop_tx, stop_rx) = crossbeam_channel::unbounded::<()>();
    let plugin_log = std::thread::spawn({
        let logs = logs.clone();
        let rx = stop_rx.clone();
        move || watch_log_impl(logs, "plugin", rx)
    });
    let engine_manager_host_log = std::thread::spawn({
        let logs = logs.clone();
        let rx = stop_rx.clone();
        move || watch_log_impl(logs, "engine-manager-host", rx)
    });
    let engine_manager_client_log = std::thread::spawn({
        let logs = logs.clone();
        let rx = stop_rx.clone();
        move || watch_log_impl(logs, "engine-manager-client", rx)
    });
    ctrlc::set_handler(move || {
        green_log!("Stopping", "watching logs");
        // 4回送ることで全てのスレッドを停止させる。
        // TODO: もう少しまともな方法があれば変更する。
        stop_tx.send(()).unwrap();
        stop_tx.send(()).unwrap();
        stop_tx.send(()).unwrap();
        stop_tx.send(()).unwrap();
    })
    .unwrap();
    let _ = stop_rx.recv();
    plugin_log.join().unwrap();
    engine_manager_host_log.join().unwrap();
    engine_manager_client_log.join().unwrap();

    fn watch_log_impl(
        logs: std::path::PathBuf,
        suffix: &str,
        stop_rx: crossbeam_channel::Receiver<()>,
    ) -> () {
        let (tx, rx) = crossbeam_channel::unbounded::<notify::Result<notify::Event>>();
        let mut watcher = notify::recommended_watcher(CrossbeamEventHandler(tx)).unwrap();
        watcher
            .watch(&logs, notify::RecursiveMode::Recursive)
            .unwrap();
        let mut current_log = find_log(&logs, suffix);
        let mut current_log_process: Option<duct::Handle> = None;

        if let Some(ref current_log) = current_log {
            green_log!("Watching", "current log: {:?}", current_log);
            current_log_process = Some(duct::cmd!("tail", "-f", current_log).start().unwrap());
        } else {
            green_log!("Watching", "no log found for {:?}", suffix);
        }

        'outer: loop {
            crossbeam_channel::select! {
                recv(stop_rx) -> _ => {
                    if let Some(ref mut current_log_process) = current_log_process {
                        current_log_process.kill().unwrap();
                    }
                    break 'outer;
                }
                recv(rx) -> event => {
                    let event = event.unwrap().unwrap();
                    match event.kind {
                        notify::EventKind::Create(_) | notify::EventKind::Remove(_) => {
                            let new_log = find_log(&logs, suffix);
                            if new_log != current_log {
                                if let Some(ref mut current_log_process) = current_log_process {
                                    current_log_process.kill().unwrap();
                                }
                                if let Some(ref new_log) = new_log {
                                    green_log!("Watching", "new log: {:?}", new_log);
                                    current_log_process =
                                        Some(duct::cmd!("tail", "-f", new_log).start().unwrap());
                                } else {
                                    green_log!("Watching", "no log found for {:?}", suffix);
                                }
                                current_log = new_log;
                            }

                            if let Some(ref current_log) = current_log {
                                let panic_path = current_log.with_extension("panic");
                                if panic_path.exists() {
                                    let panic = std::fs::read_to_string(&panic_path).unwrap();
                                    red_log!("Panicked", "{}", panic);
                                }
                            }
                        }
                        _ => {
                            continue;
                        }
                    }
                }
            }
        }
    }
    fn find_log(logs_dir: &std::path::Path, suffix: &str) -> Option<std::path::PathBuf> {
        let mut current_logs = std::fs::read_dir(&logs_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.is_file()
                    && path.extension().unwrap_or_default() == "log"
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.split('-').next().unwrap().parse::<u64>().is_ok()
                                && name.contains(format!("-{}.", suffix).as_str())
                        })
            })
            .collect::<Vec<_>>();
        current_logs.sort_by_key(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap()
                .split('-')
                .next()
                .unwrap()
                .parse::<u64>()
                .unwrap()
        });

        current_logs.last().cloned()
    }
}

fn main() {
    let args = Args::parse();

    match args.subcommand {
        SubCommands::GenerateBridge => {
            generate_bridge();
        }
        SubCommands::Build(build_args) => {
            build(build_args);
        }
        SubCommands::GenerateLicenses => {
            generate_licenses();
        }
        SubCommands::GenerateInstaller => {
            generate_installer();
        }
        SubCommands::WatchLog(args) => {
            watch_log(args);
        }
    }
}
