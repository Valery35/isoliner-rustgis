// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Генератор `np.random.default_rng(seed)` NumPy бит в бит: засев
//! `SeedSequence`, PCG64 (XSL-RR 128/64), равномерное, нормальное
//! зиккуратом и целые методом Лемира. Демо-генераторы модуля (1.11,
//! 1.10, 3.08) и случайные выборки (1.09) при том же зерне дают в
//! RustGIS те же данные, что в QGIS. Перенос сделан по исходникам
//! NumPy 2.4.6 (`numpy/random/bit_generator.pyx`,
//! `src/pcg64/pcg64.h`, `src/distributions/distributions.c`).

use crate::ziggurat::{FI, KI, NOR_INV_R, NOR_R, WI};

const INIT_A: u32 = 0x43b0_d7e5;
const MULT_A: u32 = 0x931e_8875;
const INIT_B: u32 = 0x8b51_f9dd;
const MULT_B: u32 = 0x58f3_8ded;
const MIX_MULT_L: u32 = 0xca01_f9dd;
const MIX_MULT_R: u32 = 0x4973_f715;
const POOL: usize = 4;

const PCG_MULT: u128 = (2_549_297_995_355_413_924u128 << 64) | 4_865_540_595_714_422_341u128;

fn hashmix(value: u32, hash_const: &mut u32) -> u32 {
    let mut v = value ^ *hash_const;
    *hash_const = hash_const.wrapping_mul(MULT_A);
    v = v.wrapping_mul(*hash_const);
    v ^ (v >> 16)
}

fn mix(x: u32, y: u32) -> u32 {
    let r = MIX_MULT_L
        .wrapping_mul(x)
        .wrapping_sub(MIX_MULT_R.wrapping_mul(y));
    r ^ (r >> 16)
}

/// Слова `uint32` целого зерна, младшие первыми (`_int_to_uint32_array`).
fn entropy_words(seed: u128) -> Vec<u32> {
    if seed == 0 {
        return vec![0];
    }
    let mut n = seed;
    let mut out = Vec::new();
    while n > 0 {
        out.push((n & 0xffff_ffff) as u32);
        n >>= 32;
    }
    out
}

/// `SeedSequence(seed).generate_state(n, uint64)`.
fn seed_state(seed: u128, n64: usize) -> Vec<u64> {
    let entropy = entropy_words(seed);
    let mut pool = [0u32; POOL];
    let mut hc = INIT_A;
    for (i, p) in pool.iter_mut().enumerate() {
        *p = hashmix(entropy.get(i).copied().unwrap_or(0), &mut hc);
    }
    for src in 0..POOL {
        for dst in 0..POOL {
            if src != dst {
                let h = hashmix(pool[src], &mut hc);
                pool[dst] = mix(pool[dst], h);
            }
        }
    }
    for &e in entropy.iter().skip(POOL) {
        for p in &mut pool {
            let h = hashmix(e, &mut hc);
            *p = mix(*p, h);
        }
    }
    let mut hb = INIT_B;
    let words: Vec<u32> = (0..n64 * 2)
        .map(|i| {
            let mut d = pool[i % POOL] ^ hb;
            hb = hb.wrapping_mul(MULT_B);
            d = d.wrapping_mul(hb);
            d ^ (d >> 16)
        })
        .collect();
    words
        .chunks(2)
        .map(|w| u64::from(w[0]) | (u64::from(w[1]) << 32))
        .collect()
}

/// `np.random.Generator(PCG64(seed))`.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u128,
    inc: u128,
    /// Половина 64-битного слова, отложенная `next_u32` (`has_uint32`).
    half: Option<u32>,
}

impl Rng {
    /// `np.random.default_rng(seed)` для целого неотрицательного зерна.
    pub fn new(seed: u128) -> Self {
        let s = seed_state(seed, 4);
        let initstate = (u128::from(s[0]) << 64) | u128::from(s[1]);
        let initseq = (u128::from(s[2]) << 64) | u128::from(s[3]);
        let mut r = Rng {
            state: 0,
            inc: (initseq << 1) | 1,
            half: None,
        };
        r.step();
        r.state = r.state.wrapping_add(initstate);
        r.step();
        r
    }

