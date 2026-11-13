// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Ячеистая декластеризация, порт `grid_isolines/declus.py` модуля
//! (порт GSLIB declus). Вес пробы обратен числу проб в её ячейке,
//! усреднён по нескольким смещениям начала сетки и нормирован так, что
//! сумма весов равна числу проб. Порядок действий и сложений тот же,
//! что у NumPy, чтобы размер ячейки по минимуму среднего выбирался тем
//! же.

use std::collections::HashMap;

use crate::numpy;

/// Итог перебора размеров ячейки (`declus_sweep`).
#[derive(Debug, Clone, PartialEq)]
pub struct Sweep {
    pub best_cell: f64,
    pub weights: Vec<f64>,
    pub decl_mean: f64,
    pub naive_mean: f64,
    pub sizes: Vec<f64>,
    pub means: Vec<f64>,
}

/// Веса для одного размера ячейки и декластеризованное среднее
/// (`cell_declus`).
pub fn cell_declus(
    xs: &[f64],
    ys: &[f64],
    vs: &[f64],
    cell_x: f64,
    cell_y: f64,
    noff: i64,
) -> (Vec<f64>, f64) {
    let n = xs.len();
    if n == 0 {
        return (Vec::new(), 0.0);
    }
    let cell_x = if cell_x > 0.0 { cell_x } else { 1.0 };
    let cell_y = if cell_y > 0.0 { cell_y } else { 1.0 };
    let noff = noff.max(1);
    let xmin = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let ymin = ys.iter().copied().fold(f64::INFINITY, f64::min);
    let mut acc = vec![0.0; n];
    let mut keys = vec![0i64; n];
    for io in 0..noff {
        let ox = xmin - cell_x * (io as f64 + 0.5) / noff as f64;
        let oy = ymin - cell_y * (io as f64 + 0.5) / noff as f64;
        let mut counts: HashMap<i64, u32> = HashMap::with_capacity(n);
        for i in 0..n {
            let ix = ((xs[i] - ox) / cell_x).floor() as i64;
            let iy = ((ys[i] - oy) / cell_y).floor() as i64;
            // ключ ячейки как у модуля, с переполнением int64 NumPy
            let key = ix.wrapping_mul(100_000_007).wrapping_add(iy);
            keys[i] = key;
            *counts.entry(key).or_insert(0) += 1;
        }
        for i in 0..n {
            let c = counts.get(&keys[i]).copied().unwrap_or(1);
            acc[i] += 1.0 / f64::from(c);
        }
    }
    let w: Vec<f64> = acc.iter().map(|a| a / noff as f64).collect();
    let scale = n as f64 / numpy::sum(&w);
    let w: Vec<f64> = w.iter().map(|x| x * scale).collect();
    let wv: Vec<f64> = w.iter().zip(vs).map(|(a, b)| a * b).collect();
    let decl = numpy::sum(&wv) / numpy::sum(&w);
    (w, decl)
}

/// Перебор размеров ячейки от `cell_min` до `cell_max`, выбор по
/// минимуму или максимуму среднего (`declus_sweep`).
#[allow(clippy::too_many_arguments)]
pub fn declus_sweep(
    xs: &[f64],
    ys: &[f64],
    vs: &[f64],
    cell_min: f64,
    cell_max: f64,
    ncell: i64,
    noff: i64,
    aspect: f64,
    maximum: bool,
) -> Sweep {
    let ncell = ncell.max(1) as usize;
    let lo = cell_min.min(cell_max);
    let mut hi = cell_min.max(cell_max);
    if hi <= lo {
        hi = lo * 2.0 + 1e-9;
    }
    let sizes = numpy::linspace(lo, hi, ncell);
    let mut means = Vec::with_capacity(ncell);
    let mut wlist = Vec::with_capacity(ncell);
    for &c in &sizes {
        let (w, m) = cell_declus(xs, ys, vs, c, c * aspect, noff);
        means.push(m);
        wlist.push(w);
    }
    let k = numpy::arg_extreme(&means, maximum);
    let naive = if vs.is_empty() { 0.0 } else { numpy::mean(vs) };
    Sweep {
        best_cell: sizes[k],
        weights: wlist.swap_remove(k),
        decl_mean: means[k],
        naive_mean: naive,
        sizes,
        means,
    }
}

/// Диапазон размеров ячейки для перебора: от половины шага сети до
/// половины стороны области (`suggest_range`).
pub fn suggest_range(xs: &[f64], ys: &[f64]) -> (f64, f64) {
    let n = xs.len();
    let span = |a: &[f64]| {
        let lo = a.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = a.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        hi - lo
    };
    let (w, h) = if n > 0 {
        (span(xs), span(ys))
    } else {
        (1.0, 1.0)
    };
    let area = (w * h).max(1e-9);
    let nn = (area / n.max(1) as f64).sqrt();
    let cell_min = (nn * 0.5).max(1e-9);
    let mut cell_max = w.max(h) * 0.5;
    if cell_max <= cell_min {
        cell_max = cell_min * 10.0;
    }
    (cell_min, cell_max)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Регулярная сеть: все веса равны единице, среднее не меняется.
    #[test]
    fn regular_grid_gives_equal_weights() {
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        let mut vs = Vec::new();
        for i in 0..10 {
            for j in 0..10 {
                xs.push(f64::from(i) * 10.0);
                ys.push(f64::from(j) * 10.0);
                vs.push(f64::from(i + j));
            }
        }
        let (w, m) = cell_declus(&xs, &ys, &vs, 10.0, 10.0, 4);
        assert!(w.iter().all(|x| (x - 1.0).abs() < 1e-12), "{w:?}");
        assert!((m - numpy::mean(&vs)).abs() < 1e-12);
    }

    /// Скопление в богатом месте: веса проб скопления меньше, среднее
    /// по минимуму ниже наивного, сумма весов равна числу проб.
    #[test]
    fn cluster_in_the_rich_lowers_the_mean() {
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        let mut vs = Vec::new();
        for i in 0..6 {
            for j in 0..6 {
                xs.push(f64::from(i) * 100.0);
                ys.push(f64::from(j) * 100.0);
                vs.push(1.0);
            }
        }
        for k in 0..20 {
            xs.push(250.0 + f64::from(k % 5) * 3.0);
            ys.push(250.0 + f64::from(k / 5) * 3.0);
            vs.push(5.0);
        }
        let (lo, hi) = suggest_range(&xs, &ys);
        assert!(lo > 0.0 && hi > lo);
        let s = declus_sweep(&xs, &ys, &vs, lo, hi, 24, 4, 1.0, false);
        let n = xs.len() as f64;
        assert!((numpy::sum(&s.weights) - n).abs() < 1e-9);
        assert!(s.decl_mean < s.naive_mean);
        assert!(s.weights[40] < s.weights[0]);
        assert_eq!(s.sizes.len(), 24);
        assert_eq!(s.means[numpy::arg_extreme(&s.means, false)], s.decl_mean);
    }

    #[test]
    fn empty_and_degenerate_input() {
        assert_eq!(cell_declus(&[], &[], &[], 1.0, 1.0, 4), (Vec::new(), 0.0));
        // нулевая ячейка заменяется единичной
        let (w, m) = cell_declus(&[0.0, 0.5], &[0.0, 0.0], &[1.0, 3.0], 0.0, 0.0, 1);
        assert_eq!(w, vec![1.0, 1.0]);
        assert_eq!(m, 2.0);
        // все точки в одном месте: диапазон не вырождается
        let (lo, hi) = suggest_range(&[1.0, 1.0], &[2.0, 2.0]);
        assert!(hi > lo);
    }
}
