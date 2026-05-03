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
    /// Manually connect to a peer at <IP>:<PORT>
    #[arg(long)]
    pub connect: Option<String>,
}

pub fn run(cmd: NodeCmd) -> Result<()> {
    match cmd {
        NodeCmd::Start(args) => {
            println!("🌐 Starting HCP mesh node on port {}...", args.port);
            if let Some(ref addr) = args.connect {
                println!("🔗 Manually connecting to: {}", addr);
            }
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                let registry = Arc::new(ImageRegistry::new());
                let mut node = MeshNode::new(args.port, registry);
                if let Some(addr) = args.connect {
                    node.add_initial_peer(addr);
                }
                if let Err(e) = node.run().await { eprintln!("Mesh node error: {}", e); }
            });
        }
        NodeCmd::List => { println!("Peer listing requires a running node daemon."); }
    }
    Ok(())
}
