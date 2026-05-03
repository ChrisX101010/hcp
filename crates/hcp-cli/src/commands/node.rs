use anyhow::Result;
use clap::{Args, Subcommand};
use hcp_protocol::{MeshNode, ImageRegistry};
use std::sync::Arc;

#[derive(Subcommand, Debug)]
pub enum NodeCmd {
    Start(StartArgs),
    List,
}

#[derive(Args, Debug)]
pub struct StartArgs {
    #[arg(short, long, default_value = "8080")]
    pub port: u16,
}

pub fn run(cmd: NodeCmd) -> Result<()> {
    match cmd {
        NodeCmd::Start(args) => {
            println!("🌐 Starting HCP mesh node on port {}...", args.port);
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                let registry = Arc::new(ImageRegistry::new());
                let node = MeshNode::new(args.port, registry);
                if let Err(e) = node.run().await { eprintln!("Mesh node error: {}", e); }
            });
        }
        NodeCmd::List => { println!("Peer listing requires a running node daemon."); }
    }
    Ok(())
}
