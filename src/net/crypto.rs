// Cryptographic primitives for IEEE 802.11i WPA2-PSK authentication.
// Pure `#![no_std]` Rust implementation of SHA-1, HMAC-SHA1, and PBKDF2-SHA1.

/// Compute SHA-1 digest (160 bits / 20 bytes) of input data.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let bit_len = (data.len() as u64) * 8;

    // Pad data: append 0x80, then zeroes, then 64-bit big-endian length
    let mut padded = alloc::vec::Vec::from(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

/// Compute HMAC-SHA1 of message with given secret key.
pub fn hmac_sha1(key: &[u8], message: &[u8]) -> [u8; 20] {
    let mut key_pad = [0u8; 64];
    if key.len() > 64 {
        let hashed_key = sha1(key);
        key_pad[..20].copy_from_slice(&hashed_key);
    } else {
        key_pad[..key.len()].copy_from_slice(key);
    }

    let mut o_key_pad = [0u8; 64];
    let mut i_key_pad = [0u8; 64];
    for i in 0..64 {
        o_key_pad[i] = key_pad[i] ^ 0x5C;
        i_key_pad[i] = key_pad[i] ^ 0x36;
    }

    // Inner hash: H(i_key_pad || message)
    let mut inner_input = alloc::vec::Vec::from(i_key_pad);
    inner_input.extend_from_slice(message);
    let inner_hash = sha1(&inner_input);

    // Outer hash: H(o_key_pad || inner_hash)
    let mut outer_input = alloc::vec::Vec::from(o_key_pad);
    outer_input.extend_from_slice(&inner_hash);
    sha1(&outer_input)
}

/// PBKDF2-HMAC-SHA1 key derivation (RFC 2898 / IEEE 802.11i).
/// Used by WPA2-PSK to derive the 256-bit (32-byte) Pairwise Master Key (PMK)
/// from the Wi-Fi passphrase and SSID using 4096 iterations.
pub fn pbkdf2_sha1(passphrase: &[u8], ssid: &[u8], iterations: u32, out: &mut [u8; 32]) {
    // Standard WPA2 uses 2 blocks (each block is 20 bytes from HMAC-SHA1 -> 40 bytes generated, 32 used)
    for block_idx in 1..=2 {
        let mut salt_and_idx = alloc::vec::Vec::from(ssid);
        salt_and_idx.extend_from_slice(&(block_idx as u32).to_be_bytes());

        // U_1 = PRF(P, S || INT_32_BE(i))
        let mut u = hmac_sha1(passphrase, &salt_and_idx);
        let mut t = u;

        // U_2 ... U_c
        for _ in 1..iterations {
            u = hmac_sha1(passphrase, &u);
            for k in 0..20 {
                t[k] ^= u[k];
            }
        }

        let out_offset = (block_idx - 1) as usize * 20;
        let copy_len = if block_idx == 1 { 20 } else { 12 };
        out[out_offset..out_offset + copy_len].copy_from_slice(&t[..copy_len]);
    }
}

// =========================================================================
// SHA-256 & HMAC-SHA256 (FIPS 180-4)
// =========================================================================

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256(input: &[u8]) -> [u8; 32] {
    let mut h = [
        0x6a09e667u32, 0xbb67ae85u32, 0x3c6ef372u32, 0xa54ff53au32,
        0x510e527fu32, 0x9b05688cu32, 0x1f83d9abu32, 0x5be0cd19u32,
    ];

    let bit_len = (input.len() as u64) * 8;
    let mut padded = alloc::vec::Vec::from(input);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut h_var = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h_var.wrapping_add(s1).wrapping_add(ch).wrapping_add(K256[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h_var = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(h_var);
    }

    let mut out = [0u8; 32];
    for (i, val) in h.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&val.to_be_bytes());
    }
    out
}

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k_pad = [0u8; 64];
    if key.len() > 64 {
        let hash = sha256(key);
        k_pad[..32].copy_from_slice(&hash);
    } else {
        k_pad[..key.len()].copy_from_slice(key);
    }

    let mut o_pad = [0u8; 64];
    let mut i_pad = [0u8; 64];
    for i in 0..64 {
        o_pad[i] = k_pad[i] ^ 0x5c;
        i_pad[i] = k_pad[i] ^ 0x36;
    }

    let mut inner = alloc::vec::Vec::from(i_pad);
    inner.extend_from_slice(msg);
    let inner_hash = sha256(&inner);

    let mut outer = alloc::vec::Vec::from(o_pad);
    outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

