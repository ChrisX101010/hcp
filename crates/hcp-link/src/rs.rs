//! Reed-Solomon erasure coding over GF(256), Cauchy-matrix style.
//!
//! Split a payload into `k` data shards, generate `m` parity shards, and
//! transmit all `n = k + m`. **Any `k` of the `n` shards reconstruct the
//! original** — so you can lose up to `m` whole frames on a bad radio link and
//! still recover the design. This is the technique behind CCSDS space links and
//! storage systems like Backblaze's; here it's what lets a bitstream survive
//! LoRa/FM dropouts.

use crate::gf256::Gf256;

/// Errors from the erasure codec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RsError {
    /// Fewer than `k` shards were present — reconstruction impossible.
    NotEnoughShards { have: usize, need: usize },
    /// Shard lengths disagreed.
    RaggedShards,
    /// Parameters out of range (k, m must be >=1 and k+m <= 255).
    BadParams,
    /// The selected shard submatrix was singular (should not happen with a
    /// Cauchy matrix and distinct indices; guarded anyway).
    Singular,
}

impl core::fmt::Display for RsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RsError::NotEnoughShards { have, need } => {
                write!(f, "not enough shards: have {have}, need {need}")
            }
            RsError::RaggedShards => write!(f, "shards have differing lengths"),
            RsError::BadParams => write!(f, "invalid (k, m) parameters"),
            RsError::Singular => write!(f, "shard submatrix was singular"),
        }
    }
}

impl std::error::Error for RsError {}

/// A Reed-Solomon erasure coder for a fixed (k data, m parity) split.
pub struct ReedSolomon {
    k: usize,
    m: usize,
    gf: Gf256,
    /// The (k+m) x k encoding matrix: identity on top (systematic), Cauchy below.
    matrix: Vec<Vec<u8>>,
}

impl ReedSolomon {
    /// `k` data shards, `m` parity shards. Requires k>=1, m>=1, k+m<=255.
    pub fn new(k: usize, m: usize) -> Result<Self, RsError> {
        if k == 0 || m == 0 || k + m > 255 {
            return Err(RsError::BadParams);
        }
        let gf = Gf256::new();
        let n = k + m;

        // Systematic encoding matrix. Rows 0..k are the identity (so data shards
        // pass through unchanged). Rows k..n form a Cauchy matrix, which has the
        // property that every square submatrix is invertible over GF(256).
        let mut matrix = vec![vec![0u8; k]; n];
        for i in 0..k {
            matrix[i][i] = 1;
        }
        // Cauchy: entry = 1 / (x_i XOR y_j), with x and y drawn from disjoint
        // sets of distinct field elements. Use x_i = (k + i), y_j = j.
        for i in 0..m {
            let x = (k + i) as u8;
            for j in 0..k {
                let y = j as u8;
                matrix[k + i][j] = gf.inv(x ^ y);
            }
        }

        Ok(ReedSolomon { k, m, gf, matrix })
    }

    pub fn k(&self) -> usize {
        self.k
    }
    pub fn m(&self) -> usize {
        self.m
    }
    pub fn n(&self) -> usize {
        self.k + self.m
    }

