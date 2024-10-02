use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    subcommand: SubCommands,
}

#[derive(Subcommand, Debug)]
enum SubCommands {
    #[command(version, about, long_about = None)]
    GenerateHeader,
}

fn main() {
    let args = Args::parse();

    match args.subcommand {
        SubCommands::GenerateHeader => {
            let main_crate = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
            let bindings = cbindgen::generate(&main_crate).unwrap();
            let destination_path = main_crate.join("src/rust.generated.hpp");
            bindings.write_to_file(&destination_path);

            println!("Generated bindings to {:?}", destination_path);
        }
    }
}
