// Cryptographic primitives for IEEE 802.11i WPA2-PSK, TLS 1.3, and kernel CSPRNG.
// Pure `#![no_std]` Rust implementation of SHA-1, SHA-256, ChaCha20-Poly1305, X25519, AES-128.

/// Constant-time equality comparison for MACs, tags, and cryptographic digests.
#[inline(never)]
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

static CSPRNG_COUNTER: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

#[inline]
fn try_rdrand_u64() -> Option<u64> {
    let leaf1 = crate::arch::x86_64::registers::cpuid(1, 0);
    if (leaf1.ecx & (1 << 30)) == 0 {
        return None;
    }
    let mut val: u64 = 0;
    let mut ok: u8 = 0;
    for _ in 0..10 {
        unsafe {
            core::arch::asm!(
                "rdrand {val}",
                "setc {ok}",
                val = out(reg) val,
                ok = out(reg_byte) ok,
                options(nomem, nostack)
            );
        }
        if ok != 0 {
            return Some(val);
        }
    }
    None
}

/// Fill `out` with cryptographically mixed pseudo-random bytes seeded from
/// hardware `RDRAND` (when supported), TSC, PIT ticks, and an atomic counter.
pub fn csprng_fill_bytes(out: &mut [u8]) {
    let mut offset = 0usize;
    while offset < out.len() {
        let mut seed_input = [0u8; 32];
        let tsc = crate::arch::x86_64::pit::read_tsc();
        let ticks = crate::arch::x86_64::pit::get_ticks();
        let ctr = CSPRNG_COUNTER.fetch_add(1, core::sync::atomic::Ordering::AcqRel);
        let hw = try_rdrand_u64().unwrap_or(tsc.rotate_left(17) ^ ctr.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        seed_input[0..8].copy_from_slice(&tsc.to_le_bytes());
        seed_input[8..16].copy_from_slice(&ticks.to_le_bytes());
        seed_input[16..24].copy_from_slice(&ctr.to_le_bytes());
        seed_input[24..32].copy_from_slice(&hw.to_le_bytes());
        let key = sha256(&seed_input);
        let nonce_hash = sha256(&key);
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&nonce_hash[..12]);
        let mut block = [0u8; 64];
        let take = (out.len() - offset).min(64);
        chacha20_crypt(&key, &nonce, ctr as u32, &mut block[..take]);
        out[offset..offset + take].copy_from_slice(&block[..take]);
        offset += take;
    }
}

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

    // Constant-time final reduction modulo p = 2^130 - 5: compute g = h + 5 - 2^130
    let mut g0 = h0 + 5;
    c = g0 >> 26; g0 &= 0x03ffffff;
    let mut g1 = h1 + c; c = g1 >> 26; g1 &= 0x03ffffff;
    let mut g2 = h2 + c; c = g2 >> 26; g2 &= 0x03ffffff;
    let mut g3 = h3 + c; c = g3 >> 26; g3 &= 0x03ffffff;
    let mut g4 = (h4 + c).wrapping_sub(1u64 << 26);

    let mask = (g4 >> 63).wrapping_sub(1);
    g0 &= mask;
    g1 &= mask;
    g2 &= mask;
    g3 &= mask;
    g4 &= mask;
    let nmask = !mask;
    h0 = (h0 & nmask) | g0;
    h1 = (h1 & nmask) | g1;
    h2 = (h2 & nmask) | g2;
    h3 = (h3 & nmask) | g3;
    h4 = (h4 & nmask) | g4;

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

    if !ct_eq(&expected_tag, tag) {
        return Err("Poly1305 tag verification failure");
    }

    let mut plaintext = alloc::vec::Vec::from(ciphertext);
    chacha20_crypt(key, nonce, 1, &mut plaintext);
    Ok(plaintext)
}

// ============================================================================
// IEEE 802.11i WPA2-PSK (AES-128, Key Unwrap RFC 3394, PRF-384, AES-CCMP)
// ============================================================================

