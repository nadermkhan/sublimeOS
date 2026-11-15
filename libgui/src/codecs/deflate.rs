use alloc::vec;
use alloc::vec::Vec;

const MAX_INFLATE_BYTES: usize = 32 * 1024 * 1024;

/// Compute IEEE 802.3 CRC-32 checksum.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320u32 & mask);
        }
    }
    !crc
}

/// Compute RFC 1950 Adler-32 checksum.
pub fn adler32(data: &[u8]) -> u32 {
    let mut s1 = 1u32;
    let mut s2 = 0u32;
    for &b in data {
        s1 = (s1 + b as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    (s2 << 16) | s1
}

/// Compress raw payload using RFC 1950 zlib wrapper + RFC 1951 stored DEFLATE blocks.
pub fn zlib_compress_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + (data.len() / 65535 + 1) * 5 + 6);
    out.push(0x78);
    out.push(0x01);

    let mut pos = 0usize;
    loop {
        let rem = data.len().saturating_sub(pos);
        let chunk = rem.min(65535);
        let is_final = pos + chunk >= data.len();
        out.push(if is_final { 1 } else { 0 });
        let len_u16 = chunk as u16;
        let nlen = !len_u16;
        out.extend_from_slice(&len_u16.to_le_bytes());
        out.extend_from_slice(&nlen.to_le_bytes());
        if chunk > 0 {
            out.extend_from_slice(&data[pos..pos + chunk]);
        }
        pos += chunk;
        if is_final {
            break;
        }
    }

    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Inflate an RFC 1950 zlib-wrapped DEFLATE stream.
pub fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    if data.len() < 6 {
        return Err("Zlib stream too short");
    }
    let cmf = data[0];
    let flg = data[1];
    if (cmf & 0x0F) != 8 {
        return Err("Unsupported zlib compression method (expected deflate)");
    }
    if (((cmf as u16) << 8) | (flg as u16)) % 31 != 0 {
        return Err("Invalid zlib header checksum");
    }
    let mut start = 2usize;
    if (flg & 0x20) != 0 {
        if data.len() < 10 {
            return Err("Truncated zlib preset dictionary");
        }
        start = 6;
    }
    deflate_decompress(&data[start..])
}

struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_buf: u32,
    bit_cnt: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_buf: 0,
            bit_cnt: 0,
        }
    }

    fn ensure(&mut self, n: u8) -> Result<(), &'static str> {
        while self.bit_cnt < n {
            if self.byte_pos >= self.data.len() {
                return Err("Unexpected EOF in DEFLATE stream");
            }
            self.bit_buf |= (self.data[self.byte_pos] as u32) << self.bit_cnt;
            self.byte_pos += 1;
            self.bit_cnt += 8;
        }
        Ok(())
    }

    fn read_bits(&mut self, n: u8) -> Result<u32, &'static str> {
        if n == 0 {
            return Ok(0);
        }
        self.ensure(n)?;
        let val = self.bit_buf & ((1u32 << n) - 1);
        self.bit_buf >>= n;
        self.bit_cnt -= n;
        Ok(val)
    }

    fn align_byte(&mut self) {
        let drop_bits = self.bit_cnt & 7;
        self.bit_buf >>= drop_bits;
        self.bit_cnt -= drop_bits;
    }

    fn read_aligned_bytes(&mut self, count: usize) -> Result<&'a [u8], &'static str> {
        self.align_byte();
        let buffered_bytes = (self.bit_cnt / 8) as usize;
        if buffered_bytes > 0 {
            self.byte_pos -= buffered_bytes;
            self.bit_buf = 0;
            self.bit_cnt = 0;
        }
        if self.byte_pos + count > self.data.len() {
            return Err("Truncated uncompressed DEFLATE block");
        }
        let s = &self.data[self.byte_pos..self.byte_pos + count];
        self.byte_pos += count;
        Ok(s)
    }
}

const MAX_BITS: usize = 15;

struct HuffTree {
    counts: [u16; MAX_BITS + 1],
    symbols: Vec<u16>,
}

impl HuffTree {
    fn from_lengths(lengths: &[u8]) -> Result<Self, &'static str> {
        let mut counts = [0u16; MAX_BITS + 1];
        for &len in lengths {
            if (len as usize) > MAX_BITS {
                return Err("Huffman code length exceeds 15 bits");
            }
            if len > 0 {
                counts[len as usize] += 1;
            }
        }

        let mut offsets = [0u16; MAX_BITS + 1];
        let mut total = 0u16;
        for bits in 1..=MAX_BITS {
            offsets[bits] = total;
            total = total.saturating_add(counts[bits]);
        }

        let mut symbols = vec![0u16; total as usize];
        for (sym, &len) in lengths.iter().enumerate() {
            if len > 0 {
                let idx = offsets[len as usize] as usize;
                offsets[len as usize] += 1;
                if idx < symbols.len() {
                    symbols[idx] = sym as u16;
                }
            }
        }

        Ok(Self { counts, symbols })
    }

    fn decode(&self, br: &mut BitReader<'_>) -> Result<u16, &'static str> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..=MAX_BITS {
            code |= br.read_bits(1)? as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                let sym_idx = (index + (code - first)) as usize;
                return self
                    .symbols
                    .get(sym_idx)
                    .copied()
                    .ok_or("Invalid Huffman symbol index");
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("Invalid Huffman code in DEFLATE stream")
    }
}

