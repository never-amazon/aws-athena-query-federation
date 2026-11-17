#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng { state: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn between(&mut self, min: i64, max: i64) -> i64 {
        let span = (max as i128 - min as i128 + 1) as u128;
        (min as i128 + (self.next_u64() as u128 % span) as i128) as i64
    }

    pub fn between_i128(&mut self, min: i128, max: i128) -> i128 {
        let span = (max - min + 1) as u128;
        let wide = ((self.next_u64() as u128) << 64) | self.next_u64() as u128;
        min + (wide % span) as i128
    }

    pub fn string(&mut self, min_len: usize) -> String {
        let len = self.between(min_len as i64, 8) as usize;
        (0..len)
            .map(|_| {
                if self.between(0, 4) == 0 {
                    char::from_u32(self.between(0xA0, 0x2FFF) as u32).unwrap_or('x')
                } else {
                    char::from_u32(self.between(0x20, 0x7E) as u32).unwrap_or('x')
                }
            })
            .collect()
    }
}