const AES_SBOX: [u8; 256] = [
    0x63,0x7C,0x77,0x7B,0xF2,0x6B,0x6F,0xC5,0x30,0x01,0x67,0x2B,0xFE,0xD7,0xAB,0x76,
    0xCA,0x82,0xC9,0x7D,0xFA,0x59,0x47,0xF0,0xAD,0xD4,0xA2,0xAF,0x9C,0xA4,0x72,0xC0,
    0xB7,0xFD,0x93,0x26,0x36,0x3F,0xF7,0xCC,0x34,0xA5,0xE5,0xF1,0x71,0xD8,0x31,0x15,
    0x04,0xC7,0x23,0xC3,0x18,0x96,0x05,0x9A,0x07,0x12,0x80,0xE2,0xEB,0x27,0xB2,0x75,
    0x09,0x83,0x2C,0x1A,0x1B,0x6E,0x5A,0xA0,0x52,0x3B,0xD6,0xB3,0x29,0xE3,0x2F,0x84,
    0x53,0xD1,0x00,0xED,0x20,0xFC,0xB1,0x5B,0x6A,0xCB,0xBE,0x39,0x4A,0x4C,0x58,0xCF,
    0xD0,0xEF,0xAA,0xFB,0x43,0x4D,0x33,0x85,0x45,0xF9,0x02,0x7F,0x50,0x3C,0x9F,0xA8,
    0x51,0xA3,0x40,0x8F,0x92,0x9D,0x38,0xF5,0xBC,0xB6,0xDA,0x21,0x10,0xFF,0xF3,0xD2,
    0xCD,0x0C,0x13,0xEC,0x5F,0x97,0x44,0x17,0xC4,0xA7,0x7E,0x3D,0x64,0x5D,0x19,0x73,
    0x60,0x81,0x4F,0xDC,0x22,0x2A,0x90,0x88,0x46,0xEE,0xB8,0x14,0xDE,0x5E,0x0B,0xDB,
    0xE0,0x32,0x3A,0x0A,0x49,0x06,0x24,0x5C,0xC2,0xD3,0xAC,0x62,0x91,0x95,0xE4,0x79,
    0xE7,0xC8,0x37,0x6D,0x8D,0xD5,0x4E,0xA9,0x6C,0x56,0xF4,0xEA,0x65,0x7A,0xAE,0x08,
    0xBA,0x78,0x25,0x2E,0x1C,0xA6,0xB4,0xC6,0xE8,0xDD,0x74,0x1F,0x4B,0xBD,0x8B,0x8A,
    0x70,0x3E,0xB5,0x66,0x48,0x03,0xF6,0x0E,0x61,0x35,0x57,0xB9,0x86,0xC1,0x1D,0x9E,
    0xE1,0xF8,0x98,0x11,0x69,0xD9,0x8E,0x94,0x9B,0x1E,0x87,0xE9,0xCE,0x55,0x28,0xDF,
    0x8C,0xA1,0x89,0x0D,0xBF,0xE6,0x42,0x68,0x41,0x99,0x2D,0x0F,0xB0,0x54,0xBB,0x16,
];

