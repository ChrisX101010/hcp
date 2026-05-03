use std::fs::File;
use std::io::Write;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SimError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, SimError>;

#[derive(Debug)]
pub struct SimConfig {
    pub cycles: u64,
    pub inject_errors: bool,
    pub output_dir: String,
}

#[derive(Debug, Default)]
pub struct SimReport {
    pub cycles: u64,
    pub ecc_corrections: u32,
    pub ecc_uncorrectable: u32,
    pub vcd_path: Option<String>,
}

const DATA_POS: [usize; 8] = [3, 5, 6, 7, 9, 10, 11, 12];
const PARITY_POS: [usize; 4] = [1, 2, 4, 8];

pub fn hamming_encode_8(data: u8) -> u16 {
    let mut encoded: u16 = 0;
    for (i, &pos) in DATA_POS.iter().enumerate() {
        if (data >> i) & 1 == 1 { encoded |= 1 << (pos - 1); }
    }
    for &p in &PARITY_POS {
        let mut parity = 0;
        for bit in 1..=13 {
            if bit & p == p && DATA_POS.iter().any(|&d| d == bit) {
                parity ^= (encoded >> (bit - 1)) & 1;
            }
        }
        encoded |= parity << (p - 1);
    }
    let overall = encoded.count_ones() % 2;
    encoded | ((overall as u16) << 12)
}

pub fn hamming_decode_13(encoded: u16) -> (u8, bool, bool) {
    let mut syndrome = 0;
    for &p in &PARITY_POS {
        let mut parity = 0;
        for bit in 1..=13 {
            if bit & p == p {
                parity ^= (encoded >> (bit - 1)) & 1;
            }
        }
        if parity == 1 { syndrome |= p; }
    }
    let overall = encoded.count_ones() % 2;
    let correctable = syndrome != 0 && overall == 0;
    let uncorrectable = syndrome != 0 && overall == 1;

    let mut data = 0u8;
    let mut fixed = encoded;
    if correctable { fixed ^= 1 << (syndrome - 1); }
    for (i, &pos) in DATA_POS.iter().enumerate() {
        if (fixed >> (pos - 1)) & 1 == 1 { data |= 1 << i; }
    }
    (data, correctable, uncorrectable)
}

pub fn run_simulation(config: SimConfig) -> Result<SimReport> {
    let mut report = SimReport { cycles: config.cycles, ..Default::default() };
    let mut vcd = File::create(format!("{}/simulation.vcd", config.output_dir))?;
    writeln!(vcd, "$version\nHCP Simulator v0.4.0\n$end\n$timescale 1 ns $end")?;
    writeln!(vcd, "$scope module top $end")?;
    writeln!(vcd, "$var wire 1 ! clk $end\n$var wire 8 # count $end\n$var wire 13 $ encoded $end\n$upscope $end\n$end")?;

    let mut count: u8 = 0;
    let mut trace = Vec::new();

    for cycle in 0..config.cycles {
        let clk = (cycle % 2 == 0) as u8;
        let encoded = hamming_encode_8(count);
        let (_decoded, corrected, uncorrectable) = hamming_decode_13(encoded);

        if corrected { report.ecc_corrections += 1; }
        if uncorrectable { report.ecc_uncorrectable += 1; }

        trace.push((cycle, count, encoded, clk));
        writeln!(vcd, "b{} ${}\n", encoded, cycle * 5)?;
        writeln!(vcd, "b{:08b} #{}\n", count, cycle * 5)?;
        writeln!(vcd, "{} !{}\n", if clk == 1 {'1'} else {'0'}, cycle * 5)?;
        if clk == 1 { count = count.wrapping_add(1); }
    }
    writeln!(vcd, "$end")?;
    report.vcd_path = Some(format!("{}/simulation.vcd", config.output_dir));

    println!("\n  [Sim] ASCII signal trace (first 16 cycles):\n");
    print!("                    clk │ ");
    for t in trace.iter().take(16) { print!("{} ", if t.3==1 {'█'} else {'░'}); }
    println!();
    print!("                  count │ ");
    for t in trace.iter().take(16) { print!("{:>3} ", t.1); }
    println!();
    print!("          count_encoded │ ");
    for t in trace.iter().take(16) { print!("{:>3} ", t.2); }
    println!("\n");

    Ok(report)
}
