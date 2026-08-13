//! Framing: turn an arbitrary payload into self-describing FEC frames, and
//! reassemble the payload from whatever frames survive the link.
//!
//! A [`Transmitter`] splits a payload into `k` data shards, Reed-Solomon-encodes
//! them into `n = k + m` shards, and wraps each in a [`Frame`] carrying enough
//! metadata to stand alone: which stream it belongs to, its index, the total
//! payload length, and a CRC. Frames go out over the radio in any order; some
//! are lost or corrupted. A [`Reassembler`] collects the good ones (CRC filters
//! corrupted frames, which then count as erasures) and, once it has any `k`,
//! rebuilds the exact original payload.
//!
//! This is the gr-satellites lesson applied to hardware images: a bitstream is
//! just a file, and FEC + reassembly is how files cross bad links.

use crate::rs::{ReedSolomon, RsError};

/// Wire-format overhead per frame, in bytes:
/// magic(2) + ver(1) + stream_id(4) + k(1) + m(1) + index(1)
/// + total_len(4) + shard_len(2) + crc32(4)
const HEADER_LEN: usize = 2 + 1 + 4 + 1 + 1 + 1 + 4 + 2 + 4;
const CRC_POS: usize = 16;
const MAGIC: [u8; 2] = *b"HL"; // "HCP Link"
const VERSION: u8 = 1;

/// Errors from framing / reassembly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    TooShort,
    BadMagic,
    BadVersion(u8),
    CrcMismatch,
    /// Frames from different streams or with inconsistent parameters were mixed.
    Inconsistent,
    Rs(RsError),
    /// Payload too large for the chosen shard count at u16 shard length.
    PayloadTooLarge,
    BadParams,
}

impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FrameError::TooShort => write!(f, "frame shorter than header"),
            FrameError::BadMagic => write!(f, "not an hcp-link frame"),
            FrameError::BadVersion(v) => write!(f, "unsupported frame version {v}"),
            FrameError::CrcMismatch => write!(f, "frame CRC mismatch (corrupted)"),
            FrameError::Inconsistent => write!(f, "frames inconsistent or from different streams"),
            FrameError::Rs(e) => write!(f, "erasure coding: {e}"),
            FrameError::PayloadTooLarge => write!(f, "payload too large for parameters"),
            FrameError::BadParams => write!(f, "invalid framing parameters"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<RsError> for FrameError {
    fn from(e: RsError) -> Self {
        FrameError::Rs(e)
    }
}

/// A single decoded frame's metadata plus its shard bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub stream_id: u32,
    pub k: u8,
    pub m: u8,
    pub index: u8,
    pub total_len: u32,
    pub shard: Vec<u8>,
}

impl Frame {
    /// Serialize to bytes for the radio: header + shard, CRC over the whole
    /// frame with the CRC field itself treated as zero.
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(HEADER_LEN + self.shard.len());
        v.extend_from_slice(&MAGIC);
        v.push(VERSION);
        v.extend_from_slice(&self.stream_id.to_be_bytes());
        v.push(self.k);
        v.push(self.m);
        v.push(self.index);
        v.extend_from_slice(&self.total_len.to_be_bytes());
        v.extend_from_slice(&(self.shard.len() as u16).to_be_bytes());
        v.extend_from_slice(&[0u8; 4]); // CRC placeholder at CRC_POS
        v.extend_from_slice(&self.shard);
        let crc = crc32_with_gap(&v, CRC_POS);
        v[CRC_POS..CRC_POS + 4].copy_from_slice(&crc.to_be_bytes());
        v
    }

    /// Parse and CRC-check a received frame. A CRC failure returns
    /// `CrcMismatch`, which the reassembler treats as a lost (erased) frame.
    pub fn decode(bytes: &[u8]) -> Result<Frame, FrameError> {
        if bytes.len() < HEADER_LEN {
            return Err(FrameError::TooShort);
        }
        if bytes[0..2] != MAGIC {
            return Err(FrameError::BadMagic);
        }
        if bytes[2] != VERSION {
            return Err(FrameError::BadVersion(bytes[2]));
        }
        let stream_id = u32::from_be_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]);
        let k = bytes[7];
        let m = bytes[8];
        let index = bytes[9];
        let total_len = u32::from_be_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
        let shard_len = u16::from_be_bytes([bytes[14], bytes[15]]) as usize;
        let claimed = u32::from_be_bytes([
            bytes[CRC_POS],
            bytes[CRC_POS + 1],
            bytes[CRC_POS + 2],
            bytes[CRC_POS + 3],
        ]);
        if bytes.len() < HEADER_LEN + shard_len {
            return Err(FrameError::TooShort);
        }
        let computed = crc32_with_gap(&bytes[..HEADER_LEN + shard_len], CRC_POS);
        if computed != claimed {
            return Err(FrameError::CrcMismatch);
        }
        let shard = bytes[HEADER_LEN..HEADER_LEN + shard_len].to_vec();
        Ok(Frame {
            stream_id,
            k,
            m,
            index,
            total_len,
            shard,
        })
    }
}