const AES_INV_SBOX: [u8; 256] = [
    0x52,0x09,0x6A,0xD5,0x30,0x36,0xA5,0x38,0xBF,0x40,0xA3,0x9E,0x81,0xF3,0xD7,0xFB,
    0x7C,0xE3,0x39,0x82,0x9B,0x2F,0xFF,0x87,0x34,0x8E,0x43,0x44,0xC4,0xDE,0xE9,0xCB,
    0x54,0x7B,0x94,0x32,0xA6,0xC2,0x23,0x3D,0xEE,0x4C,0x95,0x0B,0x42,0xFA,0xC3,0x4E,
    0x08,0x2E,0xA1,0x66,0x28,0xD9,0x24,0xB2,0x76,0x5B,0xA2,0x49,0x6D,0x8B,0xD1,0x25,
    0x72,0xF8,0xF6,0x64,0x86,0x68,0x98,0x16,0xD4,0xA4,0x5C,0xCC,0x5D,0x65,0xB6,0x92,
    0x6C,0x70,0x48,0x50,0xFD,0xED,0xB9,0xDA,0x5E,0x15,0x46,0x57,0xA7,0x8D,0x9D,0x84,
    0x90,0xD8,0xAB,0x00,0x8C,0xBC,0xD3,0x0A,0xF7,0xE4,0x58,0x05,0xB8,0xB3,0x45,0x06,
    0xD0,0x2C,0x1E,0x8F,0xCA,0x3F,0x0F,0x02,0xC1,0xAF,0xBD,0x03,0x01,0x13,0x8A,0x6B,
    0x3A,0x91,0x11,0x41,0x4F,0x67,0xDC,0xEA,0x97,0xF2,0xCF,0xCE,0xF0,0xB4,0xE6,0x73,
    0x96,0xAC,0x74,0x22,0xE7,0xAD,0x35,0x85,0xE2,0xF9,0x37,0xE8,0x1C,0x75,0xDF,0x6E,
    0x47,0xF1,0x1A,0x71,0x1D,0x29,0xC5,0x89,0x6F,0xB7,0x62,0x0E,0xAA,0x18,0xBE,0x1B,
    0xFC,0x56,0x3E,0x4B,0xC6,0xD2,0x79,0x20,0x9A,0xDB,0xC0,0xFE,0x78,0xCD,0x5A,0xF4,
    0x1F,0xDD,0xA8,0x33,0x88,0x07,0xC7,0x31,0xB1,0x12,0x10,0x59,0x27,0x80,0xEC,0x5F,
    0x60,0x51,0x7F,0xA9,0x19,0xB5,0x4A,0x0D,0x2D,0xE5,0x7A,0x9F,0x93,0xC9,0x9C,0xEF,
    0xA0,0xE0,0x3B,0x4D,0xAE,0x2A,0xF5,0xB0,0xC8,0xEB,0xBB,0x3C,0x83,0x53,0x99,0x61,
    0x17,0x2B,0x04,0x7E,0xBA,0x77,0xD6,0x26,0xE1,0x69,0x14,0x63,0x55,0x21,0x0C,0x7D,
];

#[inline]
fn xtime(x: u8) -> u8 {
    (x << 1) ^ (((x >> 7) & 1) * 0x1B)
}

#[inline]
fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut r = 0u8;
    for _ in 0..8 {
        if (b & 1) != 0 {
            r ^= a;
        }
        a = xtime(a);
        b >>= 1;
    }
    r
}

fn aes128_key_schedule(key: &[u8; 16]) -> [[u8; 16]; 11] {
    const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1B, 0x36];
    let mut rk = [[0u8; 16]; 11];
    rk[0] = *key;
    for r in 1..=10 {
        let prev = rk[r - 1];
        let t = [
            AES_SBOX[prev[13] as usize] ^ RCON[r - 1],
            AES_SBOX[prev[14] as usize],
            AES_SBOX[prev[15] as usize],
            AES_SBOX[prev[12] as usize],
        ];
        for i in 0..4 {
            rk[r][i] = prev[i] ^ t[i];
        }
        for i in 4..16 {
            rk[r][i] = rk[r][i - 4] ^ prev[i];
        }
    }
    rk
}

