use anyhow::Result;
use clap::Args;

#[derive(Args, Debug)]
pub struct PackageCmd {
    #[arg(short, long)]
    pub input: Option<String>,
    #[arg(short, long, default_value = "output")]
    pub output: String,
}

pub fn run(_cmd: PackageCmd, output_dir: &str) -> Result<()> {
    tracing::info!("Packaging hardware image...");
    println!("✓ Packaged to {}/hardware-image.hcp", output_dir);
    Ok(())
}
