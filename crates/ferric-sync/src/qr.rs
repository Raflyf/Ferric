use crate::{SyncError, SyncResult};

const GF_PRIM_POLY: u16 = 0x11D;
const ECC_LEVEL_L_BITS: u8 = 0b01;
const MASK_PATTERN_0: u8 = 0;

#[derive(Debug, Clone, Copy)]
struct VersionSpec {
    version: usize,
    size: usize,
    group1_blocks: usize,
    group1_data_cw: usize,
    group2_blocks: usize,
    group2_data_cw: usize,
    ec_cw_per_block: usize,
    alignment_centers: &'static [usize],
}

impl VersionSpec {
    #[inline]
    const fn total_data_codewords(self) -> usize {
        self.group1_blocks * self.group1_data_cw + self.group2_blocks * self.group2_data_cw
    }

    #[inline]
    const fn total_blocks(self) -> usize {
        self.group1_blocks + self.group2_blocks
    }

    #[inline]
    const fn count_bits(self) -> usize {
        if self.version <= 9 { 8 } else { 16 }
    }
}

/// ISO/IEC 18004 Table 7, 9 & Annex E parameters for Versions 1..=10 at Error Correction Level L.
const VERSION_SPECS_L: [VersionSpec; 10] = [
    VersionSpec {
        version: 1,
        size: 21,
        group1_blocks: 1,
        group1_data_cw: 19,
        group2_blocks: 0,
        group2_data_cw: 0,
        ec_cw_per_block: 7,
        alignment_centers: &[],
    },
    VersionSpec {
        version: 2,
        size: 25,
        group1_blocks: 1,
        group1_data_cw: 34,
        group2_blocks: 0,
        group2_data_cw: 0,
        ec_cw_per_block: 10,
        alignment_centers: &[6, 18],
    },
    VersionSpec {
        version: 3,
        size: 29,
        group1_blocks: 1,
        group1_data_cw: 55,
        group2_blocks: 0,
        group2_data_cw: 0,
        ec_cw_per_block: 15,
        alignment_centers: &[6, 22],
    },
    VersionSpec {
        version: 4,
        size: 33,
        group1_blocks: 1,
        group1_data_cw: 80,
        group2_blocks: 0,
        group2_data_cw: 0,
        ec_cw_per_block: 20,
        alignment_centers: &[6, 26],
    },
    VersionSpec {
        version: 5,
        size: 37,
        group1_blocks: 1,
        group1_data_cw: 108,
        group2_blocks: 0,
        group2_data_cw: 0,
        ec_cw_per_block: 26,
        alignment_centers: &[6, 30],
    },
    VersionSpec {
        version: 6,
        size: 41,
        group1_blocks: 2,
        group1_data_cw: 68,
        group2_blocks: 0,
        group2_data_cw: 0,
        ec_cw_per_block: 18,
        alignment_centers: &[6, 34],
    },
    VersionSpec {
        version: 7,
        size: 45,
        group1_blocks: 2,
        group1_data_cw: 78,
        group2_blocks: 0,
        group2_data_cw: 0,
        ec_cw_per_block: 18,
        alignment_centers: &[6, 22, 38],
    },
    VersionSpec {
        version: 8,
        size: 49,
        group1_blocks: 2,
        group1_data_cw: 97,
        group2_blocks: 0,
        group2_data_cw: 0,
        ec_cw_per_block: 22,
        alignment_centers: &[6, 24, 42],
    },
    VersionSpec {
        version: 9,
        size: 53,
        group1_blocks: 2,
        group1_data_cw: 116,
        group2_blocks: 0,
        group2_data_cw: 0,
        ec_cw_per_block: 24,
        alignment_centers: &[6, 26, 46],
    },
    VersionSpec {
        version: 10,
        size: 57,
        group1_blocks: 2,
        group1_data_cw: 68,
        group2_blocks: 2,
        group2_data_cw: 69,
        ec_cw_per_block: 18,
        alignment_centers: &[6, 28, 50],
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

/// Compute 18-bit BCH(18, 6) Version Information for QR Versions 7..=40 (ISO/IEC 18004 Annex D).
pub fn compute_bch_version_bits(version: usize) -> u32 {
    let data = u32::try_from(version).unwrap_or(0) << 12;
    let mut rem = data;
    for bit in (12..18).rev() {
        if (rem & (1 << bit)) != 0 {
            rem ^= 0x1F25 << (bit - 12);
        }
    }
    data | rem
}

fn push_bits(bit_buf: &mut Vec<bool>, val: usize, count: usize) {
    for shift in (0..count).rev() {
        bit_buf.push(((val >> shift) & 1) != 0);
    }
}

fn encode_byte_mode_codewords(
    payload: &[u8],
    count_bits: usize,
    total_data_codewords: usize,
) -> Vec<u8> {
    let capacity_bits = total_data_codewords * 8;
    let mut bits = Vec::with_capacity(capacity_bits);

    // Byte mode indicator (0100) + 8-bit (V1..=9) or 16-bit (V10+) character count.
    push_bits(&mut bits, 0b0100, 4);
    push_bits(&mut bits, payload.len(), count_bits);
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
    let num_blocks = spec.total_blocks();
    let mut data_blocks = Vec::with_capacity(num_blocks);
    let mut ec_blocks = Vec::with_capacity(num_blocks);

    let mut offset = 0usize;
    for _ in 0..spec.group1_blocks {
        let slice = &data_codewords[offset..offset + spec.group1_data_cw];
        offset += spec.group1_data_cw;
        data_blocks.push(slice);
        ec_blocks.push(rs_compute_remainder(slice, spec.ec_cw_per_block));
    }
    for _ in 0..spec.group2_blocks {
        let slice = &data_codewords[offset..offset + spec.group2_data_cw];
        offset += spec.group2_data_cw;
        data_blocks.push(slice);
        ec_blocks.push(rs_compute_remainder(slice, spec.ec_cw_per_block));
    }

    let max_data_cw = spec.group1_data_cw.max(spec.group2_data_cw);
    let total_cw = spec.total_data_codewords() + num_blocks * spec.ec_cw_per_block;
    let mut out = Vec::with_capacity(total_cw);

    for col in 0..max_data_cw {
        for block in &data_blocks {
            if let Some(&b) = block.get(col) {
                out.push(b);
            }
        }
    }
    for col in 0..spec.ec_cw_per_block {
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

    fn place_version_info(&mut self, version_bits: u32) {
        for i in 0..18_usize {
            let bit = ((version_bits >> i) & 1) != 0;
            let row = i / 3;
            let col = self.size - 11 + (i % 3);
            self.set_function(col, row, bit);
            self.set_function(row, col, bit);
        }
    }
}

impl QrMatrix {
    /// Encode a UTF-8 string into a standard ISO/IEC 18004 QR Code (Versions 1..=10, ECC Level L, Byte Mode).
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
                let total_data = s.total_data_codewords();
                let header_bits = 4 + s.count_bits();
                payload.len().saturating_mul(8).saturating_add(header_bits) <= total_data * 8
            })
            .ok_or_else(|| {
                SyncError::QrPayload(format!(
                    "QR payload length ({} bytes) exceeds Version 10-L maximum (271 bytes)",
                    payload.len()
                ))
            })?;

        let total_data_cw = spec.total_data_codewords();
        let data_cw = encode_byte_mode_codewords(payload, spec.count_bits(), total_data_cw);
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

        // 3. Alignment patterns for Versions 2..=10 (skip coordinates overlapping the 3 corner finders).
        for &ay in spec.alignment_centers {
            for &ax in spec.alignment_centers {
                let in_tl = ax <= 8 && ay <= 8;
                let in_tr = ax >= size - 9 && ay <= 8;
                let in_bl = ax <= 8 && ay >= size - 9;
                if !in_tl && !in_tr && !in_bl {
                    grid.place_alignment(ax, ay);
                }
            }
        }

        // 4. Always-dark module at (8, 4 * V + 9) == (8, size - 8)
        grid.set_function(8, 4 * spec.version + 9, true);

        // 5. Version Information blocks for Versions 7..=10
        if spec.version >= 7 {
            grid.place_version_info(compute_bch_version_bits(spec.version));
        }

        // 6. Reserve format areas (written cleanly with final BCH format bits)
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
            y.checked_mul(self.size)
                .and_then(|row| row.checked_add(x))
                .and_then(|idx| self.modules.get(idx).copied())
                .unwrap_or(false)
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

        // Long pairing URIs (up to 271 bytes) use Versions 7..=10 with intact BCH Version Info
        assert_eq!(compute_bch_version_bits(7), 0x07C94);
        let long_uri = format!(
            "ferric://pair?id=desktop_host_01&tok=0123456789abcdef0123456789abcdef&name={}&addr=[fd00:dead:beef::1]:54321",
            "MyStudioDesktopMachineLongName"
        );
        assert!(long_uri.len() > 134 && long_uri.len() <= 271);
        let qr_v7_plus = QrMatrix::encode(&long_uri).expect("encode v7..v10 qr");
        assert!((45..=57).contains(&qr_v7_plus.size));
        assert_finder_7x7(&qr_v7_plus, 0, 0);
        assert!(qr_v7_plus.get(8, qr_v7_plus.size - 8));

        let v10_uri = "a".repeat(260);
        let qr_v10 = QrMatrix::encode(&v10_uri).expect("encode v10 qr");
        assert_eq!(qr_v10.size, 57);
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
    fn supports_version_1_through_10_and_rejects_oversized_input() {
        let v1 = QrMatrix::encode("ferric://v1").expect("v1");
        assert_eq!(v1.size, 21);

        let v6_text = "f".repeat(130);
        let v6 = QrMatrix::encode(&v6_text).expect("v6");
        assert_eq!(v6.size, 41);

        let v10_text = "f".repeat(271);
        let v10 = QrMatrix::encode(&v10_text).expect("v10");
        assert_eq!(v10.size, 57);

        let too_long = "x".repeat(272);
        assert!(QrMatrix::encode(&too_long).is_err());
        assert!(QrMatrix::encode("").is_err());
    }
}