/// Splits a payload into transmittable frames.
pub struct Transmitter {
    k: usize,
    m: usize,
    rs: ReedSolomon,
    stream_id: u32,
}

impl Transmitter {
    /// `k` data shards, `m` parity shards. Up to `m` frames may be lost.
    pub fn new(k: usize, m: usize, stream_id: u32) -> Result<Self, FrameError> {
        if k == 0 || m == 0 || k + m > 255 {
            return Err(FrameError::BadParams);
        }
        let rs = ReedSolomon::new(k, m)?;
        Ok(Transmitter {
            k,
            m,
            rs,
            stream_id,
        })
    }

    /// Frame a payload. Returns `n = k + m` encoded byte frames ready to send.
    pub fn frame(&self, payload: &[u8]) -> Result<Vec<Vec<u8>>, FrameError> {
        let total_len = payload.len();
        if total_len > u32::MAX as usize {
            return Err(FrameError::PayloadTooLarge);
        }
        // shard length = ceil(total_len / k); all shards padded to equal length.
        let shard_len = total_len.div_ceil(self.k).max(1);
        if shard_len > u16::MAX as usize {
            return Err(FrameError::PayloadTooLarge);
        }

        // build k data shards, zero-padded
        let mut data = vec![vec![0u8; shard_len]; self.k];
        for (i, byte) in payload.iter().enumerate() {
            data[i / shard_len][i % shard_len] = *byte;
        }

        let all = self.rs.encode(&data)?;
        let mut frames = Vec::with_capacity(all.len());
        for (index, shard) in all.into_iter().enumerate() {
            let frame = Frame {
                stream_id: self.stream_id,
                k: self.k as u8,
                m: self.m as u8,
                index: index as u8,
                total_len: total_len as u32,
                shard,
            };
            frames.push(frame.encode());
        }
        Ok(frames)
    }
}

/// Collects received frames and reconstructs the payload once enough arrive.
pub struct Reassembler {
    stream_id: Option<u32>,
    k: Option<usize>,
    m: Option<usize>,
    total_len: Option<usize>,
    shards: Vec<(usize, Vec<u8>)>,
    seen: Vec<bool>,
}

impl Reassembler {
    pub fn new() -> Self {
        Reassembler {
            stream_id: None,
            k: None,
            m: None,
            total_len: None,
            shards: Vec::new(),
            seen: vec![false; 256],
        }
    }

    /// Feed one received raw frame. Corrupted frames (CRC fail) are silently
    /// dropped — they simply become erasures the FEC will cover. Returns `true`
    /// if this frame was accepted and new.
    pub fn push(&mut self, raw: &[u8]) -> Result<bool, FrameError> {
        let frame = match Frame::decode(raw) {
            Ok(f) => f,
            Err(FrameError::CrcMismatch) => return Ok(false), // treat as erasure
            Err(e) => return Err(e),
        };

        match self.stream_id {
            None => {
                self.stream_id = Some(frame.stream_id);
                self.k = Some(frame.k as usize);
                self.m = Some(frame.m as usize);
                self.total_len = Some(frame.total_len as usize);
            }
            Some(id) => {
                if id != frame.stream_id
                    || self.k != Some(frame.k as usize)
                    || self.m != Some(frame.m as usize)
                    || self.total_len != Some(frame.total_len as usize)
                {
                    return Err(FrameError::Inconsistent);
                }
            }
        }

        let idx = frame.index as usize;
        if self.seen[idx] {
            return Ok(false); // duplicate
        }
        self.seen[idx] = true;
        self.shards.push((idx, frame.shard));
        Ok(true)
    }

    /// How many distinct good frames have been collected.
    pub fn have(&self) -> usize {
        self.shards.len()
    }

    /// Whether enough frames are present to reconstruct.
    pub fn is_complete(&self) -> bool {
        matches!(self.k, Some(k) if self.have() >= k)
    }

    /// Attempt reconstruction. Returns the exact original payload once `k`
    /// distinct frames have been collected.
    pub fn reconstruct(&self) -> Result<Vec<u8>, FrameError> {
        let k = self.k.ok_or(FrameError::Inconsistent)?;
        let m = self.m.ok_or(FrameError::Inconsistent)?;
        let total_len = self.total_len.ok_or(FrameError::Inconsistent)?;
        if self.have() < k {
            return Err(FrameError::Rs(RsError::NotEnoughShards {
                have: self.have(),
                need: k,
            }));
        }
        let rs = ReedSolomon::new(k, m)?;
        let data = rs.reconstruct(&self.shards)?;
        let mut out = Vec::with_capacity(k * data[0].len());
        for shard in &data {
            out.extend_from_slice(shard);
        }
        out.truncate(total_len);
        Ok(out)
    }
}

