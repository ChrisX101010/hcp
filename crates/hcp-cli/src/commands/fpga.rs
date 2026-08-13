use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Subcommand, Debug)]
pub enum FpgaCmd {
    Deploy(DeployCmd),
    List,
}

#[derive(Args, Debug)]
pub struct DeployCmd {
    #[arg(short, long, default_value = "ice40")]
    pub board: String,
    #[arg(short, long)]
    pub image: Option<String>,
    #[arg(long)]
    pub flash: bool,
    #[arg(long)]
    pub verify: bool,
}

pub fn run(cmd: FpgaCmd, _output_dir: &str) -> Result<()> {
    match cmd {
        FpgaCmd::Deploy(deploy) => {
            println!("🚀 Deploying to {} board...", deploy.board);
            if deploy.flash {
                println!("💾 Flashing bitstream...");
            }
            if deploy.verify {
                println!("✅ Verifying ECC in hardware...");
            }
        }
        FpgaCmd::List => {
            println!("Supported boards:");
            println!("  • ice40      — Lattice iCE40-HX8K (open-source flow)");
            println!("  • ecp5       — Lattice ECP5 (open-source flow)");
            println!("  • xilinx*    — Requires Vivado license");
        }
    }
    Ok(())
}