    /// Зерно модуля `seed if seed and seed > 0 else None`: без зерна -
    /// случайное от часов и адреса, как свежая энтропия NumPy.
    pub fn from_module_seed(seed: i64) -> Self {
        if seed > 0 {
            return Rng::new(u128::from(seed.unsigned_abs()));
        }
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        h.write_u128(t);
        let a = h.finish();
        h.write_u64(a);
        Rng::new((u128::from(a) << 64) | u128::from(h.finish()))
    }

    /// Состояние и приращение, как `bit_generator.state` NumPy.
    pub fn state(&self) -> (u128, u128) {
        (self.state, self.inc)
    }

    fn step(&mut self) {
        self.state = self.state.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
    }

    pub fn next_u64(&mut self) -> u64 {
        self.step();
        let hi = (self.state >> 64) as u64;
        let lo = self.state as u64;
        (hi ^ lo).rotate_right((self.state >> 122) as u32)
    }

    pub fn next_u32(&mut self) -> u32 {
        if let Some(h) = self.half.take() {
            return h;
        }
        let n = self.next_u64();
        self.half = Some((n >> 32) as u32);
        n as u32
    }

    /// `random()`: [0, 1) с шагом 2^-53.
    pub fn random(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// `uniform(low, high)`.
    pub fn uniform(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.random()
    }

    /// `standard_normal()`: зиккурат NumPy.
    pub fn standard_normal(&mut self) -> f64 {
        loop {
            let mut r = self.next_u64();
            let idx = (r & 0xff) as usize;
            r >>= 8;
            let sign = r & 1;
            let rabs = (r >> 1) & 0x000f_ffff_ffff_ffff;
            let mut x = rabs as f64 * WI[idx];
            if sign & 1 == 1 {
                x = -x;
            }
            if rabs < KI[idx] {
                return x;
            }
            if idx == 0 {
                loop {
                    let xx = -NOR_INV_R * (-self.random()).ln_1p();
                    let yy = -(-self.random()).ln_1p();
                    if yy + yy > xx * xx {
                        return if (rabs >> 8) & 1 == 1 {
                            -(NOR_R + xx)
                        } else {
                            NOR_R + xx
                        };
                    }
                }
            } else if (FI[idx - 1] - FI[idx]) * self.random() + FI[idx] < (-0.5 * x * x).exp() {
                return x;
            }
        }
    }

    /// `normal(loc, scale)`.
    pub fn normal(&mut self, loc: f64, scale: f64) -> f64 {
        loc + scale * self.standard_normal()
    }

    /// `normal(loc, scale, n)`.
    pub fn normals(&mut self, loc: f64, scale: f64, n: usize) -> Vec<f64> {
        (0..n).map(|_| self.normal(loc, scale)).collect()
    }

    /// `integers(low, high)`: целое в [low, high), int64, метод Лемира.
    pub fn integers(&mut self, low: i64, high: i64) -> i64 {
        let rng = high.wrapping_sub(low).wrapping_sub(1) as u64;
        low.wrapping_add(self.bounded(rng) as i64)
    }

    /// Целое в [0, rng] (`random_bounded_uint64`, без маски).
    fn bounded(&mut self, rng: u64) -> u64 {
        if rng == 0 {
            0
        } else if rng < 0xffff_ffff {
            let excl = rng as u32 + 1;
            let mut m = u64::from(self.next_u32()) * u64::from(excl);
            let mut left = m as u32;
            if left < excl {
                let threshold = (u32::MAX - rng as u32) % excl;
                while left < threshold {
                    m = u64::from(self.next_u32()) * u64::from(excl);
                    left = m as u32;
                }
            }
            m >> 32
        } else if rng == 0xffff_ffff {
            u64::from(self.next_u32())
        } else if rng == u64::MAX {
            self.next_u64()
        } else {
            let excl = rng + 1;
            let mut m = u128::from(self.next_u64()) * u128::from(excl);
            let mut left = m as u64;
            if left < excl {
                let threshold = (u64::MAX - rng) % excl;
                while left < threshold {
                    m = u128::from(self.next_u64()) * u128::from(excl);
                    left = m as u64;
                }
            }
            (m >> 64) as u64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Эталоны сняты NumPy 2.4.6 (Python из QGIS 4.2.3).
    #[test]
    fn seed_42_state_like_numpy() {
        let r = Rng::new(42);
        assert_eq!(
            r.state(),
            (
                274_674_114_334_540_486_603_088_602_300_644_985_544,
                332_724_090_758_049_132_448_979_897_138_935_081_983
            )
        );
    }
}