// =========================================================================
// HKDF (RFC 5869) & TLS 1.3 Key Schedule (RFC 8446)
// =========================================================================

pub fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    let zero_salt = [0u8; 32];
    let actual_salt = if salt.is_empty() { &zero_salt[..] } else { salt };
    hmac_sha256(actual_salt, ikm)
}

pub fn hkdf_expand(prk: &[u8; 32], info: &[u8], length: usize) -> alloc::vec::Vec<u8> {
    let mut okm = alloc::vec::Vec::with_capacity(length);
    let mut prev_t = alloc::vec::Vec::new();
    let mut block_idx: u8 = 1;

    while okm.len() < length {
        let mut msg = alloc::vec::Vec::new();
        msg.extend_from_slice(&prev_t);
        msg.extend_from_slice(info);
        msg.push(block_idx);
        let t = hmac_sha256(prk, &msg);
        let needed = (length - okm.len()).min(32);
        okm.extend_from_slice(&t[..needed]);
        prev_t = alloc::vec::Vec::from(&t[..]);
        block_idx += 1;
    }

    okm
}

pub fn hkdf_expand_label(secret: &[u8; 32], label: &str, context: &[u8], length: u16) -> alloc::vec::Vec<u8> {
    let mut hkdf_label = alloc::vec::Vec::new();
    hkdf_label.extend_from_slice(&length.to_be_bytes());
    let full_label = alloc::format!("tls13 {}", label);
    let l_bytes = full_label.as_bytes();
    hkdf_label.push(l_bytes.len() as u8);
    hkdf_label.extend_from_slice(l_bytes);
    hkdf_label.push(context.len() as u8);
    hkdf_label.extend_from_slice(context);
    hkdf_expand(secret, &hkdf_label, length as usize)
}

// =========================================================================
// X25519 Elliptic-Curve Diffie-Hellman (RFC 7748)
// =========================================================================

#[derive(Clone, Copy)]
struct FieldElem([u64; 5]);

impl FieldElem {
    fn from_bytes(b: &[u8; 32]) -> Self {
        let mut w = [0u64; 4];
        for i in 0..4 {
            let mut chunk = [0u8; 8];
            chunk.copy_from_slice(&b[i * 8..(i + 1) * 8]);
            w[i] = u64::from_le_bytes(chunk);
        }
        let m = (1u64 << 51) - 1;
        Self([
            w[0] & m,
            ((w[0] >> 51) | (w[1] << 13)) & m,
            ((w[1] >> 38) | (w[2] << 26)) & m,
            ((w[2] >> 25) | (w[3] << 39)) & m,
            (w[3] >> 12) & m,
        ])
    }