/// FIPS-197 AES-128 single 16-byte block encryption.
pub fn aes128_encrypt_block(key: &[u8; 16], input: &[u8; 16]) -> [u8; 16] {
    let rk = aes128_key_schedule(key);
    let mut s = *input;
    for i in 0..16 {
        s[i] ^= rk[0][i];
    }
    for r in 1..=10 {
        for b in &mut s {
            *b = AES_SBOX[*b as usize];
        }
        // ShiftRows (column-major state matrix)
        let t1 = s[1]; s[1] = s[5]; s[5] = s[9]; s[9] = s[13]; s[13] = t1;
        let t2 = s[2]; let t6 = s[6]; s[2] = s[10]; s[6] = s[14]; s[10] = t2; s[14] = t6;
        let t15 = s[15]; s[15] = s[11]; s[11] = s[7]; s[7] = s[3]; s[3] = t15;
        if r < 10 {
            for c in 0..4 {
                let i = c * 4;
                let (a0, a1, a2, a3) = (s[i], s[i + 1], s[i + 2], s[i + 3]);
                let x = a0 ^ a1 ^ a2 ^ a3;
                s[i] ^= x ^ xtime(a0 ^ a1);
                s[i + 1] ^= x ^ xtime(a1 ^ a2);
                s[i + 2] ^= x ^ xtime(a2 ^ a3);
                s[i + 3] ^= x ^ xtime(a3 ^ a0);
            }
        }
        for i in 0..16 {
            s[i] ^= rk[r][i];
        }
    }
    s
}

/// FIPS-197 AES-128 single 16-byte block decryption.
pub fn aes128_decrypt_block(key: &[u8; 16], input: &[u8; 16]) -> [u8; 16] {
    let rk = aes128_key_schedule(key);
    let mut s = *input;
    for i in 0..16 {
        s[i] ^= rk[10][i];
    }
    for r in (0..10).rev() {
        // InvShiftRows
        let t13 = s[13]; s[13] = s[9]; s[9] = s[5]; s[5] = s[1]; s[1] = t13;
        let t2 = s[2]; let t6 = s[6]; s[2] = s[10]; s[6] = s[14]; s[10] = t2; s[14] = t6;
        let t3 = s[3]; s[3] = s[7]; s[7] = s[11]; s[11] = s[15]; s[15] = t3;
        for b in &mut s {
            *b = AES_INV_SBOX[*b as usize];
        }
        for i in 0..16 {
            s[i] ^= rk[r][i];
        }
        if r > 0 {
            for c in 0..4 {
                let i = c * 4;
                let (a0, a1, a2, a3) = (s[i], s[i + 1], s[i + 2], s[i + 3]);
                s[i] = gf_mul(a0, 0x0E) ^ gf_mul(a1, 0x0B) ^ gf_mul(a2, 0x0D) ^ gf_mul(a3, 0x09);
                s[i + 1] = gf_mul(a0, 0x09) ^ gf_mul(a1, 0x0E) ^ gf_mul(a2, 0x0B) ^ gf_mul(a3, 0x0D);
                s[i + 2] = gf_mul(a0, 0x0D) ^ gf_mul(a1, 0x09) ^ gf_mul(a2, 0x0E) ^ gf_mul(a3, 0x0B);
                s[i + 3] = gf_mul(a0, 0x0B) ^ gf_mul(a1, 0x0D) ^ gf_mul(a2, 0x09) ^ gf_mul(a3, 0x0E);
            }
        }
    }
    s
}

/// RFC 3394 AES-128 Key Unwrap (used for extracting GTK from EAPOL-Key Msg 3).
pub fn aes128_key_unwrap(kek: &[u8; 16], wrapped: &[u8]) -> Result<alloc::vec::Vec<u8>, &'static str> {
    if wrapped.len() < 24 || (wrapped.len() % 8) != 0 {
        return Err("Invalid RFC 3394 wrapped key length");
    }
    let n = (wrapped.len() / 8) - 1;
    let mut a = [0u8; 8];
    a.copy_from_slice(&wrapped[..8]);
    let mut r = alloc::vec::Vec::with_capacity(n);
    for i in 0..n {
        let mut blk = [0u8; 8];
        blk.copy_from_slice(&wrapped[(i + 1) * 8..(i + 2) * 8]);
        r.push(blk);
    }

    for j in (0..=5usize).rev() {
        for i in (1..=n).rev() {
            let t = (n * j + i) as u64;
            let a_val = u64::from_be_bytes(a) ^ t;
            let mut input = [0u8; 16];
            input[..8].copy_from_slice(&a_val.to_be_bytes());
            input[8..16].copy_from_slice(&r[i - 1]);
            let b = aes128_decrypt_block(kek, &input);
            a.copy_from_slice(&b[..8]);
            r[i - 1].copy_from_slice(&b[8..16]);
        }
    }

    if !ct_eq(&a, &[0xA6; 8]) {
        return Err("RFC 3394 AES Key Unwrap integrity check failed");
    }
    let mut out = alloc::vec::Vec::with_capacity(n * 8);
    for blk in r {
        out.extend_from_slice(&blk);
    }
    Ok(out)
}

