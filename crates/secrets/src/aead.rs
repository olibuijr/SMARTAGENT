//! ChaCha20-Poly1305 AEAD (RFC 8439), pure `std`, zero deps. Replaces the old
//! XOR obfuscation with authenticated encryption: tampering is detected (the
//! Poly1305 tag fails) and the keystream is cryptographic. Verified against the
//! RFC 8439 test vectors in the tests below.

// ─────────────────────────── ChaCha20 (§2.3) ───────────────────────────

fn quarter_round(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]); s[d] ^= s[a]; s[d] = s[d].rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]); s[b] ^= s[c]; s[b] = s[b].rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]); s[d] ^= s[a]; s[d] = s[d].rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]); s[b] ^= s[c]; s[b] = s[b].rotate_left(7);
}

fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut st = [0u32; 16];
    st[0] = 0x6170_7865; st[1] = 0x3320_646e; st[2] = 0x7962_2d32; st[3] = 0x6b20_6574;
    for i in 0..8 {
        st[4 + i] = u32::from_le_bytes([key[4 * i], key[4 * i + 1], key[4 * i + 2], key[4 * i + 3]]);
    }
    st[12] = counter;
    for i in 0..3 {
        st[13 + i] = u32::from_le_bytes([nonce[4 * i], nonce[4 * i + 1], nonce[4 * i + 2], nonce[4 * i + 3]]);
    }
    let mut w = st;
    for _ in 0..10 {
        quarter_round(&mut w, 0, 4, 8, 12);
        quarter_round(&mut w, 1, 5, 9, 13);
        quarter_round(&mut w, 2, 6, 10, 14);
        quarter_round(&mut w, 3, 7, 11, 15);
        quarter_round(&mut w, 0, 5, 10, 15);
        quarter_round(&mut w, 1, 6, 11, 12);
        quarter_round(&mut w, 2, 7, 8, 13);
        quarter_round(&mut w, 3, 4, 9, 14);
    }
    let mut out = [0u8; 64];
    for i in 0..16 {
        let v = w[i].wrapping_add(st[i]);
        out[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

/// XOR `data` with the ChaCha20 keystream starting at `counter` (encrypt = decrypt).
fn chacha20_xor(key: &[u8; 32], counter: u32, nonce: &[u8; 12], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for (i, chunk) in data.chunks(64).enumerate() {
        let ks = chacha20_block(key, counter + i as u32, nonce);
        for (j, b) in chunk.iter().enumerate() {
            out.push(b ^ ks[j]);
        }
    }
    out
}

// ─────────────────────────── Poly1305 (§2.5) ───────────────────────────
// 5×26-bit limb arithmetic mod 2^130-5 (poly1305-donna style).

fn poly1305(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
    let rd = |i: usize| u32::from_le_bytes([key[i], key[i + 1], key[i + 2], key[i + 3]]);
    // Clamp r.
    let r0 = rd(0) & 0x3ff_ffff;
    let r1 = (rd(3) >> 2) & 0x3ff_ff03;
    let r2 = (rd(6) >> 4) & 0x3ff_c0ff;
    let r3 = (rd(9) >> 6) & 0x3f0_3fff;
    let r4 = (rd(12) >> 8) & 0x00f_ffff;
    let s1 = r1 * 5; let s2 = r2 * 5; let s3 = r3 * 5; let s4 = r4 * 5;

    let (mut h0, mut h1, mut h2, mut h3, mut h4) = (0u32, 0u32, 0u32, 0u32, 0u32);

    let mut i = 0;
    while i < msg.len() {
        let n = (msg.len() - i).min(16);
        let mut block = [0u8; 17];
        block[..n].copy_from_slice(&msg[i..i + n]);
        block[n] = 1; // high bit / final-block marker
        let b = |k: usize| u32::from_le_bytes([block[k], block[k + 1], block[k + 2], block[k + 3]]);
        h0 += b(0) & 0x3ff_ffff;
        h1 += (b(3) >> 2) & 0x3ff_ffff;
        h2 += (b(6) >> 4) & 0x3ff_ffff;
        h3 += (b(9) >> 6) & 0x3ff_ffff;
        h4 += (u32::from_le_bytes([block[12], block[13], block[14], block[15]]) >> 8) | ((block[16] as u32) << 24);

        // h *= r  (mod 2^130-5)
        let d0 = h0 as u64 * r0 as u64 + h1 as u64 * s4 as u64 + h2 as u64 * s3 as u64 + h3 as u64 * s2 as u64 + h4 as u64 * s1 as u64;
        let d1 = h0 as u64 * r1 as u64 + h1 as u64 * r0 as u64 + h2 as u64 * s4 as u64 + h3 as u64 * s3 as u64 + h4 as u64 * s2 as u64;
        let d2 = h0 as u64 * r2 as u64 + h1 as u64 * r1 as u64 + h2 as u64 * r0 as u64 + h3 as u64 * s4 as u64 + h4 as u64 * s3 as u64;
        let d3 = h0 as u64 * r3 as u64 + h1 as u64 * r2 as u64 + h2 as u64 * r1 as u64 + h3 as u64 * r0 as u64 + h4 as u64 * s4 as u64;
        let d4 = h0 as u64 * r4 as u64 + h1 as u64 * r3 as u64 + h2 as u64 * r2 as u64 + h3 as u64 * r1 as u64 + h4 as u64 * r0 as u64;

        let mut c;
        c = d0 >> 26; h0 = d0 as u32 & 0x3ff_ffff;
        let d1 = d1 + c; c = d1 >> 26; h1 = d1 as u32 & 0x3ff_ffff;
        let d2 = d2 + c; c = d2 >> 26; h2 = d2 as u32 & 0x3ff_ffff;
        let d3 = d3 + c; c = d3 >> 26; h3 = d3 as u32 & 0x3ff_ffff;
        let d4 = d4 + c; c = d4 >> 26; h4 = d4 as u32 & 0x3ff_ffff;
        h0 += (c as u32) * 5; c = (h0 >> 26) as u64; h0 &= 0x3ff_ffff; h1 += c as u32;

        i += n;
    }

    // Fully carry/reduce h.
    let mut c;
    c = h1 >> 26; h1 &= 0x3ff_ffff; h2 += c;
    c = h2 >> 26; h2 &= 0x3ff_ffff; h3 += c;
    c = h3 >> 26; h3 &= 0x3ff_ffff; h4 += c;
    c = h4 >> 26; h4 &= 0x3ff_ffff; h0 += c * 5;
    c = h0 >> 26; h0 &= 0x3ff_ffff; h1 += c;

    // Compute h - p to see if h >= p.
    let mut g0 = h0.wrapping_add(5); c = g0 >> 26; g0 &= 0x3ff_ffff;
    let mut g1 = h1.wrapping_add(c); c = g1 >> 26; g1 &= 0x3ff_ffff;
    let mut g2 = h2.wrapping_add(c); c = g2 >> 26; g2 &= 0x3ff_ffff;
    let mut g3 = h3.wrapping_add(c); c = g3 >> 26; g3 &= 0x3ff_ffff;
    let g4 = h4.wrapping_add(c).wrapping_sub(1 << 26);

    // If g4 didn't borrow (bit 31 clear), use g (h >= p); else keep h.
    let mask = (g4 >> 31).wrapping_sub(1); // all-ones if g4 top bit clear
    let nmask = !mask;
    h0 = (h0 & nmask) | (g0 & mask);
    h1 = (h1 & nmask) | (g1 & mask);
    h2 = (h2 & nmask) | (g2 & mask);
    h3 = (h3 & nmask) | (g3 & mask);
    h4 = (h4 & nmask) | (g4 & mask);

    // Serialize h (mod 2^128) + s.
    let mut f = (h0 | (h1 << 26)) as u64 + u32::from_le_bytes([key[16], key[17], key[18], key[19]]) as u64;
    let mut tag = [0u8; 16];
    tag[0..4].copy_from_slice(&(f as u32).to_le_bytes());
    f = ((h1 >> 6) | (h2 << 20)) as u64 + u32::from_le_bytes([key[20], key[21], key[22], key[23]]) as u64 + (f >> 32);
    tag[4..8].copy_from_slice(&(f as u32).to_le_bytes());
    f = ((h2 >> 12) | (h3 << 14)) as u64 + u32::from_le_bytes([key[24], key[25], key[26], key[27]]) as u64 + (f >> 32);
    tag[8..12].copy_from_slice(&(f as u32).to_le_bytes());
    f = ((h3 >> 18) | (h4 << 8)) as u64 + u32::from_le_bytes([key[28], key[29], key[30], key[31]]) as u64 + (f >> 32);
    tag[12..16].copy_from_slice(&(f as u32).to_le_bytes());
    tag
}

// ─────────────────────────── AEAD (§2.8) ───────────────────────────

fn pad16(v: &mut Vec<u8>) {
    let rem = v.len() % 16;
    if rem != 0 {
        v.extend(std::iter::repeat(0u8).take(16 - rem));
    }
}

fn tag(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], ct: &[u8]) -> [u8; 16] {
    let otk = chacha20_block(key, 0, nonce);
    let mut poly_key = [0u8; 32];
    poly_key.copy_from_slice(&otk[..32]);
    let mut mac = Vec::with_capacity(aad.len() + ct.len() + 32);
    mac.extend_from_slice(aad); pad16(&mut mac);
    mac.extend_from_slice(ct); pad16(&mut mac);
    mac.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    mac.extend_from_slice(&(ct.len() as u64).to_le_bytes());
    poly1305(&poly_key, &mac)
}

/// Encrypt: returns nonce(12) ++ ciphertext ++ tag(16).
pub fn seal(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let ct = chacha20_xor(key, 1, nonce, plaintext);
    let t = tag(key, nonce, aad, &ct);
    let mut out = Vec::with_capacity(12 + ct.len() + 16);
    out.extend_from_slice(nonce);
    out.extend_from_slice(&ct);
    out.extend_from_slice(&t);
    out
}

/// Decrypt a `seal` output; None if the tag fails (tampered/wrong key).
pub fn open(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < 28 {
        return None;
    }
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&sealed[..12]);
    let ct = &sealed[12..sealed.len() - 16];
    let got = &sealed[sealed.len() - 16..];
    let want = tag(key, &nonce, aad, ct);
    // Constant-time compare.
    let mut diff = 0u8;
    for (a, b) in got.iter().zip(want.iter()) {
        diff |= a ^ b;
    }
    if diff != 0 {
        return None;
    }
    Some(chacha20_xor(key, 1, &nonce, ct))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn chacha20_block_rfc_vector() {
        // RFC 8439 §2.3.2
        let mut key = [0u8; 32];
        for i in 0..32 { key[i] = i as u8; }
        let nonce = [0, 0, 0, 9, 0, 0, 0, 74, 0, 0, 0, 0];
        let blk = chacha20_block(&key, 1, &nonce);
        let expect = hex("10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4ed2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e");
        assert_eq!(&blk[..], &expect[..]);
    }

    #[test]
    fn poly1305_rfc_vector() {
        // RFC 8439 §2.5.2
        let key = hex("85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b");
        let mut k = [0u8; 32]; k.copy_from_slice(&key);
        let msg = b"Cryptographic Forum Research Group";
        let t = poly1305(&k, msg);
        assert_eq!(t.to_vec(), hex("a8061dc1305136c6c22b8baf0c0127a9"));
    }

    #[test]
    fn aead_rfc_vector() {
        // RFC 8439 §2.8.2
        let key = hex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
        let mut k = [0u8; 32]; k.copy_from_slice(&key);
        let nonce_v = hex("070000004041424344454647");
        let mut nonce = [0u8; 12]; nonce.copy_from_slice(&nonce_v);
        let aad = hex("50515253c0c1c2c3c4c5c6c7");
        let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let sealed = seal(&k, &nonce, &aad, pt);
        let ct = &sealed[12..sealed.len() - 16];
        let tg = &sealed[sealed.len() - 16..];
        assert_eq!(ct, &hex("d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b6116")[..]);
        assert_eq!(tg, &hex("1ae10b594f09e26a7e902ecbd0600691")[..]);
        // Round-trip + tamper detection.
        assert_eq!(open(&k, &aad, &sealed).unwrap(), pt);
        let mut bad = sealed.clone(); bad[20] ^= 1;
        assert!(open(&k, &aad, &bad).is_none());
    }
}