    fn to_bytes(&self) -> [u8; 32] {
        let mut h = *self;
        let m = (1u64 << 51) - 1;
        let mut c = h.0[0] >> 51; h.0[0] &= m; h.0[1] += c;
        c = h.0[1] >> 51; h.0[1] &= m; h.0[2] += c;
        c = h.0[2] >> 51; h.0[2] &= m; h.0[3] += c;
        c = h.0[3] >> 51; h.0[3] &= m; h.0[4] += c;
        c = h.0[4] >> 51; h.0[4] &= m; h.0[0] += c * 19;
        c = h.0[0] >> 51; h.0[0] &= m; h.0[1] += c;

        let mut t = h;
        t.0[0] += 19;
        c = t.0[0] >> 51; t.0[0] &= m; t.0[1] += c;
        c = t.0[1] >> 51; t.0[1] &= m; t.0[2] += c;
        c = t.0[2] >> 51; t.0[2] &= m; t.0[3] += c;
        c = t.0[3] >> 51; t.0[3] &= m; t.0[4] += c;
        c = t.0[4] >> 51;
        if c != 0 {
            t.0[4] &= m;
            h = t;
        }

        let mut w = [0u64; 4];
        w[0] = h.0[0] | (h.0[1] << 51);
        w[1] = (h.0[1] >> 13) | (h.0[2] << 38);
        w[2] = (h.0[2] >> 26) | (h.0[3] << 25);
        w[3] = (h.0[3] >> 39) | (h.0[4] << 12);

        let mut out = [0u8; 32];
        for i in 0..4 {
            out[i * 8..(i + 1) * 8].copy_from_slice(&w[i].to_le_bytes());
        }
        out
    }

    fn add(&self, rhs: &Self) -> Self {
        let mut d = [
            self.0[0] + rhs.0[0],
            self.0[1] + rhs.0[1],
            self.0[2] + rhs.0[2],
            self.0[3] + rhs.0[3],
            self.0[4] + rhs.0[4],
        ];
        let m = (1u64 << 51) - 1;
        let mut c = d[0] >> 51; d[0] &= m; d[1] += c;
        c = d[1] >> 51; d[1] &= m; d[2] += c;
        c = d[2] >> 51; d[2] &= m; d[3] += c;
        c = d[3] >> 51; d[3] &= m; d[4] += c;
        c = d[4] >> 51; d[4] &= m; d[0] += c * 19;
        c = d[0] >> 51; d[0] &= m; d[1] += c;
        Self(d)
    }

    fn sub(&self, rhs: &Self) -> Self {
        let p0 = (1u64 << 52) - 38;
        let p14 = (1u64 << 52) - 2;
        let mut d = [
            (self.0[0] + p0) - rhs.0[0],
            (self.0[1] + p14) - rhs.0[1],
            (self.0[2] + p14) - rhs.0[2],
            (self.0[3] + p14) - rhs.0[3],
            (self.0[4] + p14) - rhs.0[4],
        ];
        let m = (1u64 << 51) - 1;
        let mut c = d[0] >> 51; d[0] &= m; d[1] += c;
        c = d[1] >> 51; d[1] &= m; d[2] += c;
        c = d[2] >> 51; d[2] &= m; d[3] += c;
        c = d[3] >> 51; d[3] &= m; d[4] += c;
        c = d[4] >> 51; d[4] &= m; d[0] += c * 19;
        c = d[0] >> 51; d[0] &= m; d[1] += c;
        Self(d)
    }

    fn mul(&self, rhs: &Self) -> Self {
        let mut r = [0u128; 5];
        for i in 0..5 {
            for j in 0..5 {
                let prod = (self.0[i] as u128) * (rhs.0[j] as u128);
                let k = i + j;
                if k < 5 {
                    r[k] += prod;
                } else {
                    r[k - 5] += prod * 19;
                }
            }
        }
        let m = (1u128 << 51) - 1;
        let mut c = r[0] >> 51; r[0] &= m; r[1] += c;
        c = r[1] >> 51; r[1] &= m; r[2] += c;
        c = r[2] >> 51; r[2] &= m; r[3] += c;
        c = r[3] >> 51; r[3] &= m; r[4] += c;
        c = r[4] >> 51; r[4] &= m; r[0] += c * 19;
        c = r[0] >> 51; r[0] &= m; r[1] += c;
        Self([r[0] as u64, r[1] as u64, r[2] as u64, r[3] as u64, r[4] as u64])
    }

    fn sqr(&self) -> Self {
        self.mul(self)
    }