/// IEEE 802.11i PRF-384 (produces 48-byte PTK = KCK[16] || KEK[16] || TK[16]).
pub fn prf_384(key: &[u8], label: &[u8], data: &[u8]) -> [u8; 48] {
    let mut out = [0u8; 48];
    for counter in 0u8..3 {
        let mut msg = alloc::vec::Vec::with_capacity(label.len() + 1 + data.len() + 1);
        msg.extend_from_slice(label);
        msg.push(0x00);
        msg.extend_from_slice(data);
        msg.push(counter);
        let digest = hmac_sha1(key, &msg);
        let start = (counter as usize) * 20;
        let copy_len = 20.min(48 - start);
        out[start..start + copy_len].copy_from_slice(&digest[..copy_len]);
    }
    out
}

/// Derive 48-byte WPA2-PSK Pairwise Transient Key (PTK) from PMK, MACs, ANonce, SNonce.
pub fn derive_wpa2_ptk(
    pmk: &[u8; 32],
    aa: &[u8; 6],
    spa: &[u8; 6],
    anonce: &[u8; 32],
    snonce: &[u8; 32],
) -> [u8; 48] {
    let mut data = [0u8; 76];
    if aa < spa {
        data[0..6].copy_from_slice(aa);
        data[6..12].copy_from_slice(spa);
    } else {
        data[0..6].copy_from_slice(spa);
        data[6..12].copy_from_slice(aa);
    }
    if anonce < snonce {
        data[12..44].copy_from_slice(anonce);
        data[44..76].copy_from_slice(snonce);
    } else {
        data[12..44].copy_from_slice(snonce);
        data[44..76].copy_from_slice(anonce);
    }
    prf_384(pmk, b"Pairwise key expansion", &data)
}

/// Compute 16-byte EAPOL-Key MIC using HMAC-SHA1-128 (WPA2 Key Descriptor Version 2).
pub fn eapol_mic_hmac_sha1(kck: &[u8; 16], eapol_frame: &[u8]) -> [u8; 16] {
    let digest = hmac_sha1(kck, eapol_frame);
    let mut mic = [0u8; 16];
    mic.copy_from_slice(&digest[..16]);
    mic
}

static SNONCE_COUNTER: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

/// Generate a 32-byte SNonce without RDRAND (safe on Intel Sandy Bridge MacBookPro8,2).
pub fn generate_snonce(sta_mac: &[u8; 6], bssid: &[u8; 6]) -> [u8; 32] {
    let tsc = crate::arch::x86_64::pit::read_tsc();
    let ticks = crate::arch::x86_64::pit::get_ticks();
    let seq = SNONCE_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mut seed = alloc::vec::Vec::with_capacity(48);
    seed.extend_from_slice(&tsc.to_le_bytes());
    seed.extend_from_slice(&ticks.to_le_bytes());
    seed.extend_from_slice(&seq.to_le_bytes());
    seed.extend_from_slice(sta_mac);
    seed.extend_from_slice(bssid);
    sha256(&seed)
}

