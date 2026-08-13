//! End-to-end known-answer test: generate the Verilog from Rust, then simulate
//! it against the FIPS-197 vectors with Icarus Verilog.
//!
//! If `iverilog` is not on PATH the test prints a notice and passes, so `cargo
//! test` still works on machines without an HDL toolchain. Set
//! `HCP_REQUIRE_IVERILOG=1` in CI to turn a missing simulator into a failure.

use std::env;
use std::fs;
use std::process::Command;

fn have_iverilog() -> bool {
    Command::new("iverilog")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn generated_rtl_passes_fips197_vectors() {
    if !have_iverilog() {
        if env::var("HCP_REQUIRE_IVERILOG").as_deref() == Ok("1") {
            panic!("iverilog not found but HCP_REQUIRE_IVERILOG=1");
        }
        eprintln!("note: iverilog not found; skipping RTL simulation test");
        eprintln!("      install with: apt install iverilog  /  brew install icarus-verilog");
        return;
    }

    let dir = env::temp_dir().join("hcp_crypto_kat");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");

    let rtl = dir.join("aes128_enc.sv");
    fs::write(&rtl, hcp_crypto::generate_aes128_enc()).expect("write rtl");

    // The testbench lives next to the crate so it ships with the source.
    let tb = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tb")
        .join("aes128_enc_tb.sv");
    assert!(tb.exists(), "testbench missing at {}", tb.display());

    let exe = dir.join("sim");
    let compile = Command::new("iverilog")
        .args(["-g2012", "-o"])
        .arg(&exe)
        .arg(&rtl)
        .arg(&tb)
        .output()
        .expect("run iverilog");
    assert!(
        compile.status.success(),
        "iverilog failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new("vvp").arg(&exe).output().expect("run vvp");
    let stdout = String::from_utf8_lossy(&run.stdout);
    eprintln!("{stdout}");

    assert!(
        stdout.contains("ALL TESTS PASSED"),
        "simulation did not report success:\n{stdout}"
    );
    assert!(!stdout.contains("FAIL"), "a vector failed:\n{stdout}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn every_generated_sbox_arm_matches_the_field() {
    let v = hcp_crypto::generate_aes128_enc();
    let t = hcp_crypto::sbox_table();
    for i in 0..256usize {
        let needle = format!("8'h{:02x}: sbox = 8'h{:02x};", i, t[i]);
        assert!(v.contains(&needle), "missing S-box arm for {:#04x}", i);
    }
}

#[test]
fn emitted_module_is_self_contained() {
    let v = hcp_crypto::generate_aes128_enc();
    assert_eq!(v.matches("module aes128_enc").count(), 1);
    assert_eq!(v.matches("endmodule").count(), 1);
    // Must not reference anything we do not define in the same file.
    for undefined in ["hamming_enc", "hamming_dec", "$display", "$finish"] {
        assert!(
            !v.contains(undefined),
            "generated RTL should not contain {undefined}"
        );
    }

    // No `initial` blocks: they are simulation-only and not synthesizable.
    // Checked per-line so the word may still appear in comments.
    for line in v.lines() {
        let code = line.split("//").next().unwrap_or("").trim();
        assert!(
            !code.starts_with("initial"),
            "generated RTL must not contain an initial block: {line}"
        );
    }
}
