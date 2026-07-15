use clap::Parser;
use herald::cli::Cli;

fn main() {
    let cli = Cli::parse();
    std::process::exit(herald::commands::run(cli));
}
