// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! То, в чём NumPy считает иначе наивного цикла. Сверка с модулем идёт
//! до 1e-9, а выбор размера ячейки по минимуму среднего чувствителен к
//! последнему знаку, поэтому суммы складываются в том же порядке, что
//! у NumPy.

/// Сумма как у `np.sum` для непрерывного массива f64: попарное сложение
/// блоками по 128 с восемью накопителями (`pairwise_sum` в
/// `numpy/core/src/umath/loops_utils.h`).
pub fn sum(a: &[f64]) -> f64 {
    const BLOCK: usize = 128;
    let n = a.len();
    if n < 8 {
        // NumPy начинает с -0.0, на сумму это не влияет
        let mut res = -0.0;
        for &x in a {
            res += x;
        }
        res
    } else if n <= BLOCK {
        let mut r = [0.0f64; 8];
        r.copy_from_slice(&a[..8]);
        let mut i = 8;
        while i + 8 <= n {
            for (k, acc) in r.iter_mut().enumerate() {
                *acc += a[i + k];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        for &x in &a[i..] {
            res += x;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        sum(&a[..n2]) + sum(&a[n2..])
    }
}

/// Встроенная `sum()` Python 3.12 по дробным числам: сложение с
/// компенсацией Ноймайера (`builtin_sum_impl` CPython).
pub fn py_sum(a: impl IntoIterator<Item = f64>) -> f64 {
    let mut s = 0.0f64;
    let mut c = 0.0f64;
    for x in a {
        let t = s + x;
        if s.abs() >= x.abs() {
            c += (s - t) + x;
        } else {
            c += (x - t) + s;
        }
        s = t;
    }
    if c != 0.0 && c.is_finite() { s + c } else { s }
}

/// Среднее как у `np.mean`: сумма делится на длину.
pub fn mean(a: &[f64]) -> f64 {
    sum(a) / a.len() as f64
}

/// `np.linspace(lo, hi, n)`: k-й узел `lo + k * step`, последний - ровно `hi`.
pub fn linspace(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    if n == 1 {
        return vec![lo];
    }
    let step = (hi - lo) / (n - 1) as f64;
    let mut out: Vec<f64> = (0..n)
        .map(|k| {
            if step == 0.0 {
                (k as f64 / (n - 1) as f64) * (hi - lo) + lo
            } else {
                k as f64 * step + lo
            }
        })
        .collect();
    if let Some(last) = out.last_mut() {
        *last = hi;
    }
    out
}

/// Номер первого наименьшего (`np.argmin`) или наибольшего (`np.argmax`).
/// NaN у NumPy выигрывает сразу, здесь так же.
pub fn arg_extreme(a: &[f64], max: bool) -> usize {
    let mut best = 0;
    for (i, &v) in a.iter().enumerate() {
        if v.is_nan() {
            return i;
        }
        let b = a[best];
        if (max && v > b) || (!max && v < b) {
            best = i;
        }
    }
    best
}

/// `np.percentile(v, p)` способом `linear`.
pub fn percentile(v: &[f64], p: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    if s.is_empty() {
        return f64::NAN;
    }
    let pos = p / 100.0 * (s.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = (lo + 1).min(s.len() - 1);
    let t = pos - lo as f64;
    // NumPy: a + (b - a) * t, при t >= 0.5 - b - (b - a) * (1 - t)
    let (a, b) = (s[lo], s[hi]);
    let d = b - a;
    if t >= 0.5 {
        b - d * (1.0 - t)
    } else {
        a + d * t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairwise_sum_matches_plain_on_exact_values() {
        let a: Vec<f64> = (1..=1000).map(f64::from).collect();
        assert_eq!(sum(&a), 500_500.0);
        assert_eq!(sum(&a[..5]), 15.0);
        assert_eq!(sum(&a[..100]), 5050.0);
        assert_eq!(mean(&a[..4]), 2.5);
    }

    /// Попарное сложение теряет меньше наивного: 0.1 тысячу раз.
    #[test]
    fn pairwise_sum_is_closer_than_naive() {
        let a = vec![0.1; 1000];
        let naive: f64 = a.iter().sum();
        assert!((sum(&a) - 100.0).abs() <= (naive - 100.0).abs());
    }

    #[test]
    fn linspace_ends_exactly() {
        let s = linspace(0.3, 7.1, 24);
        assert_eq!(s.len(), 24);
        assert_eq!(s[0], 0.3);
        assert_eq!(s[23], 7.1);
        assert_eq!(s[1], (7.1 - 0.3) / 23.0 + 0.3);
    }

    #[test]
    fn extremes_take_the_first() {
        assert_eq!(arg_extreme(&[3.0, 1.0, 1.0, 2.0], false), 1);
        assert_eq!(arg_extreme(&[3.0, 5.0, 5.0], true), 1);
    }

    #[test]
    fn percentile_is_linear() {
        let v = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(percentile(&v, 50.0), 2.5);
        assert_eq!(percentile(&v, 0.0), 1.0);
        assert_eq!(percentile(&v, 100.0), 4.0);
    }
}
