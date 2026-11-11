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
