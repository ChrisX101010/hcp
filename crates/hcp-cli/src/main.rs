//! HCP — Hardware Context Protocol CLI

use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod commands;
use commands::{CompileCmd, PackageCmd, ServeCmd, SimulateCmd, FpgaCmd, NodeCmd};

/// Hardware Context Protocol — MCP for Hardware
#[derive(Parser)]
#[command(name = "hcp")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    #[arg(short, long, global = true)]
    verbose: bool,
    #[arg(short, long, global = true, default_value = "output")]
    output: String,
}

#[derive(Subcommand)]
enum Commands {
    Compile(CompileCmd),
    Package(PackageCmd),
    Serve(ServeCmd),
    Simulate(SimulateCmd),
    #[command(subcommand)]
    Fpga(FpgaCmd),
    #[command(subcommand)]
    Node(NodeCmd),
    Completion { shell: clap_complete::Shell },
    Demo,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(format!("hcp={}", log_level)))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    if matches!(cli.command, Commands::Demo) {
        print_banner();
    }

    match cli.command {
        Commands::Compile(cmd) => commands::compile::run(cmd, &cli.output),
        Commands::Package(cmd) => commands::package::run(cmd, &cli.output),
        Commands::Serve(cmd) => commands::serve::run(cmd),
        Commands::Simulate(cmd) => commands::simulate::run(cmd, &cli.output),
        Commands::Fpga(cmd) => commands::fpga::run(cmd, &cli.output),
        Commands::Node(cmd) => commands::node::run(cmd),
        Commands::Completion { shell } => {
            use clap::CommandFactory;
            clap_complete::generate(shell, &mut Cli::command(), "hcp", &mut std::io::stdout());
            Ok(())
        }
        Commands::Demo => commands::demo::run(&cli.output),
    }
}

fn print_banner() {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║     HCP — Hardware Context Protocol v{}                      ║", env!("CARGO_PKG_VERSION"));
    println!("║     Compile → Package → Serve → Simulate → Deploy               ║");
    println!("║                                                                  ║");
    println!("║     Dedicated to the memory of Zoran Modli (1948-2020)           ║");
    println!("║     and the Galaksija movement — hardware for everyone.          ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");
}
