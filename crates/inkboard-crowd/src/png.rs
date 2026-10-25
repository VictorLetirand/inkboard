//! A valid PNG of a chosen size without any dependency: 8-bit RGB noise in "stored" deflate blocks (no compression, so the file size is
//! exactly what the scenario asks for). The host and the guests only look at the header (and the host's renderer can decode it).

use crate::rng::Rng;

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let mut crc_input = Vec::with_capacity(4 + body.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(body);
    out.extend_from_slice(&crc_input);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// PNG of `w` x `h` pixels filled with seeded noise (about `3 * w * h` bytes).
#[must_use]
pub fn noise_png(w: u32, h: u32, seed: u64) -> Vec<u8> {
    let (w, h) = (w.max(1), h.max(1));
    let mut rng = Rng::new(seed);
    let row = 1 + 3 * w as usize;
    let mut raw = vec![0u8; row * h as usize];
    for r in raw.chunks_mut(row) {
        r[0] = 0; // filter: none
        rng.fill(&mut r[1..]);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        z.extend_from_slice(&(b.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// PNG of about `bytes` bytes (4:3 picture).
#[must_use]
pub fn noise_png_of_size(bytes: usize, seed: u64) -> Vec<u8> {
    let pixels = (bytes / 3).max(16) as f64;
    let w = (pixels * 4.0 / 3.0).sqrt().max(4.0) as u32;
    let h = ((pixels / f64::from(w)) as u32).max(1);
    noise_png(w, h, seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_png_has_the_requested_size_and_a_readable_header() {
        let p = noise_png_of_size(100_000, 1);
        assert!(p.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!((90_000..115_000).contains(&p.len()), "{}", p.len());
        assert_eq!(&p[12..16], b"IHDR");
        assert_eq!(noise_png_of_size(100_000, 1), p, "same seed, same bytes");
        assert_ne!(noise_png_of_size(100_000, 2), p);
    }

    #[test]
    fn known_checksums() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }
}
