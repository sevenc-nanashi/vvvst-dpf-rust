use clap::{Parser, Subcommand};

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

    /// Rustのビルドをスキップするかどうか。
    #[clap(long)]
    skip_rust: bool,
    /// C++のビルドをスキップするかどうか。
    #[clap(long)]
    skip_cpp: bool,
}

fn generate_header() {
    let main_crate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let bindings = cbindgen::generate(&main_crate).unwrap();
    let destination_path = main_crate.join("src/rust.generated.hpp");
    bindings.write_to_file(&destination_path);

    println!("Generated bindings to {:?}", destination_path);
}
fn build(args: BuildArgs) {
    println!("Building...");
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
    if let Some(dev_server_url) = args.dev_server_url {
        envs.insert("VVVST_DEV_SERVER_URL".to_string(), dev_server_url);
    }
    if !args.skip_rust {
        if args.release {
            duct::cmd!("cargo", "build", "--release")
        } else {
            duct::cmd!("cargo", "build")
        }
        .dir(main_crate)
        .full_env(envs)
        .run()
        .unwrap();
    }

    let destination_path = if cfg!(windows) {
        // Visual Studioと合わせる
        let build_name = if args.release {
            "x64-Release"
        } else {
            "x64-Debug"
        };

        main_crate.join("build").join(build_name)
    } else {
        main_crate.join("build")
    };

    if !args.skip_cpp {
        let build_type = format!(
            "-DCMAKE_BUILD_TYPE={}",
            if args.release { "Release" } else { "Debug" }
        );
        let build_dir = format!("-B{}", &destination_path.to_string_lossy());
        // なぜか_add_libraryが無限に再帰するので、vcpkgを無効化する。
        // https://github.com/microsoft/vcpkg/issues/11307
        if cfg!(windows) {
            duct::cmd!("cmake", "-DCMAKE_TOOLCHAIN_FILE=OFF", &build_type, &build_dir)
        } else {
            duct::cmd!("cmake", &build_type, &build_dir)
        }
        .dir(main_crate)
        .run()
        .unwrap();
        duct::cmd!("cmake", "--build", &destination_path)
            .dir(main_crate)
            .run()
            .unwrap();
    }

    println!("Built to {:?}", &destination_path);
    println!("Plugin dir: {:?}", destination_path.join("bin"));
    if args.log {
        println!("Logs dir: {:?}", main_crate.join("logs"));
    }
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
    }
}
