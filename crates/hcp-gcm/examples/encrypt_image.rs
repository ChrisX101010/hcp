//! Encrypt a hardware image for transit, the way the fabric layer would.
//!
//! The image's SHA-256 digest (already used as `image_digest` in a
//! `PlacementOffer`) goes in as GCM's associated data — so the ciphertext is
//! cryptographically bound to the exact design being offered. Tamper with
//! either the encrypted image or the claimed digest and decryption fails.
//!
//! Run:  cargo run -p hcp-gcm --example encrypt_image

use hcp_gcm::{Aes128Gcm, SequenceNonce};

fn main() {
    // A session key would come from the pairing/key-exchange step; fixed here.
    let session_key = [0x42u8; 16];
    let gcm = Aes128Gcm::new(&session_key);

    // Pretend this is a compiled bitstream.
    let image = b"BITSTREAM:aes128_enc:971LUT/399FF/20BRAM: ...binary...";
    // Its content address (normally a real SHA-256). Bound in as AAD.
    let image_digest = b"sha256:1122334455667788990011223344556677889900112233445566778899001122";

    // A fresh, never-repeated nonce per message.
    let mut nonces = SequenceNonce::new([0xDE, 0xAD, 0xBE, 0xEF]);
    let nonce = nonces.next().expect("nonce");

    let (ciphertext, tag) = gcm.encrypt(&nonce, image_digest, image);
    println!("plaintext image : {} bytes", image.len());
    println!("ciphertext      : {} bytes", ciphertext.len());
    println!("auth tag        : {} bytes", tag.len());
    println!("nonce           : {}", hex(&nonce));

    // Receiver: same key + nonce + digest recovers the image and proves integrity.
    match gcm.decrypt(&nonce, image_digest, &ciphertext, &tag) {
        Ok(recovered) => {
            println!(
                "\ndecrypt OK — image verified and recovered ({} bytes)",
                recovered.len()
            );
            assert_eq!(recovered, image);
        }
        Err(e) => println!("decrypt failed: {e}"),
    }

    // If an attacker lies about which design this is (wrong digest), it fails.
    let wrong_digest = b"sha256:0000000000000000000000000000000000000000000000000000000000000000";
    match gcm.decrypt(&nonce, wrong_digest, &ciphertext, &tag) {
        Ok(_) => println!("\n(unexpected) decrypt succeeded with wrong digest"),
        Err(e) => println!("\ndigest mismatch correctly rejected: {e}"),
    }

    println!("\nThe image is bound to its identity — you can't swap the design out from under the offer.");
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