    fn mul_scalar(&self, s: u64) -> Self {
        let mut r = [0u128; 5];
        for i in 0..5 {
            r[i] = (self.0[i] as u128) * (s as u128);
        }
        let m = (1u128 << 51) - 1;
        let mut c = r[0] >> 51; r[0] &= m; r[1] += c;
        c = r[1] >> 51; r[1] &= m; r[2] += c;
        c = r[2] >> 51; r[2] &= m; r[3] += c;
        c = r[3] >> 51; r[3] &= m; r[4] += c;
        c = r[4] >> 51; r[4] &= m; r[0] += c * 19;
        c = r[0] >> 51; r[0] &= m; r[1] += c;
        Self([r[0] as u64, r[1] as u64, r[2] as u64, r[3] as u64, r[4] as u64])
    }

    fn invert(&self) -> Self {
        let mut exp = [0xffu8; 32];
        exp[0] = 0xeb;
        exp[31] = 0x7f;

        let mut res = FieldElem([1, 0, 0, 0, 0]);
        for i in (0..255).rev() {
            res = res.sqr();
            let bit = (exp[i / 8] >> (i % 8)) & 1;
            if bit == 1 {
                res = res.mul(self);
            }
        }
        res
    }
}

pub fn x25519(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    let mut k = *scalar;
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;

    let x1 = FieldElem::from_bytes(point);
    let mut x2 = FieldElem([1, 0, 0, 0, 0]);
    let mut z2 = FieldElem([0, 0, 0, 0, 0]);
    let mut x3 = x1;
    let mut z3 = FieldElem([1, 0, 0, 0, 0]);
    let mut swap = 0;

    for i in (0..255).rev() {
        let bit = ((k[i / 8] >> (i % 8)) & 1) as usize;
        let do_swap = swap ^ bit;
        swap = bit;
        if do_swap != 0 {
            core::mem::swap(&mut x2, &mut x3);
            core::mem::swap(&mut z2, &mut z3);
        }

        let a = x2.add(&z2);
        let aa = a.sqr();
        let b = x2.sub(&z2);
        let bb = b.sqr();
        let e = aa.sub(&bb);
        let c = x3.add(&z3);
        let d = x3.sub(&z3);
        let da = d.mul(&a);
        let cb = c.mul(&b);

        x3 = da.add(&cb).sqr();
        z3 = x1.mul(&da.sub(&cb).sqr());
        x2 = aa.mul(&bb);
        z2 = e.mul(&aa.add(&e.mul_scalar(121665)));
    }
    if swap != 0 {
        core::mem::swap(&mut x2, &mut x3);
        core::mem::swap(&mut z2, &mut z3);
    }

    x2.mul(&z2.invert()).to_bytes()
}

pub fn x25519_base(scalar: &[u8; 32]) -> [u8; 32] {
    let mut base_point = [0u8; 32];
    base_point[0] = 9;
    x25519(scalar, &base_point)
}

// =========================================================================
// ChaCha20 & Poly1305 AEAD (RFC 8439)
// =========================================================================

pub fn chacha20_crypt(key: &[u8; 32], nonce: &[u8; 12], counter: u32, data: &mut [u8]) {
    let mut k_words = [0u32; 8];
    for i in 0..8 {
        k_words[i] = u32::from_le_bytes(key[i * 4..(i + 1) * 4].try_into().unwrap());
    }
    let n_words = [
        u32::from_le_bytes(nonce[0..4].try_into().unwrap()),
        u32::from_le_bytes(nonce[4..8].try_into().unwrap()),
        u32::from_le_bytes(nonce[8..12].try_into().unwrap()),
    ];

    let mut curr_counter = counter;
    for chunk in data.chunks_mut(64) {
        let mut st = [
            0x61707865, 0x3320646e, 0x79622d32, 0x6b206574,
            k_words[0], k_words[1], k_words[2], k_words[3],
            k_words[4], k_words[5], k_words[6], k_words[7],
            curr_counter, n_words[0], n_words[1], n_words[2],
        ];

        let orig = st;
        for _ in 0..10 {
            // Column rounds
            qr(&mut st, 0, 4, 8, 12);
            qr(&mut st, 1, 5, 9, 13);
            qr(&mut st, 2, 6, 10, 14);
            qr(&mut st, 3, 7, 11, 15);
            // Diagonal rounds
            qr(&mut st, 0, 5, 10, 15);
            qr(&mut st, 1, 6, 11, 12);
            qr(&mut st, 2, 7, 8, 13);
            qr(&mut st, 3, 4, 9, 14);
        }

        for i in 0..16 {
            st[i] = st[i].wrapping_add(orig[i]);
        }

        let mut block = [0u8; 64];
        for i in 0..16 {
            block[i * 4..(i + 1) * 4].copy_from_slice(&st[i].to_le_bytes());
        }

        for (i, b) in chunk.iter_mut().enumerate() {
            *b ^= block[i];
        }

        curr_counter = curr_counter.wrapping_add(1);
    }
}

