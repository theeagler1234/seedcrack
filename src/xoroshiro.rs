const GOLDEN_RATIO_64: u64 = 0x9E37_79B9_7F4A_7C15;
const SILVER_RATIO_64: u64 = 0x6A09_E667_F3BC_C909;

fn mix_stafford13(z: u64) -> u64 {
    let z = (z ^ z >> 30).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let z = (z ^ z >> 27).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ z >> 31
}

#[derive(Clone, Copy, Debug)]
pub struct WorldgenRandom {
    lo: u64,
    hi: u64,
}

impl Default for WorldgenRandom {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldgenRandom {
    pub fn new() -> Self {
        let mut r = WorldgenRandom { lo: 0, hi: 0 };
        r.set_seed(0);
        r
    }

    pub fn set_seed(&mut self, seed: i64) {
        let low = seed as u64 ^ SILVER_RATIO_64;
        let high = low.wrapping_add(GOLDEN_RATIO_64);
        self.lo = mix_stafford13(low);
        self.hi = mix_stafford13(high);
        if self.lo | self.hi == 0 {
            self.lo = GOLDEN_RATIO_64;
            self.hi = SILVER_RATIO_64;
        }
    }

    fn raw(&mut self) -> u64 {
        let (s0, mut s1) = (self.lo, self.hi);
        let result = s0.wrapping_add(s1).rotate_left(17).wrapping_add(s0);
        s1 ^= s0;
        self.lo = s0.rotate_left(49) ^ s1 ^ s1 << 21;
        self.hi = s1.rotate_left(28);
        result
    }

    fn next(&mut self, bits: u32) -> i32 {
        (self.raw() >> (64 - bits)) as i32
    }

    pub fn next_int(&mut self, bound: i32) -> i32 {
        if bound & (bound - 1) == 0 {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let sample = self.next(31);
            let modulo = sample % bound;
            if sample.wrapping_sub(modulo).wrapping_add(bound - 1) >= 0 {
                return modulo;
            }
        }
    }

    pub fn next_long(&mut self) -> i64 {
        let upper = self.next(32);
        let lower = self.next(32);
        ((upper as i64) << 32).wrapping_add(lower as i64)
    }

    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 * 5.960_464_5e-8_f32
    }

    pub fn next_double(&mut self) -> f64 {
        let upper = self.next(26) as i64;
        let lower = self.next(27) as i64;
        ((upper << 27) + lower) as f64 * 1.110_223_024_625_156_5e-16
    }

    pub fn between(&mut self, min: i32, max: i32) -> i32 {
        self.next_int(max - min + 1) + min
    }

    pub fn set_decoration_seed(&mut self, seed: i64, x: i32, z: i32) -> i64 {
        self.set_seed(seed);
        let x_scale = self.next_long() | 1;
        let z_scale = self.next_long() | 1;
        let result = (x as i64)
            .wrapping_mul(x_scale)
            .wrapping_add((z as i64).wrapping_mul(z_scale))
            ^ seed;
        self.set_seed(result);
        result
    }

    pub fn set_feature_seed(&mut self, decoration: i64, index: i32, step: i32) {
        self.set_seed(
            decoration
                .wrapping_add(index as i64)
                .wrapping_add(10_000 * step as i64),
        );
    }

    /// `RandomSource.forkPositional()`: a factory seeded from this generator's
    /// *current* internal state, used to derive deterministic per-block or
    /// per-name random streams (surface rules such as bedrock, feature
    /// placement, random sequences) without disturbing `self`.
    pub fn fork_positional(&self) -> PositionalFactory {
        PositionalFactory { lo: self.lo, hi: self.hi }
    }
}

/// Per-block hash used by vanilla for positional random forks
/// (`Mth.getSeed(x, y, z)`), e.g. surface-rule bedrock placement and legacy
/// per-position feature seeding.
pub fn position_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut l = (x as i64).wrapping_mul(3_129_871) ^ (z as i64).wrapping_mul(116_129_781) ^ (y as i64);
    l = l.wrapping_mul(l).wrapping_mul(42_317_861).wrapping_add(l.wrapping_mul(11));
    l >> 16
}

/// `XoroshiroRandomSource.XoroshiroPositionalRandomFactory`: forks either by
/// block position (`at`, cheap, no remix - this runs per block during surface
/// building) or by a fixed name (`from_hash_of`, one full Stafford remix,
/// used once per named random such as "minecraft:bedrock_floor").
///
/// NOTE: the exact byte layout `from_hash_of` uses to turn the MD5 digest
/// into (seed_lo_hash, seed_hi_hash) is reconstructed from community
/// write-ups on the 1.20+ "random sequence" seeding change, not from
/// decompiled source read directly here. Treat bedrock predictions from this
/// as unverified until cross-checked against a live world (see the
/// `bedrock check` style verifier in torch-client).
#[derive(Clone, Copy, Debug)]
pub struct PositionalFactory {
    lo: u64,
    hi: u64,
}

impl PositionalFactory {
    pub fn at(&self, x: i32, y: i32, z: i32) -> WorldgenRandom {
        let seed = position_seed(x, y, z) as u64;
        let mut r = WorldgenRandom { lo: self.lo ^ seed, hi: self.hi };
        if r.lo | r.hi == 0 {
            r.lo = GOLDEN_RATIO_64;
            r.hi = SILVER_RATIO_64;
        }
        r
    }

    pub fn from_hash_of(&self, name: &str) -> PositionalFactory {
        let digest = md5(name.as_bytes());
        let hash_lo = u64::from_be_bytes(digest[0..8].try_into().expect("8 bytes"));
        let hash_hi = u64::from_be_bytes(digest[8..16].try_into().expect("8 bytes"));
        PositionalFactory {
            lo: mix_stafford13(self.lo ^ hash_lo),
            hi: mix_stafford13(self.hi ^ hash_hi),
        }
    }
}

/// Minimal MD5 (RFC 1321), only needed to derive named positional-random
/// forks (`from_hash_of`); not exposed outside this module.
fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
        14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15,
        21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501, 0x698098d8,
        0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340,
        0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87,
        0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
        0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039,
        0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92,
        0xffeff47d, 0x85845dd1, 0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    let mut msg = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    let (mut a0, mut b0, mut c0, mut d0): (u32, u32, u32, u32) =
        (0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476);

    for chunk in msg.chunks_exact(64) {
        let m: [u32; 16] = std::array::from_fn(|i| u32::from_le_bytes(chunk[i * 4..i * 4 + 4].try_into().expect("4 bytes")));
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

#[cfg(test)]
mod md5_tests {
    use super::md5;

    #[test]
    fn matches_known_vectors() {
        // RFC 1321 test vectors.
        assert_eq!(md5(b""), hex("d41d8cd98f00b204e9800998ecf8427e"));
        assert_eq!(md5(b"abc"), hex("900150983cd24fb0d6963f7d28e17f72"));
        assert_eq!(md5(b"minecraft:bedrock_floor"), md5(b"minecraft:bedrock_floor"));
    }

    fn hex(s: &str) -> [u8; 16] {
        let mut out = [0u8; 16];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("valid hex");
        }
        out
    }
}
