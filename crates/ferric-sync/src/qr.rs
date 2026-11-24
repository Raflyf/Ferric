use crate::{SyncError, SyncResult};

const GF_PRIM_POLY: u16 = 0x11D;
const ECC_LEVEL_L_BITS: u8 = 0b01;
const MASK_PATTERN_0: u8 = 0;

#[derive(Debug, Clone, Copy)]
struct VersionSpec {
    version: usize,
    size: usize,
    num_blocks: usize,
    data_codewords_per_block: usize,
    ec_codewords_per_block: usize,
}

/// ISO/IEC 18004 Table 7 & 9 parameters for Versions 1..=6 at Error Correction Level L.
const VERSION_SPECS_L: [VersionSpec; 6] = [
    VersionSpec {
        version: 1,
        size: 21,
        num_blocks: 1,
        data_codewords_per_block: 19,
        ec_codewords_per_block: 7,
    },
    VersionSpec {
        version: 2,
        size: 25,
        num_blocks: 1,
        data_codewords_per_block: 34,
        ec_codewords_per_block: 10,
    },
    VersionSpec {
        version: 3,
        size: 29,
        num_blocks: 1,
        data_codewords_per_block: 55,
        ec_codewords_per_block: 15,
    },
    VersionSpec {
        version: 4,
        size: 33,
        num_blocks: 1,
        data_codewords_per_block: 80,
        ec_codewords_per_block: 20,
    },
    VersionSpec {
        version: 5,
        size: 37,
        num_blocks: 1,
        data_codewords_per_block: 108,
        ec_codewords_per_block: 26,
    },
    VersionSpec {
        version: 6,
        size: 41,
        num_blocks: 2,
        data_codewords_per_block: 68,
        ec_codewords_per_block: 18,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrMatrix {
    pub size: usize,
    pub modules: Vec<bool>,
}

fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut acc = 0u8;
    while b != 0 {
        if (b & 1) != 0 {
            acc ^= a;
        }
        let carry = (a & 0x80) != 0;
        a <<= 1;
        if carry {
            a ^= (GF_PRIM_POLY & 0xFF) as u8;
        }
        b >>= 1;
    }
    acc
}

fn rs_generator_poly(degree: usize) -> Vec<u8> {
    let mut poly = Vec::with_capacity(degree + 1);
    poly.push(1u8);
    let mut root = 1u8;
    for _ in 0..degree {
        let mut next = vec![0u8; poly.len() + 1];
        for (idx, &coeff) in poly.iter().enumerate() {
            next[idx] ^= coeff;
            next[idx + 1] ^= gf_mul(coeff, root);
        }
        poly = next;
        root = gf_mul(root, 0x02);
    }
    poly
}

fn rs_compute_remainder(data: &[u8], degree: usize) -> Vec<u8> {
    let poly = rs_generator_poly(degree);
    let mut rem = vec![0u8; degree];
    for &byte in data {
        let factor = byte ^ rem[0];
        rem.rotate_left(1);
        rem[degree - 1] = 0;
        for (slot, &g) in rem.iter_mut().zip(&poly[1..]) {
            *slot ^= gf_mul(g, factor);
        }
    }
    rem
}

/// Compute 15-bit BCH(15, 5) Format Information masked with `0x5412` (ISO/IEC 18004 Annex C).
pub fn compute_bch_format_bits(ecc_bits: u8, mask_pattern: u8) -> u16 {
    let data = (u16::from(ecc_bits & 0x03) << 3) | u16::from(mask_pattern & 0x07);
    let mut rem = data << 10;
    for bit in (10..15).rev() {
        if (rem & (1 << bit)) != 0 {
            rem ^= 0x537 << (bit - 10);
        }
    }
    ((data << 10) | rem) ^ 0x5412
}

fn push_bits(bit_buf: &mut Vec<bool>, val: usize, count: usize) {
    for shift in (0..count).rev() {
        bit_buf.push(((val >> shift) & 1) != 0);
    }
}

fn encode_byte_mode_codewords(payload: &[u8], total_data_codewords: usize) -> Vec<u8> {
    let capacity_bits = total_data_codewords * 8;
    let mut bits = Vec::with_capacity(capacity_bits);

    // Byte mode indicator (0100) + 8-bit character count (Versions 1..=9).
    push_bits(&mut bits, 0b0100, 4);
    push_bits(&mut bits, payload.len(), 8);
    for &b in payload {
        push_bits(&mut bits, usize::from(b), 8);
    }

    let terminator_len = 4usize.min(capacity_bits.saturating_sub(bits.len()));
    bits.resize(bits.len() + terminator_len, false);
    while !bits.len().is_multiple_of(8) {
        bits.push(false);
    }

    let mut codewords = Vec::with_capacity(total_data_codewords);
    for chunk in bits.as_chunks::<8>().0 {
        let mut byte = 0u8;
        for &bit in chunk {
            byte = (byte << 1) | u8::from(bit);
        }
        codewords.push(byte);
    }

    let pad_bytes = [0xEC_u8, 0x11_u8];
    let mut pad_idx = 0usize;
    while codewords.len() < total_data_codewords {
        codewords.push(pad_bytes[pad_idx & 1]);
        pad_idx += 1;
    }

    codewords
}

fn interleave_blocks(data_codewords: &[u8], spec: VersionSpec) -> Vec<u8> {
    let mut data_blocks = Vec::with_capacity(spec.num_blocks);
    let mut ec_blocks = Vec::with_capacity(spec.num_blocks);

    for b in 0..spec.num_blocks {
        let start = b * spec.data_codewords_per_block;
        let end = start + spec.data_codewords_per_block;
        let slice = &data_codewords[start..end];
        data_blocks.push(slice);
        ec_blocks.push(rs_compute_remainder(slice, spec.ec_codewords_per_block));
    }

    let total_cw = spec.num_blocks * (spec.data_codewords_per_block + spec.ec_codewords_per_block);
    let mut out = Vec::with_capacity(total_cw);

    for col in 0..spec.data_codewords_per_block {
        for block in &data_blocks {
            out.push(block[col]);
        }
    }
    for col in 0..spec.ec_codewords_per_block {
        for ec in &ec_blocks {
            out.push(ec[col]);
        }
    }

    out
}

struct GridBuilder {
    size: usize,
    modules: Vec<bool>,
    is_function: Vec<bool>,
}

impl GridBuilder {
    fn new(size: usize) -> Self {
        Self {
            size,
            modules: vec![false; size * size],
            is_function: vec![false; size * size],
        }
    }

    fn set_function(&mut self, x: usize, y: usize, dark: bool) {
        if x < self.size && y < self.size {
            let idx = y * self.size + x;
            self.modules[idx] = dark;
            self.is_function[idx] = true;
        }
    }

    fn place_finder_with_separator(&mut self, origin_x: isize, origin_y: isize) {
        for dy in -1..=7_isize {
            for dx in -1..=7_isize {
                let x = origin_x + dx;
                let y = origin_y + dy;
                if x < 0 || y < 0 || x >= self.size as isize || y >= self.size as isize {
                    continue;
                }
                let in_finder = (0..=6).contains(&dx) && (0..=6).contains(&dy);
                let dark = in_finder
                    && (dx == 0
                        || dx == 6
                        || dy == 0
                        || dy == 6
                        || ((2..=4).contains(&dx) && (2..=4).contains(&dy)));
                self.set_function(x as usize, y as usize, dark);
            }
        }
    }

    fn place_alignment(&mut self, center_x: usize, center_y: usize) {
        for dy in -2..=2_isize {
            for dx in -2..=2_isize {
                let x = (center_x as isize + dx) as usize;
                let y = (center_y as isize + dy) as usize;
                let dark = dx.abs() == 2 || dy.abs() == 2 || (dx == 0 && dy == 0);
                self.set_function(x, y, dark);
            }
        }
    }

    fn reserve_and_draw_format_bits(&mut self, format_bits: u16) {
        for i in 0..15_usize {
            let bit = ((format_bits >> i) & 1) != 0;

            // Copy 1 around top-left finder
            if i < 6 {
                self.set_function(8, i, bit);
            } else if i < 8 {
                self.set_function(8, i + 1, bit);
            } else if i == 8 {
                self.set_function(7, 8, bit);
            } else {
                self.set_function(14 - i, 8, bit);
            }

            // Copy 2 split across top-right and bottom-left finders
            if i < 8 {
                self.set_function(self.size - 1 - i, 8, bit);
            } else {
                self.set_function(8, self.size - 15 + i, bit);
            }
        }
    }
}

impl QrMatrix {
    /// Encode a UTF-8 string into a standard ISO/IEC 18004 QR Code (Versions 1..=6, ECC Level L, Byte Mode).
    pub fn encode(text: &str) -> SyncResult<Self> {
        let payload = text.as_bytes();
        if payload.is_empty() {
            return Err(SyncError::QrPayload(
                "QR payload cannot be empty".to_string(),
            ));
        }

        let spec = VERSION_SPECS_L
            .iter()
            .copied()
            .find(|s| {
                let total_data = s.num_blocks * s.data_codewords_per_block;
                // 4-bit mode + 8-bit char count = 12 header bits => total_data * 8 - 12 bits
                payload.len() * 8 + 12 <= total_data * 8
            })
            .ok_or_else(|| {
                SyncError::QrPayload(format!(
                    "QR payload length ({} bytes) exceeds Version 6-L maximum (134 bytes)",
                    payload.len()
                ))
            })?;

        let total_data_cw = spec.num_blocks * spec.data_codewords_per_block;
        let data_cw = encode_byte_mode_codewords(payload, total_data_cw);
        let all_codewords = interleave_blocks(&data_cw, spec);

        let size = spec.size;
        let mut grid = GridBuilder::new(size);

        // 1. Finder patterns + separators at three corners
        grid.place_finder_with_separator(0, 0);
        grid.place_finder_with_separator((size - 7) as isize, 0);
        grid.place_finder_with_separator(0, (size - 7) as isize);

        // 2. Timing patterns along row 6 and column 6
        for i in 8..(size - 8) {
            let dark = i.is_multiple_of(2);
            grid.set_function(i, 6, dark);
            grid.set_function(6, i, dark);
        }

        // 3. Alignment pattern for Versions 2..=6 at (size - 7, size - 7)
        if spec.version >= 2 {
            grid.place_alignment(size - 7, size - 7);
        }

        // 4. Always-dark module at (8, 4 * V + 9) == (8, size - 8)
        grid.set_function(8, 4 * spec.version + 9, true);

        // 5. Reserve format areas (written cleanly with final BCH format bits)
        let format_bits = compute_bch_format_bits(ECC_LEVEL_L_BITS, MASK_PATTERN_0);
        grid.reserve_and_draw_format_bits(format_bits);

        // 6. Zigzag data + ECC bit placement with Mask Pattern 0: (row + col) % 2 == 0
        let total_codeword_bits = all_codewords.len() * 8;
        let mut bit_idx = 0usize;
        let mut upward = true;
        let mut col = (size - 1) as isize;

        while col > 0 {
            if col == 6 {
                col -= 1;
            }
            for step in 0..size {
                let y = if upward { size - 1 - step } else { step };
                for dx in 0..2_isize {
                    let x = (col - dx) as usize;
                    let idx = y * size + x;
                    if grid.is_function[idx] {
                        continue;
                    }
                    let mut bit = if bit_idx < total_codeword_bits {
                        let cw = all_codewords[bit_idx >> 3];
                        let shift = 7 - (bit_idx & 7);
                        ((cw >> shift) & 1) != 0
                    } else {
                        false
                    };
                    bit_idx += 1;

                    // Mask Pattern 0: invert module when (row + col) % 2 == 0
                    if (y + x).is_multiple_of(2) {
                        bit = !bit;
                    }
                    grid.modules[idx] = bit;
                }
            }
            upward = !upward;
            col -= 2;
        }

        // Re-stamp BCH format bits (already function-protected, ensures pristine integrity)
        grid.reserve_and_draw_format_bits(format_bits);

        Ok(Self {
            size,
            modules: grid.modules,
        })
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> bool {
        if x < self.size && y < self.size {
            self.modules[y * self.size + x]
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_finder_7x7(qr: &QrMatrix, ox: usize, oy: usize) {
        for dy in 0..7 {
            for dx in 0..7 {
                let expected = dx == 0
                    || dx == 6
                    || dy == 0
                    || dy == 6
                    || ((2..=4).contains(&dx) && (2..=4).contains(&dy));
                assert_eq!(
                    qr.get(ox + dx, oy + dy),
                    expected,
                    "finder mismatch at ({}, {})",
                    ox + dx,
                    oy + dy
                );
            }
        }
    }

    #[test]
    fn encodes_ferric_pairing_uri_with_valid_size_and_intact_finders() {
        let uri = "ferric://pair?id=ed25519_abc&tok=0123456789abcdef&name=Desktop-PC";
        let qr = QrMatrix::encode(uri).expect("encode pairing qr");
        assert!((21..=41).contains(&qr.size));
        assert_eq!(qr.modules.len(), qr.size * qr.size);

        assert_finder_7x7(&qr, 0, 0);
        assert_finder_7x7(&qr, qr.size - 7, 0);
        assert_finder_7x7(&qr, 0, qr.size - 7);

        // White separators adjacent to top-left finder
        for i in 0..8 {
            assert!(!qr.get(7, i));
            assert!(!qr.get(i, 7));
        }
        // Dark module at (8, size - 8)
        assert!(qr.get(8, qr.size - 8));
    }

    #[test]
    fn bch_format_and_reed_solomon_syndromes_are_exact() {
        assert_eq!(
            compute_bch_format_bits(ECC_LEVEL_L_BITS, MASK_PATTERN_0),
            0x77C4
        );

        let sample = b"ferric-rs-test";
        let ec = rs_compute_remainder(sample, 10);
        let mut codeword = sample.to_vec();
        codeword.extend_from_slice(&ec);

        // Evaluate C(alpha^i) == 0 for i in 0..10
        let mut alpha_i = 1u8;
        for _ in 0..10 {
            let mut eval = 0u8;
            for &coeff in &codeword {
                eval = gf_mul(eval, alpha_i) ^ coeff;
            }
            assert_eq!(eval, 0);
            alpha_i = gf_mul(alpha_i, 0x02);
        }
    }

    #[test]
    fn supports_version_1_through_6_and_rejects_oversized_input() {
        let v1 = QrMatrix::encode("ferric://v1").expect("v1");
        assert_eq!(v1.size, 21);

        let v6_text = "f".repeat(130);
        let v6 = QrMatrix::encode(&v6_text).expect("v6");
        assert_eq!(v6.size, 41);

        let too_long = "x".repeat(135);
        assert!(QrMatrix::encode(&too_long).is_err());
        assert!(QrMatrix::encode("").is_err());
    }
}