#[inline(always)]
fn qr(st: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    st[a] = st[a].wrapping_add(st[b]); st[d] ^= st[a]; st[d] = st[d].rotate_left(16);
    st[c] = st[c].wrapping_add(st[d]); st[b] ^= st[c]; st[b] = st[b].rotate_left(12);
    st[a] = st[a].wrapping_add(st[b]); st[d] ^= st[a]; st[d] = st[d].rotate_left(8);
    st[c] = st[c].wrapping_add(st[d]); st[b] ^= st[c]; st[b] = st[b].rotate_left(7);
}

pub fn poly1305_mac(key: &[u8; 32], data: &[u8]) -> [u8; 16] {
    let r0 = (u32::from_le_bytes(key[0..4].try_into().unwrap())) & 0x03ffffff;
    let r1 = (u32::from_le_bytes(key[3..7].try_into().unwrap()) >> 2) & 0x03ffff03;
    let r2 = (u32::from_le_bytes(key[6..10].try_into().unwrap()) >> 4) & 0x03ffc0ff;
    let r3 = (u32::from_le_bytes(key[9..13].try_into().unwrap()) >> 6) & 0x03f03fff;
    let r4 = (u32::from_le_bytes(key[12..16].try_into().unwrap()) >> 8) & 0x000fffff;

    let s1 = r1 * 5;
    let s2 = r2 * 5;
    let s3 = r3 * 5;
    let s4 = r4 * 5;

    let mut h0: u64 = 0;
    let mut h1: u64 = 0;
    let mut h2: u64 = 0;
    let mut h3: u64 = 0;
    let mut h4: u64 = 0;

    for chunk in data.chunks(16) {
        let mut m = [0u8; 17];
        m[..chunk.len()].copy_from_slice(chunk);
        m[chunk.len()] = 1;

        let w0 = u32::from_le_bytes(m[0..4].try_into().unwrap());
        let w1 = u32::from_le_bytes(m[3..7].try_into().unwrap()) >> 2;
        let w2 = u32::from_le_bytes(m[6..10].try_into().unwrap()) >> 4;
        let w3 = u32::from_le_bytes(m[9..13].try_into().unwrap()) >> 6;
        let w4 = u32::from_le_bytes(m[12..16].try_into().unwrap()) >> 8;
        let w4_extra = (m[16] as u32) << 24;

        h0 += (w0 & 0x03ffffff) as u64;
        h1 += (w1 & 0x03ffffff) as u64;
        h2 += (w2 & 0x03ffffff) as u64;
        h3 += (w3 & 0x03ffffff) as u64;
        h4 += ((w4 | w4_extra) & 0x03ffffff) as u64;

        let d0 = h0 * (r0 as u64) + h1 * (s4 as u64) + h2 * (s3 as u64) + h3 * (s2 as u64) + h4 * (s1 as u64);
        let d1 = h0 * (r1 as u64) + h1 * (r0 as u64) + h2 * (s4 as u64) + h3 * (s3 as u64) + h4 * (s2 as u64);
        let d2 = h0 * (r2 as u64) + h1 * (r1 as u64) + h2 * (r0 as u64) + h3 * (s4 as u64) + h4 * (s3 as u64);
        let d3 = h0 * (r3 as u64) + h1 * (r2 as u64) + h2 * (r1 as u64) + h3 * (r0 as u64) + h4 * (s4 as u64);
        let d4 = h0 * (r4 as u64) + h1 * (r3 as u64) + h2 * (r2 as u64) + h3 * (r1 as u64) + h4 * (r0 as u64);

        let mut c: u64;
        c = d0 >> 26; h0 = d0 & 0x03ffffff;
        let d1 = d1 + c; c = d1 >> 26; h1 = d1 & 0x03ffffff;
        let d2 = d2 + c; c = d2 >> 26; h2 = d2 & 0x03ffffff;
        let d3 = d3 + c; c = d3 >> 26; h3 = d3 & 0x03ffffff;
        let d4 = d4 + c; c = d4 >> 26; h4 = d4 & 0x03ffffff;
        h0 += c * 5;
        c = h0 >> 26; h0 &= 0x03ffffff;
        h1 += c;
    }

    let mut c = h0 >> 26; h0 &= 0x03ffffff;
    h1 += c; c = h1 >> 26; h1 &= 0x03ffffff;
    h2 += c; c = h2 >> 26; h2 &= 0x03ffffff;
    h3 += c; c = h3 >> 26; h3 &= 0x03ffffff;
    h4 += c; c = h4 >> 26; h4 &= 0x03ffffff;
    h0 += c * 5; c = h0 >> 26; h0 &= 0x03ffffff;
    h1 += c;

    let f = (h0 as u128) | ((h1 as u128) << 26) | ((h2 as u128) << 52) | ((h3 as u128) << 78) | ((h4 as u128) << 104);
    let s = u128::from_le_bytes(key[16..32].try_into().unwrap());
    let out = f.wrapping_add(s);
    out.to_le_bytes()
}

