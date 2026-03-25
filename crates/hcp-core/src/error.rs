//! Error types for the HCP core library.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum HcpError {
    #[error("Signal width mismatch: expected {expected} bits, got {actual} bits")]
    WidthMismatch { expected: usize, actual: usize },

    #[error("Port '{port}' not found in module '{module}'")]
    PortNotFound { port: String, module: String },

    #[error("ECC scheme '{scheme}' is not supported for signal width {width}")]
    UnsupportedEcc { scheme: String, width: usize },

    #[error("Module '{0}' has no clock input — cannot use sequential logic")]
    NoClock(String),
}