    /// Encode `k` equal-length data shards into `n` shards (data + parity).
    pub fn encode(&self, data: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, RsError> {
        if data.len() != self.k {
            return Err(RsError::BadParams);
        }
        let len = data[0].len();
        if data.iter().any(|s| s.len() != len) {
            return Err(RsError::RaggedShards);
        }

        let mut out = Vec::with_capacity(self.n());
        // data shards pass through (systematic)
        for s in data {
            out.push(s.clone());
        }
        // parity shards = Cauchy rows * data
        for i in 0..self.m {
            let row = &self.matrix[self.k + i];
            let mut parity = vec![0u8; len];
            for (j, s) in data.iter().enumerate() {
                let coeff = row[j];
                if coeff != 0 {
                    for b in 0..len {
                        parity[b] ^= self.gf.mul(coeff, s[b]);
                    }
                }
            }
            out.push(parity);
        }
        Ok(out)
    }

    /// Reconstruct the `k` data shards from any `k` (or more) present shards.
    ///
    /// `present` pairs each available shard's original index (0..n) with its
    /// bytes. Order doesn't matter; extra shards beyond `k` are ignored.
    pub fn reconstruct(&self, present: &[(usize, Vec<u8>)]) -> Result<Vec<Vec<u8>>, RsError> {
        if present.len() < self.k {
            return Err(RsError::NotEnoughShards {
                have: present.len(),
                need: self.k,
            });
        }
        let len = present[0].1.len();
        if present.iter().any(|(_, s)| s.len() != len) {
            return Err(RsError::RaggedShards);
        }

        // Take the first k present shards; build the k x k submatrix of their
        // encoding rows, invert it, and multiply by the received bytes.
        let chosen = &present[..self.k];
        let mut sub = vec![vec![0u8; self.k]; self.k];
        for (r, (idx, _)) in chosen.iter().enumerate() {
            sub[r] = self.matrix[*idx].clone();
        }
        let inv = self.invert(&sub)?;

        let mut data = vec![vec![0u8; len]; self.k];
        for (i, row) in inv.iter().enumerate() {
            for b in 0..len {
                let mut acc = 0u8;
                for (r, (_, shard)) in chosen.iter().enumerate() {
                    let c = row[r];
                    if c != 0 {
                        acc ^= self.gf.mul(c, shard[b]);
                    }
                }
                data[i][b] = acc;
            }
        }
        Ok(data)
    }

    /// Gauss-Jordan inversion of a k x k matrix over GF(256).
    fn invert(&self, m: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, RsError> {
        let n = m.len();
        // augmented [ m | I ]
        let mut a: Vec<Vec<u8>> = m
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let mut r = row.clone();
                r.extend((0..n).map(|j| if i == j { 1u8 } else { 0u8 }));
                r
            })
            .collect();

        for col in 0..n {
            // find pivot
            let mut piv = col;
            while piv < n && a[piv][col] == 0 {
                piv += 1;
            }
            if piv == n {
                return Err(RsError::Singular);
            }
            a.swap(col, piv);

            // normalize pivot row
            let inv = self.gf.inv(a[col][col]);
            for x in 0..2 * n {
                a[col][x] = self.gf.mul(a[col][x], inv);
            }
            // eliminate other rows
            for row in 0..n {
                if row != col && a[row][col] != 0 {
                    let factor = a[row][col];
                    for x in 0..2 * n {
                        let t = self.gf.mul(factor, a[col][x]);
                        a[row][x] ^= t;
                    }
                }
            }
        }

        // right half is the inverse
        Ok(a.iter().map(|row| row[n..].to_vec()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shards(data: &[&[u8]]) -> Vec<Vec<u8>> {
        data.iter().map(|s| s.to_vec()).collect()
    }

    #[test]
    fn encode_is_systematic() {
        let rs = ReedSolomon::new(3, 2).unwrap();
        let data = shards(&[b"aaaa", b"bbbb", b"cccc"]);
        let all = rs.encode(&data).unwrap();
        assert_eq!(all.len(), 5);
        // first k shards are unchanged
        assert_eq!(&all[0], b"aaaa");
        assert_eq!(&all[1], b"bbbb");
        assert_eq!(&all[2], b"cccc");
    }

    #[test]
    fn recover_from_exactly_k_shards() {
        let rs = ReedSolomon::new(3, 2).unwrap();
        let data = shards(&[b"aaaa", b"bbbb", b"cccc"]);
        let all = rs.encode(&data).unwrap();
        // Drop shards 0 and 2; keep 1, 3, 4 (one data + two parity).
        let present = vec![
            (1usize, all[1].clone()),
            (3usize, all[3].clone()),
            (4usize, all[4].clone()),
        ];
        let recovered = rs.reconstruct(&present).unwrap();
        assert_eq!(recovered, data);
    }

    #[test]
    fn every_way_of_losing_m_shards_recovers() {
        let rs = ReedSolomon::new(4, 3).unwrap(); // n = 7, can lose any 3
        let data = shards(&[b"0000", b"1111", b"2222", b"3333"]);
        let all = rs.encode(&data).unwrap();
        let n = rs.n();
        // choose every combination of k=4 survivors out of 7
        for a in 0..n {
            for b in (a + 1)..n {
                for c in (b + 1)..n {
                    for d in (c + 1)..n {
                        let present: Vec<(usize, Vec<u8>)> =
                            [a, b, c, d].iter().map(|&i| (i, all[i].clone())).collect();
                        let rec = rs.reconstruct(&present).unwrap();
                        assert_eq!(rec, data, "failed for survivors {a},{b},{c},{d}");
                    }
                }
            }
        }
    }

    #[test]
    fn too_few_shards_errors() {
        let rs = ReedSolomon::new(3, 2).unwrap();
        let data = shards(&[b"aaaa", b"bbbb", b"cccc"]);
        let all = rs.encode(&data).unwrap();
        let present = vec![(0usize, all[0].clone()), (1usize, all[1].clone())]; // only 2 < 3
        assert!(matches!(
            rs.reconstruct(&present),
            Err(RsError::NotEnoughShards { have: 2, need: 3 })
        ));
    }

    #[test]
    fn bad_params_rejected() {
        assert!(ReedSolomon::new(0, 2).is_err());
        assert!(ReedSolomon::new(3, 0).is_err());
        assert!(ReedSolomon::new(200, 100).is_err()); // > 255
    }

    #[test]
    fn larger_random_payload() {
        let rs = ReedSolomon::new(6, 4).unwrap();
        // 6 shards of 64 bytes each
        let data: Vec<Vec<u8>> = (0..6)
            .map(|s| (0..64).map(|b| ((s * 31 + b * 7) & 0xff) as u8).collect())
            .collect();
        let all = rs.encode(&data).unwrap();
        // lose 4 arbitrary shards (indices 1,4,7,9)
        let survivors = [0usize, 2, 3, 5, 6, 8];
        let present: Vec<(usize, Vec<u8>)> =
            survivors.iter().map(|&i| (i, all[i].clone())).collect();
        assert_eq!(rs.reconstruct(&present).unwrap(), data);
    }
}