pub fn chacha20_poly1305_seal(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &[u8],
) -> (alloc::vec::Vec<u8>, [u8; 16]) {
    // Generate Poly1305 key from counter 0
    let mut poly_key = [0u8; 64];
    chacha20_crypt(key, nonce, 0, &mut poly_key);

    let mut ciphertext = alloc::vec::Vec::from(plaintext);
    chacha20_crypt(key, nonce, 1, &mut ciphertext);

    let mut auth_data = alloc::vec::Vec::new();
    auth_data.extend_from_slice(aad);
    while (auth_data.len() % 16) != 0 { auth_data.push(0); }
    auth_data.extend_from_slice(&ciphertext);
    while (auth_data.len() % 16) != 0 { auth_data.push(0); }
    auth_data.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    auth_data.extend_from_slice(&(ciphertext.len() as u64).to_le_bytes());

    let mut k32 = [0u8; 32];
    k32.copy_from_slice(&poly_key[0..32]);
    let tag = poly1305_mac(&k32, &auth_data);

    (ciphertext, tag)
}

pub fn chacha20_poly1305_open(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8; 16],
) -> Result<alloc::vec::Vec<u8>, &'static str> {
    let mut poly_key = [0u8; 64];
    chacha20_crypt(key, nonce, 0, &mut poly_key);

    let mut auth_data = alloc::vec::Vec::new();
    auth_data.extend_from_slice(aad);
    while (auth_data.len() % 16) != 0 { auth_data.push(0); }
    auth_data.extend_from_slice(ciphertext);
    while (auth_data.len() % 16) != 0 { auth_data.push(0); }
    auth_data.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    auth_data.extend_from_slice(&(ciphertext.len() as u64).to_le_bytes());

    let mut k32 = [0u8; 32];
    k32.copy_from_slice(&poly_key[0..32]);
    let expected_tag = poly1305_mac(&k32, &auth_data);

    if &expected_tag != tag {
        return Err("Poly1305 tag verification failure");
    }

    let mut plaintext = alloc::vec::Vec::from(ciphertext);
    chacha20_crypt(key, nonce, 1, &mut plaintext);
    Ok(plaintext)
}
