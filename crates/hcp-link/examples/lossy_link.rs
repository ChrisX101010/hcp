//! Ship a "bitstream" over a lossy radio link and get it back intact.
//!
//! This is the full receive-side story, the gr-satellites way:
//!   payload -> FEC frames -> [lossy link drops ~30%] -> reassemble -> payload
//!
//! Run:  cargo run -p hcp-link --example lossy_link

use hcp_link::{Reassembler, Transmitter};

/// A crude deterministic "radio": drops roughly 1 in `drop_1_in` frames.
fn lossy(frames: &[Vec<u8>], drop_1_in: usize) -> Vec<(usize, Vec<u8>)> {
    let mut out = Vec::new();
    let mut seed = 0x1234_5678u32;
    for (i, f) in frames.iter().enumerate() {
        // simple LCG for repeatable pseudo-random drops
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let drop = (seed as usize % drop_1_in) == 0;
        if drop {
            println!("   link dropped frame {i}");
        } else {
            out.push((i, f.clone()));
        }
    }
    out
}

fn main() {
    // Pretend this is a compiled bitstream. Give it some size so framing matters.
    let bitstream: Vec<u8> = (0..1024).map(|i| ((i * 37 + 11) & 0xff) as u8).collect();
    println!("bitstream: {} bytes", bitstream.len());

    // k=8 data + m=4 parity => 12 frames, tolerant of losing any 4.
    let tx = Transmitter::new(8, 4, 0xBEEF).unwrap();
    let frames = tx.frame(&bitstream).unwrap();
    let frame_size = frames[0].len();
    println!(
        "framed into {} frames of {} bytes ({} data + {} parity)",
        frames.len(),
        frame_size,
        8,
        4
    );
    println!(
        "each frame fits a LoRa packet; can lose any {} of {} frames.\n",
        4,
        frames.len()
    );

    // Send over the lossy link.
    let received = lossy(&frames, 4); // ~25% loss
    println!(
        "\n{} of {} frames survived the link.",
        received.len(),
        frames.len()
    );

    // Reassemble from whatever arrived.
    let mut rx = Reassembler::new();
    for (_, raw) in &received {
        let _ = rx.push(raw);
    }

    if rx.is_complete() {
        let recovered = rx.reconstruct().unwrap();
        let ok = recovered == bitstream;
        println!(
            "reassembled from {} frames -> {} ({} bytes)",
            rx.have(),
            if ok { "EXACT MATCH" } else { "MISMATCH!" },
            recovered.len()
        );
        assert!(ok);
        println!("\nThe design crossed a 25%-loss link and rebuilt perfectly — no retransmit.");
    } else {
        println!(
            "only {} frames arrived; need {}. Would request a re-send of a few frames.",
            rx.have(),
            8
        );
    }
}