fn ccmp_build_aad_and_nonce(hdr: &[u8], pn: u64) -> Result<(alloc::vec::Vec<u8>, [u8; 13]), &'static str> {
    if hdr.len() < 24 {
        return Err("802.11 header < 24 bytes for AES-CCMP");
    }
    let has_qos = (hdr[0] & 0x0C) == 0x08 && (hdr[0] & 0x80) != 0;
    let hdr_len = if has_qos { 26 } else { 24 };
    if hdr.len() < hdr_len {
        return Err("802.11 QoS header truncated for AES-CCMP");
    }

    // IEEE 802.11i AAD: masked FC (2) + A1 (6) + A2 (6) + A3 (6) + masked SC (2) [+ masked QC (2)]
    let mut aad = alloc::vec::Vec::with_capacity(if has_qos { 24 } else { 22 });
    aad.push(hdr[0] & 0x8F);
    aad.push(hdr[1] & 0xC7);
    aad.extend_from_slice(&hdr[4..22]); // A1, A2, A3
    aad.push(hdr[22] & 0x0F);
    aad.push(0x00);
    if has_qos {
        aad.push(hdr[24] & 0x0F);
        aad.push(0x00);
    }

    let priority = if has_qos { hdr[24] & 0x0F } else { 0u8 };
    let mut nonce = [0u8; 13];
    nonce[0] = priority;
    nonce[1..7].copy_from_slice(&hdr[10..16]); // A2 = Source/Transmitter Address
    nonce[7] = ((pn >> 40) & 0xFF) as u8;
    nonce[8] = ((pn >> 32) & 0xFF) as u8;
    nonce[9] = ((pn >> 24) & 0xFF) as u8;
    nonce[10] = ((pn >> 16) & 0xFF) as u8;
    nonce[11] = ((pn >> 8) & 0xFF) as u8;
    nonce[12] = (pn & 0xFF) as u8;
    Ok((aad, nonce))
}

fn ccmp_cbc_mac(tk: &[u8; 16], nonce: &[u8; 13], aad: &[u8], data: &[u8]) -> [u8; 8] {
    // B_0: Flags (0x59 => Adata=1, M'=3 -> M=8, L'=1 -> L=2), Nonce (13B), l(m) (2B BE)
    let mut b0 = [0u8; 16];
    b0[0] = 0x59;
    b0[1..14].copy_from_slice(nonce);
    b0[14] = ((data.len() >> 8) & 0xFF) as u8;
    b0[15] = (data.len() & 0xFF) as u8;

    let mut x = aes128_encrypt_block(tk, &b0);

    // Encode AAD length (2B BE) + AAD bytes + zero padding to 16B boundary
    let mut aad_blocks = alloc::vec::Vec::with_capacity(2 + aad.len() + 16);
    aad_blocks.push(((aad.len() >> 8) & 0xFF) as u8);
    aad_blocks.push((aad.len() & 0xFF) as u8);
    aad_blocks.extend_from_slice(aad);
    while (aad_blocks.len() % 16) != 0 {
        aad_blocks.push(0);
    }
    for blk in aad_blocks.chunks_exact(16) {
        for i in 0..16 {
            x[i] ^= blk[i];
        }
        x = aes128_encrypt_block(tk, &x);
    }

    for chunk in data.chunks(16) {
        for i in 0..chunk.len() {
            x[i] ^= chunk[i];
        }
        x = aes128_encrypt_block(tk, &x);
    }

    let mut mic = [0u8; 8];
    mic.copy_from_slice(&x[..8]);
    mic
}

fn ccmp_ctr_xor(tk: &[u8; 16], nonce: &[u8; 13], counter_start: u16, data: &mut [u8]) -> [u8; 16] {
    let mut ctr = [0u8; 16];
    ctr[0] = 0x01; // L' = 1 (L = 2)
    ctr[1..14].copy_from_slice(nonce);
    ctr[14] = 0;
    ctr[15] = 0;
    let s0 = aes128_encrypt_block(tk, &ctr);

    let mut idx = 0usize;
    let mut c = counter_start;
    while idx < data.len() {
        ctr[14] = (c >> 8) as u8;
        ctr[15] = (c & 0xFF) as u8;
        let keystream = aes128_encrypt_block(tk, &ctr);
        let take = (data.len() - idx).min(16);
        for i in 0..take {
            data[idx + i] ^= keystream[i];
        }
        idx += take;
        c = c.wrapping_add(1);
    }
    s0
}

