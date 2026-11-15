// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Поля с пространственной структурой для демо-генераторов, порт
//! `grid_isolines/tools/demo_fields.py`: белый шум, сглаженный тремя
//! проходами скользящего среднего, выборка билинейно и значения в
//! заданном диапазоне с наггетом. Порядок обращений к генератору тот же.

use crate::npy_rng::Rng;
use crate::numpy;

/// Квадратное поле построчно.
#[derive(Debug, Clone)]
pub struct Field {
    pub n: usize,
    pub v: Vec<f64>,
}

impl Field {
    fn at(&self, i: usize, j: usize) -> f64 {
        self.v[i * self.n + j]
    }
}

/// `np.convolve(a, ones(l), "same")`: длина `max(len(a), l)`, центр как у
/// NumPy (начало полной свёртки `min - 1 - min / 2`).
fn conv_same(a: &[f64], l: usize) -> Vec<f64> {
    let n = a.len();
    let full = n + l - 1;
    let out_len = n.max(l);
    let m = n.min(l);
    let start = m - 1 - m / 2;
    (start..start + out_len)
        .take(full)
        .map(|k| {
            // a[j] * 1 по j, где 0 <= k - j < l
            let lo = (k + 1).saturating_sub(l);
            let hi = k.min(n - 1);
            let mut s = 0.0;
            for x in &a[lo..=hi] {
                s += x;
            }
            s
        })
        .collect()
}

/// `demo_field(rng, G, w)`: поле GxG со средним 0 и ст. отклонением 1.
pub fn demo_field(rng: &mut Rng, g: usize, w: usize) -> Field {
    let l = 2 * w + 1;
    let mut rows = g;
    let mut cols = g;
    let mut f: Vec<f64> = (0..g * g).map(|_| rng.standard_normal()).collect();
    let ones = vec![1.0; g];
    let d = conv_same(&ones, l);
    for _ in 0..3 {
        // по столбцам (ось 0), затем деление на d по строкам
        let nr = rows.max(l);
        let mut f2 = vec![0.0; nr * cols];
        for j in 0..cols {
            let col: Vec<f64> = (0..rows).map(|i| f[i * cols + j]).collect();
            let c = conv_same(&col, l);
            for (i, v) in c.iter().enumerate() {
                f2[i * cols + j] = v / d[i];
            }
        }
        rows = nr;
        // по строкам (ось 1), деление на d по столбцам
        let nc = cols.max(l);
        let mut f3 = vec![0.0; rows * nc];
        for i in 0..rows {
            let c = conv_same(&f2[i * cols..(i + 1) * cols], l);
            for (j, v) in c.iter().enumerate() {
                f3[i * nc + j] = v / d[j];
            }
        }
        cols = nc;
        f = f3;
    }
    let s = {
        let x = numpy::var(&f).sqrt();
        if x == 0.0 { 1.0 } else { x }
    };
    let m = numpy::mean(&f);
    Field {
        n: rows,
        v: f.iter().map(|x| (x - m) / s).collect(),
    }
}

/// `demo_sample`: билинейная выборка поля в точках.
pub fn demo_sample(f: &Field, xs: &[f64], ys: &[f64], ext: (f64, f64, f64, f64)) -> Vec<f64> {
    let (xmin, xmax, ymin, ymax) = ext;
    let g = f.n;
    let nz = |v: f64| if v == 0.0 { 1.0 } else { v };
    let top = (g - 1) as f64;
    xs.iter()
        .zip(ys)
        .map(|(&x, &y)| {
            let fx = (x - xmin) / nz(xmax - xmin) * top;
            let fy = (y - ymin) / nz(ymax - ymin) * top;
            let x0 = (fx.floor() as i64).clamp(0, g as i64 - 1) as usize;
            let x1 = (x0 + 1).min(g - 1);
            let y0 = (fy.floor() as i64).clamp(0, g as i64 - 1) as usize;
            let y1 = (y0 + 1).min(g - 1);
            let tx = fx - x0 as f64;
            let ty = fy - y0 as f64;
            let v0 = f.at(y0, x0) * (1.0 - tx) + f.at(y0, x1) * tx;
            let v1 = f.at(y1, x0) * (1.0 - tx) + f.at(y1, x1) * tx;
            v0 * (1.0 - ty) + v1 * ty
        })
        .collect()
}

/// `demo_values`: значения в [vmin, vmax] - структура плюс наггет долей `nf`.
#[allow(clippy::too_many_arguments)]
pub fn demo_values(
    rng: &mut Rng,
    g: usize,
    w: usize,
    xs: &[f64],
    ys: &[f64],
    ext: (f64, f64, f64, f64),
    vmin: f64,
    vmax: f64,
    nf: f64,
) -> Vec<f64> {
    let field = demo_field(rng, g, w);
    let s = demo_sample(&field, xs, ys, ext);
    let mean = 0.5 * (vmin + vmax);
    let tstd = 0.25 * (vmax - vmin);
    let ks = (1.0 - nf).max(0.0).sqrt() * tstd;
    let kn = nf.max(0.0).sqrt() * tstd;
    let noise: Vec<f64> = (0..xs.len()).map(|_| rng.standard_normal()).collect();
    s.iter()
        .zip(&noise)
        .map(|(v, z)| (mean + v * ks + z * kn).clamp(vmin, vmax))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_mode_like_numpy() {
        // np.convolve([1,2,3,4,5], ones(3), "same") = [3, 6, 9, 12, 9]
        assert_eq!(
            conv_same(&[1.0, 2.0, 3.0, 4.0, 5.0], 3),
            vec![3.0, 6.0, 9.0, 12.0, 9.0]
        );
        // ядро длиннее сигнала: np.convolve([1,2], ones(3), "same") = [1, 3, 3]
        assert_eq!(conv_same(&[1.0, 2.0], 3), vec![1.0, 3.0, 3.0]);
        assert_eq!(
            conv_same(&[1.0, 2.0, 3.0, 4.0], 5),
            vec![3.0, 6.0, 10.0, 10.0, 9.0]
        );
        assert_eq!(
            conv_same(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 4),
            vec![3.0, 6.0, 10.0, 14.0, 18.0, 15.0]
        );
    }
}
