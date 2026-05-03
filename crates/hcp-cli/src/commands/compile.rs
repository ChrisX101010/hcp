use clap::Args;
use anyhow::Result;

/// Compile HDL source with ECC protection
#[derive(Args, Debug)]
pub struct CompileCmd {
    /// Input HDL file (Rust module definition or Verilog)
    #[arg(short, long)]
    pub input: Option<String>,

    /// ECC scheme: hamming-sec-ded, bch, reed-solomon
    #[arg(short, long, default_value = "hamming-sec-ded")]
    pub ecc: String,

    /// Target bit-width for ECC encoding
    #[arg(short, long, default_value = "8")]
    pub width: usize,

    /// Output Verilog file
    #[arg(short, long)]
    pub output: Option<String>,
}

pub fn run(cmd: CompileCmd, output_dir: &str) -> Result<()> {
    tracing::info!("Compiling with ECC scheme: {}", cmd.ecc);
    
    // Delegate to hcp-hdl crate
    let result = hcp_hdl::compile(
        cmd.input.as_deref(),
        &cmd.ecc,
        cmd.width,
        output_dir,
    )?;
    
    println!("✓ Compiled {} → {} (ECC overhead: {:.1}%)", 
        cmd.input.as_deref().unwrap_or("inline module"),
        result.verilog_path,
        result.ecc_overhead_pct
    );
    
    Ok(())
}