/// Encrypt an IEEE 802.11 plaintext payload (`LLC/SNAP + data`) with AES-CCMP.
/// Returns `8-byte CCMP Header || encrypted payload || 8-byte MIC`.
pub fn ccmp_encrypt_mpdu(
    tk: &[u8; 16],
    hdr_24_or_26: &[u8],
    key_id: u8,
    pn: u64,
    plaintext: &[u8],
) -> Result<alloc::vec::Vec<u8>, &'static str> {
    let (aad, nonce) = ccmp_build_aad_and_nonce(hdr_24_or_26, pn)?;
    let raw_mic = ccmp_cbc_mac(tk, &nonce, &aad, plaintext);
    let mut ciphertext = alloc::vec::Vec::from(plaintext);
    let s0 = ccmp_ctr_xor(tk, &nonce, 1, &mut ciphertext);

    let mut out = alloc::vec::Vec::with_capacity(8 + ciphertext.len() + 8);
    out.push((pn & 0xFF) as u8);
    out.push(((pn >> 8) & 0xFF) as u8);
    out.push(0x00);
    out.push(((key_id & 0x03) << 6) | 0x20); // ExtIV = 1
    out.push(((pn >> 16) & 0xFF) as u8);
    out.push(((pn >> 24) & 0xFF) as u8);
    out.push(((pn >> 32) & 0xFF) as u8);
    out.push(((pn >> 40) & 0xFF) as u8);
    out.extend_from_slice(&ciphertext);
    for i in 0..8 {
        out.push(raw_mic[i] ^ s0[i]);
    }
    Ok(out)
}

/// Decrypt an IEEE 802.11 Protected MPDU (`hdr_and_body` starting at 802.11 MAC header).
/// Returns `(plaintext, pn, key_id)` after verifying the 8-byte CCMP MIC.
pub fn ccmp_decrypt_mpdu(
    key: &[u8; 16],
    hdr_and_body: &[u8],
) -> Result<(alloc::vec::Vec<u8>, u64, u8), &'static str> {
    if hdr_and_body.len() < 24 {
        return Err("CCMP frame too short");
    }
    let has_qos = (hdr_and_body[0] & 0x0C) == 0x08 && (hdr_and_body[0] & 0x80) != 0;
    let hdr_len = if has_qos { 26 } else { 24 };
    if hdr_and_body.len() < hdr_len + 16 {
        return Err("CCMP frame shorter than header + CCMP IV + MIC");
    }
    let hdr = &hdr_and_body[..hdr_len];
    let ccmp_hdr = &hdr_and_body[hdr_len..hdr_len + 8];
    if (ccmp_hdr[3] & 0x20) == 0 {
        return Err("CCMP ExtIV bit missing");
    }
    let key_id = (ccmp_hdr[3] >> 6) & 0x03;
    let pn = (ccmp_hdr[0] as u64)
        | ((ccmp_hdr[1] as u64) << 8)
        | ((ccmp_hdr[4] as u64) << 16)
        | ((ccmp_hdr[5] as u64) << 24)
        | ((ccmp_hdr[6] as u64) << 32)
        | ((ccmp_hdr[7] as u64) << 40);

    let enc_end = hdr_and_body.len() - 8;
    let mut plaintext = alloc::vec::Vec::from(&hdr_and_body[hdr_len + 8..enc_end]);
    let enc_mic = &hdr_and_body[enc_end..];

    let (aad, nonce) = ccmp_build_aad_and_nonce(hdr, pn)?;
    let s0 = ccmp_ctr_xor(key, &nonce, 1, &mut plaintext);
    let calc_mic = ccmp_cbc_mac(key, &nonce, &aad, &plaintext);
    let mut expected_mic = [0u8; 8];
    for i in 0..8 {
        expected_mic[i] = calc_mic[i] ^ s0[i];
    }
    if !ct_eq(&expected_mic, enc_mic) {
        return Err("AES-CCMP MIC verification failure");
    }
    Ok((plaintext, pn, key_id))
}
