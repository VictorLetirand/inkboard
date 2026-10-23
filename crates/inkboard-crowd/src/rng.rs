//! Small seeded generator (SplitMix64): every run with the same seed makes the same boards and the same decisions.

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut r = Self(seed ^ 0x9E37_79B9_7F4A_7C15);
        r.next_u64();
        r
    }

    /// A generator for sub-stream `k` of `seed` (one per guest, per board page...).
    pub fn stream(seed: u64, k: u64) -> Self {
        Self::new(seed.wrapping_mul(0xD134_2543_DE82_EF95) ^ k.wrapping_mul(0xA24B_AED4_963E_E407).wrapping_add(k))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f64()
    }

    /// Uniform integer in `[0, n)` (0 when `n` is 0).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next_u64() % n as u64) as usize }
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.f64() < p
    }

    /// Exponentially distributed delay of the given mean (Poisson arrivals).
    pub fn exp(&mut self, mean: f64) -> f64 {
        -mean * (1.0 - self.f64()).max(1e-12).ln()
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    pub fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let v = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
    }
}