fn fixed_trees() -> Result<(HuffTree, HuffTree), &'static str> {
    let mut lit_len = [0u8; 288];
    lit_len[..144].fill(8);
    lit_len[144..256].fill(9);
    lit_len[256..280].fill(7);
    lit_len[280..288].fill(8);
    let dist_len = [5u8; 32];
    Ok((HuffTree::from_lengths(&lit_len)?, HuffTree::from_lengths(&dist_len)?))
}

fn dynamic_trees(br: &mut BitReader<'_>) -> Result<(HuffTree, HuffTree), &'static str> {
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let hlit = br.read_bits(5)? as usize + 257;
    let hdist = br.read_bits(5)? as usize + 1;
    let hclen = br.read_bits(4)? as usize + 4;
    if hlit > 286 || hdist > 32 {
        return Err("Invalid dynamic Huffman table header");
    }

    let mut cl_lengths = [0u8; 19];
    for i in 0..hclen {
        cl_lengths[ORDER[i]] = br.read_bits(3)? as u8;
    }
    let cl_tree = HuffTree::from_lengths(&cl_lengths)?;

    let total = hlit + hdist;
    let mut lengths = Vec::with_capacity(total);
    while lengths.len() < total {
        let sym = cl_tree.decode(br)? as usize;
        match sym {
            0..=15 => lengths.push(sym as u8),
            16 => {
                let prev = *lengths.last().ok_or("Invalid RLE repeat code 16")?;
                let rep = br.read_bits(2)? as usize + 3;
                for _ in 0..rep {
                    if lengths.len() >= total {
                        break;
                    }
                    lengths.push(prev);
                }
            }
            17 => {
                let rep = br.read_bits(3)? as usize + 3;
                for _ in 0..rep {
                    if lengths.len() >= total {
                        break;
                    }
                    lengths.push(0);
                }
            }
            18 => {
                let rep = br.read_bits(7)? as usize + 11;
                for _ in 0..rep {
                    if lengths.len() >= total {
                        break;
                    }
                    lengths.push(0);
                }
            }
            _ => return Err("Invalid code-length symbol"),
        }
    }

    let lit_tree = HuffTree::from_lengths(&lengths[..hlit])?;
    let dist_tree = HuffTree::from_lengths(&lengths[hlit..total])?;
    Ok((lit_tree, dist_tree))
}

/// Inflate raw RFC 1951 DEFLATE stream.
pub fn deflate_decompress(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    const LEN_BASE: [u16; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const LEN_EXTRA: [u8; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DIST_BASE: [u16; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u8; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];

    let mut br = BitReader::new(data);
    let mut out = Vec::new();

    loop {
        let bfinal = br.read_bits(1)?;
        let btype = br.read_bits(2)?;

        match btype {
            0 => {
                let hdr = br.read_aligned_bytes(4)?;
                let len = u16::from_le_bytes([hdr[0], hdr[1]]) as usize;
                let nlen = u16::from_le_bytes([hdr[2], hdr[3]]);
                if (len as u16) != !nlen {
                    return Err("Stored DEFLATE block length mismatch");
                }
                if out.len() + len > MAX_INFLATE_BYTES {
                    return Err("DEFLATE output exceeds 32 MiB limit");
                }
                let raw = br.read_aligned_bytes(len)?;
                out.extend_from_slice(raw);
            }
            1 | 2 => {
                let (lit_tree, dist_tree) = if btype == 1 {
                    fixed_trees()?
                } else {
                    dynamic_trees(&mut br)?
                };
                loop {
                    let sym = lit_tree.decode(&mut br)? as usize;
                    if sym < 256 {
                        if out.len() >= MAX_INFLATE_BYTES {
                            return Err("DEFLATE output exceeds 32 MiB limit");
                        }
                        out.push(sym as u8);
                    } else if sym == 256 {
                        break;
                    } else {
                        let lidx = sym - 257;
                        if lidx >= LEN_BASE.len() {
                            return Err("Invalid DEFLATE match length symbol");
                        }
                        let match_len =
                            LEN_BASE[lidx] as usize + br.read_bits(LEN_EXTRA[lidx])? as usize;
                        let dsym = dist_tree.decode(&mut br)? as usize;
                        if dsym >= DIST_BASE.len() {
                            return Err("Invalid DEFLATE distance symbol");
                        }
                        let dist =
                            DIST_BASE[dsym] as usize + br.read_bits(DIST_EXTRA[dsym])? as usize;
                        if dist == 0 || dist > out.len() {
                            return Err("Invalid DEFLATE back-reference distance");
                        }
                        if out.len() + match_len > MAX_INFLATE_BYTES {
                            return Err("DEFLATE output exceeds 32 MiB limit");
                        }
                        for _ in 0..match_len {
                            let b = out[out.len() - dist];
                            out.push(b);
                        }
                    }
                }
            }
            _ => return Err("Invalid DEFLATE block type"),
        }

        if bfinal != 0 {
            break;
        }
    }

    Ok(out)
}
