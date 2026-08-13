use anyhow::Result;
use clap::Args;

#[derive(Args, Debug)]
pub struct ServeCmd {
    #[arg(short, long, default_value = "127.0.0.1:8080")]
    pub bind: String,
    #[arg(long)]
    pub client: bool,
}

pub fn run(cmd: ServeCmd) -> Result<()> {
    if cmd.client {
        println!("📡 HCP client mode — connecting to {}", cmd.bind);
    } else {
        println!("🖥️  HCP server mode — listening on {}", cmd.bind);
    }
    Ok(())
}
