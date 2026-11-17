#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn between(&mut self, min: i64, max: i64) -> i64 {
        let span = u128::try_from(i128::from(max) - i128::from(min) + 1).unwrap();
        let offset = i128::try_from(u128::from(self.next_u64()) % span).unwrap();
        i64::try_from(i128::from(min) + offset).unwrap()
    }

    pub fn between_i128(&mut self, min: i128, max: i128) -> i128 {
        let span = u128::try_from(max - min + 1).unwrap();
        let wide = (u128::from(self.next_u64()) << 64) | u128::from(self.next_u64());
        min + i128::try_from(wide % span).unwrap()
    }

    pub fn string(&mut self, min_len: usize) -> String {
        let len = usize::try_from(self.between(
            i64::try_from(min_len).unwrap(),
            i64::try_from(JQWIK_MAX_STRING_LENGTH).unwrap(),
        ))
        .unwrap();
        (0..len)
            .map(|_| {
                jqwik_char(u32::try_from(self.between(0, i64::from(JQWIK_CHAR_COUNT) - 1)).unwrap())
            })
            .collect()
    }
}

pub const JQWIK_MAX_STRING_LENGTH: usize = 255;

const JQWIK_CHAR_RANGES: [(u32, u32); 3] = [(0x0000, 0xD7FF), (0xF900, 0xFDCF), (0xFDF0, 0xFFFD)];

pub const JQWIK_CHAR_COUNT: u32 = (0xD7FF + 1) + (0xFDCF - 0xF900 + 1) + (0xFFFD - 0xFDF0 + 1);

pub fn jqwik_char(index: u32) -> char {
    let mut rest = index % JQWIK_CHAR_COUNT;
    for (lo, hi) in JQWIK_CHAR_RANGES {
        let size = hi - lo + 1;
        if rest < size {
            return char::from_u32(lo + rest).expect("jqwik char ranges exclude surrogates");
        }
        rest -= size;
    }
    unreachable!("index reduced modulo JQWIK_CHAR_COUNT")
}
