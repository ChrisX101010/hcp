use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FpgaError {
    #[error("Board not supported: {0}")]
    UnsupportedBoard(String),
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    #[error("Synthesis failed: {0}")]
    SynthesisError(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, FpgaError>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Board {
    Ice40,
    Ecp5,
}

pub struct FpgaDeployer {
    pub board: Board,
    pub work_dir: String,
}

impl FpgaDeployer {
    pub async fn synthesize(&self, verilog_dir: &str) -> Result<Vec<u8>> {
        tracing::info!("Generating Yosys script...");
        let script = format!(
            "read_verilog {dir}/*.sv\nhierarchy -check -top counter_ecc\nproc; opt; memory; opt\ntechmap; opt\nabc -g cmos2; opt\nwrite_verilog -noattr {dir}/synth.v",
            dir = verilog_dir
        );
        std::fs::write(format!("{}/synth.ys", self.work_dir), script)?;
        tracing::info!("Script written to synth.ys");

        if !Command::new("yosys").arg("-V").output().is_ok() {
            return Err(FpgaError::ToolNotFound("yosys".to_string()));
        }

        tracing::info!("Running Yosys synthesis...");
        let out = Command::new("yosys")
            .arg("-s")
            .arg(format!("{}/synth.ys", self.work_dir))
            .output()?;
        if !out.status.success() {
            return Err(FpgaError::SynthesisError(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ));
        }

        tracing::info!("✓ Synthesis complete");
        Ok(vec![0xAB, 0xCD, 0xEF]) // Placeholder bitstream
    }

    pub async fn flash(&self, bitstream: &[u8]) -> Result<()> {
        let path = format!("{}/output.bin", self.work_dir);
        std::fs::write(&path, bitstream)?;
        let tool = match self.board {
            Board::Ice40 => "iceprog",
            Board::Ecp5 => "openFPGALoader",
        };
        if !Command::new(tool).arg("--version").output().is_ok() {
            return Err(FpgaError::ToolNotFound(tool.to_string()));
        }
        tracing::info!("Flashing with {}...", tool);
        Command::new(tool).arg(&path).status()?;
        Ok(())
    }
}
