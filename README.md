# HCP — Hardware Context Protocol

**Ship hardware designs anywhere, run them on fabric you trust.**

HCP is a hardware-software co-design framework and a consent-based protocol for
sharing reconfigurable fabric across a network. You write hardware in Rust; the
compiler generates synthesizable Verilog with error correction baked in;
designs are packaged as content-addressed images, encrypted, framed to survive
lossy radio links, and placed — only by consent — on remote FPGA fabric.

Dedicated to Zoran Modli, who broadcast software over FM radio from Belgrade in
1983. HCP does the same thing with *hardware*.

---

## The one-line pitch

> **SatNOGS coordinates ground stations that share radio observations. HCP
> coordinates nodes that share reconfigurable hardware** — shipping
> FEC-protected, encrypted, signed designs over any link, so a design compiled
> on your laptop can run on fabric across the room or across the world.

Remote FPGA execution exists (AWS F1, Microsoft Catapult) — but it's
proprietary and datacenter-locked. HCP is the **open, self-hostable,
transport-agnostic, consent-based** version. Nobody else offers "npm-for-circuits
that runs over any radio, survives bad links, and shares fabric by consent."

## Why now

Published security research on FPGA-as-a-Service (IACR ePrint 2021/746, *Remote
Exploitation of FaaS Platforms*) shows the core hazard of sharing fabric: a
hostile partial-reconfiguration bitstream can attack the host. Cloud FaaS trusts
"the customer paid." HCP's trust model is fundamentally different and directly
answers that gap: **identity + integrity + consent + admission control** at
every step.

---

## Architecture

```
   write hardware in Rust
        │  hcp-core / hcp-hdl / hcp-ecc   compile → Verilog, ECC baked in
        ▼
   hcp-package        content-addressed OCI image + hcp.json manifest
        │  hcp-gcm    encrypt + authenticate (AES-128-GCM, image bound to digest)
        │  hcp-link   Reed-Solomon FEC frames — survive a lossy radio link
        │  <transport>  BLE / LoRa / WiFi / serial actually moves the bytes
        ▼
   remote node
        │  hcp-identity   verify who signed the offer (Ed25519)
        │  hcp-fabric     consent handshake: advertise → offer → decide
        │  hcp-admit      host policy gate: resources, class, allow/denylist
        │  hcp-mesh       live map: who's online, who can host, best route
        ▼
   design runs on trusted fabric
```

## Crates

| Crate | Role | Verified against |
| --- | --- | --- |
| `hcp-core`, `hcp-hdl`, `hcp-ecc` | Rust → Verilog compiler with automatic Hamming SEC-DED ECC | — |
| `hcp-package` | OCI hardware images, SHA-256 content addressing, `hcp.json` | — |
| `hcp-protocol` | JSON-RPC 2.0 server / client / registry | — |
| `hcp-crypto` | AES-128 hardware core + Rust generator | **FIPS-197** known-answer vectors |
| `hcp-gcm` | AES-128-GCM authenticated encryption | **NIST/McGrew-Viega** GCM vectors |
| `hcp-identity` | Ed25519 node identity + trust store | signed-offer round trips |
| `hcp-fabric` | Consent-based capability / offer / decision protocol | 22 protocol tests |
| `hcp-link` | Reed-Solomon FEC framing for lossy links | exhaustive loss-recovery |
| `hcp-mesh` | Consent-only mesh map, topology, quality-weighted routing | 14 topology tests |
| `hcp-admit` | Bitstream admission control (the FaaS-security gap) | 9 policy tests |

Every cryptographic and coding component is checked against published standard
test vectors, not just self-consistency — the same rigor the `nsacyber` projects
model (reproducible, verifiable, signed).

## What makes it original

1. **Transport-agnostic by design.** The control plane is tiny (a capability
   advertisement is 73 bytes) because HCP ships *designs*, not data streams — so
   it rides WiFi, BLE, LoRa, even 280-baud FM, exactly like Ventilator 202.
2. **Integrity everywhere.** ECC in every signal (hardware), Reed-Solomon across
   frames (link), GCM tag per message (transport), SHA-256 per image (identity).
   Built for bad links, not perfect datacenter fabric.
3. **Consent is structural, not bolted on.** A node is only ever a placement
   target if it *advertised* capacity and *accepted* a *signed* offer that
   *passed its own admission policy*. There is no discover-and-commandeer path.

## Quick start

```bash
cargo build --workspace
cargo test  --workspace          # standard-vector-verified crypto + FEC + protocol

# See the pieces work:
cargo run -p hcp-crypto --example emit          # generate the AES core
cargo run -p hcp-gcm    --example encrypt_image # encrypt a design, bound to its digest
cargo run -p hcp-link   --example lossy_link    # ship it over a 25%-loss link
cargo run -p hcp-identity --example paired_handshake  # signed placement, spoof rejected
cargo run -p hcp-mesh   --example mesh_map      # live fleet map + routing
```

Optional hardware verification (needs `iverilog` + `yosys`):

```bash
just aes-sim     # simulate the AES core against FIPS-197
just aes-synth   # synthesize for iCE40, print resource usage
```

## Roadmap

| Phase | What | Status |
| --- | --- | --- |
| 1–3 | HDL compiler, ECC pass, OCI images, JSON-RPC server | ✅ done |
| 4 | Crypto + identity + AEAD (`hcp-crypto`, `hcp-gcm`, `hcp-identity`) | ✅ done |
| 5 | Fabric sharing + FEC transport + mesh (`hcp-fabric`, `hcp-link`, `hcp-mesh`) | ✅ done |
| 6 | Admission control (`hcp-admit`) | ✅ done |
| 7 | Real transports (BLE / LoRa via GNU Radio / URH); placement executor (Verilator) | next |
| 8 | `crypto_pass` into `hcp-hdl` (`#[encrypt(Aes128)]` like `#[ecc(...)]`) | planned |

## Heritage — Zoran Modli & Galaksija

In autumn 1983, Zoran Modli broadcast computer programs over FM on *Ventilator
202*, Radio Beograd. Listeners taped the tones and loaded them into their
Galaksija, ZX Spectrum, or C64. Over three years, 150 programs went out — and
came back modified for re-broadcast: collaborative software distribution a
decade before the web.

| 1983 Belgrade | 2026 HCP |
| --- | --- |
| Software as FSK audio over FM | Hardware as content-addressed images over any link |
| 280 bits/second | Enough — because we ship designs, not streams |
| No error correction | ECC + FEC + AEAD at every layer |
| Tape recorder as download client | `hcp pull`, signed and verified |
| Listeners modify and re-broadcast | Git-style versioning + consent-based mesh |

The principle is identical: encode instructions, transmit through a medium,
rebuild functional computation on the receiver. Modli sent software that tells
hardware what to do. HCP sends the hardware definitions themselves.

Dedicated to Zoran Modli (d. 23 Feb 2020) and the communitarian spirit of the
Galaksija movement — the belief that powerful technology belongs to everyone.

## License

Apache-2.0.
