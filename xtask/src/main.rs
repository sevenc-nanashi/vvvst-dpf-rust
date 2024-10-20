use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
};

use clap::{Parser, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};

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

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    subcommand: SubCommands,
}

#[derive(Subcommand, Debug)]
enum SubCommands {
    /// C++のヘッダーファイルを生成する。
    #[command(version, about, long_about = None)]
    GenerateHeader,
    /// プラグインをビルドする。
    #[command(version, about, long_about = None)]
    Build(BuildArgs),
    /// licenses.jsonを生成する。
    #[command(version, about, long_about = None)]
    GenerateLicenses,
    /// Windows用のインストーラーを生成する。
    #[command(version, about, long_about = None)]
    GenerateInstaller,
}

#[derive(Parser, Debug)]
struct BuildArgs {
    /// Releaseビルドを行うかどうか。
    #[clap(short, long)]
    release: bool,
    /// logs内にVST内のログを出力するかどうか。
    #[clap(short, long)]
    log: bool,
    /// 開発用サーバーのURL。デフォルトはhttp://localhost:5173。
    #[clap(short, long)]
    dev_server_url: Option<String>,
}

fn generate_header() {
    let main_crate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let bindings = cbindgen::generate(&main_crate).unwrap();
    let destination_path = main_crate.join("src/rust.generated.hpp");
    bindings.write_to_file(&destination_path);

    green_log!("Finished", "generated to {:?}", destination_path,);
}
fn build(args: BuildArgs) {
    let main_crate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    if args.release {
        let editor_path = main_crate
            .join("resources")
            .join("editor")
            .join("index.html");
        if !editor_path.exists() {
            panic!("Editor resources not found at {:?}", editor_path);
        }

        if args.log {
            panic!("Cannot enable logging in release mode");
        }
        if args.dev_server_url.is_some() {
            panic!("Cannot specify dev server URL in release mode");
        }
    }
    let mut envs = std::env::vars().collect::<std::collections::HashMap<_, _>>();
    if args.log {
        envs.insert("VVVST_LOG".to_string(), "1".to_string());
    }
    if let Some(ref dev_server_url) = args.dev_server_url {
        envs.insert("VVVST_DEV_SERVER_URL".to_string(), dev_server_url.clone());
    }

    let build_name = if args.release { "release" } else { "debug" };
    green_log!(
        "Building",
        "log: {}, dev_server_url: {:?}, release: {}",
        args.log,
        args.dev_server_url,
        args.release
    );

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
    .before_spawn(|command| {
        blue_log!("Running", "{:?}", command);

        Ok(())
    })
    .dir(main_crate)
    .run()
    .unwrap();
    duct::cmd!("cmake", "--build", &destination_path)
        .dir(main_crate)
        .before_spawn(|command| {
            blue_log!("Running", "{:?}", command);

            Ok(())
        })
        .full_env(envs)
        .run()
        .unwrap();

    let elapsed = current.elapsed();
    green_log!(
        "Finished",
        "in {}.{:03}s",
        elapsed.as_secs(),
        elapsed.subsec_millis()
    );
    green_log!("", "destination: {:?}", &destination_path);
    green_log!("", "plugin: {:?}", destination_path.join("bin"),);
    if args.log {
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

    let version: String = main_cargo_toml.package.unwrap().version.unwrap();

    let installer_base = main_crate
        .join("resources")
        .join("installer")
        .join("installer_base.nsi");
    let installer_dist = main_crate.join("installer.nsi");

    let installer_base = std::fs::read_to_string(&installer_base).unwrap();
    std::fs::write(
        &installer_dist,
        installer_base.replace("{version}", &version),
    )
    .unwrap();
    blue_log!("Building", "wrote nsis script to {:?}", installer_dist);

    duct::cmd!("makensis", &installer_dist, "/INPUTCHARSET", "UTF8")
        .dir(main_crate)
        .before_spawn(|command| {
            blue_log!("Running", "{:?}", command);

            Ok(())
        })
        .run()
        .unwrap();
    green_log!(
        "Finished",
        "to {:?} in {}.{:03}s",
        installer_dist.with_extension("exe"),
        current.elapsed().as_secs(),
        current.elapsed().subsec_millis()
    );
}

fn main() {
    let args = Args::parse();

    match args.subcommand {
        SubCommands::GenerateHeader => {
            generate_header();
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
    }
}