impl Default for Reassembler {
    fn default() -> Self {
        Self::new()
    }
}

// ---- CRC-32 (IEEE 802.3, reflected) ---------------------------------------

/// CRC-32 over `data`, treating the 4-byte field at `gap` as zero (so the CRC
/// can live inside the framed bytes it protects).
fn crc32_with_gap(data: &[u8], gap: usize) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for (i, &b) in data.iter().enumerate() {
        let byte = if i >= gap && i < gap + 4 { 0 } else { b };
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_no_loss() {
        let tx = Transmitter::new(4, 2, 0xABCD).unwrap();
        let payload = b"a compiled bitstream for aes128_enc, 971 LUT / 20 BRAM".to_vec();
        let frames = tx.frame(&payload).unwrap();
        assert_eq!(frames.len(), 6);

        let mut rx = Reassembler::new();
        for f in &frames {
            rx.push(f).unwrap();
        }
        assert!(rx.is_complete());
        assert_eq!(rx.reconstruct().unwrap(), payload);
    }

    #[test]
    fn recovers_after_losing_m_frames() {
        let tx = Transmitter::new(4, 3, 1).unwrap(); // can lose 3 of 7
        let payload: Vec<u8> = (0..200).map(|i| (i * 7 % 251) as u8).collect();
        let frames = tx.frame(&payload).unwrap();

        // deliver only frames 2,3,4,5 (lose 0,1,6 — three losses)
        let mut rx = Reassembler::new();
        for i in [2usize, 3, 4, 5] {
            rx.push(&frames[i]).unwrap();
        }
        assert!(rx.is_complete());
        assert_eq!(rx.reconstruct().unwrap(), payload);
    }

    #[test]
    fn corrupted_frame_is_ignored_as_erasure() {
        let tx = Transmitter::new(3, 2, 7).unwrap();
        let payload = b"hardware over radio".to_vec();
        let mut frames = tx.frame(&payload).unwrap();

        // corrupt frame 0's body (flip a byte in the shard region)
        let last = frames[0].len() - 1;
        frames[0][last] ^= 0xFF;

        let mut rx = Reassembler::new();
        let accepted0 = rx.push(&frames[0]).unwrap();
        assert!(!accepted0, "corrupted frame must be rejected");
        for i in 1..frames.len() {
            rx.push(&frames[i]).unwrap();
        }
        assert!(rx.is_complete());
        assert_eq!(rx.reconstruct().unwrap(), payload);
    }

    #[test]
    fn not_enough_frames_reports_shortfall() {
        let tx = Transmitter::new(4, 2, 1).unwrap();
        let payload = vec![1u8; 100];
        let frames = tx.frame(&payload).unwrap();
        let mut rx = Reassembler::new();
        rx.push(&frames[0]).unwrap();
        rx.push(&frames[1]).unwrap();
        assert!(!rx.is_complete());
        assert!(matches!(
            rx.reconstruct(),
            Err(FrameError::Rs(RsError::NotEnoughShards {
                have: 2,
                need: 4
            }))
        ));
    }

    #[test]
    fn mixing_streams_is_rejected() {
        let tx1 = Transmitter::new(3, 2, 100).unwrap();
        let tx2 = Transmitter::new(3, 2, 200).unwrap();
        let f1 = tx1.frame(b"payload one xx").unwrap();
        let f2 = tx2.frame(b"payload two yy").unwrap();
        let mut rx = Reassembler::new();
        rx.push(&f1[0]).unwrap();
        assert!(matches!(rx.push(&f2[0]), Err(FrameError::Inconsistent)));
    }

    #[test]
    fn duplicate_frames_are_idempotent() {
        let tx = Transmitter::new(3, 2, 1).unwrap();
        let frames = tx.frame(b"data payload!!").unwrap();
        let mut rx = Reassembler::new();
        assert!(rx.push(&frames[0]).unwrap());
        assert!(!rx.push(&frames[0]).unwrap()); // dup
        assert_eq!(rx.have(), 1);
    }

    #[test]
    fn single_byte_payload() {
        let tx = Transmitter::new(3, 2, 1).unwrap();
        let frames = tx.frame(&[0x42]).unwrap();
        let mut rx = Reassembler::new();
        for i in [4usize, 3, 2] {
            rx.push(&frames[i]).unwrap();
        }
        assert_eq!(rx.reconstruct().unwrap(), vec![0x42]);
    }

    #[test]
    fn crc_detects_header_corruption() {
        let tx = Transmitter::new(3, 2, 1).unwrap();
        let frames = tx.frame(b"xyz").unwrap();
        let mut bad = frames[0].clone();
        bad[9] ^= 0x01; // corrupt the index byte in the header
        assert_eq!(Frame::decode(&bad), Err(FrameError::CrcMismatch));
    }
}
