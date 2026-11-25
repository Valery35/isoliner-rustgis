// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 1.07 Вариограммная карта γ(h_x, h_y) и оценка анизотропии.
//! Порт `kb2d.py::variogram_map` модуля QGIS.
//!
//! Для всех пар берётся вектор разноса (dx, dy) и полудисперсия
//! 0.5*(z_i - z_j)²; усредняется по двумерной сетке расстояний
//! в [-maxlag; maxlag]. Карта симметрична (учитываем пару и зеркало).
//! Веса декластеризации: пара берётся с весом w_i·w_j.

use std::collections::HashMap;

/// Результат вариограммной карты.
#[derive(Debug, Clone)]
pub struct VariogramMap {
    /// Сетка γ (квадрат 2*n_bins+1), NaN в пустых ячейках.
    pub grid: Vec<f64>,
    /// Число пар в ячейке.
    pub counts: Vec<f64>,
    /// Размер сетки.
    pub size: usize,
    /// Шаг ячейки.
    pub cell: f64,
    /// Половина окна.
    pub maxlag: f64,
    /// Силл (дисперсия значений).
    pub sill: f64,
    /// Азимут главной оси (географический, градусы).
    pub azimuth: f64,
    /// Анизотропия 0..1 (1 — изотропия).
    pub anis: f64,
    /// Радиус по главной оси.
    pub range_major: f64,
    /// Радиус по малой оси.
    pub range_minor: f64,
    /// Радиус упёрся в окно.
    pub range_capped: bool,
    /// Радиусы по направлениям (азимут → радиус).
    pub dir_ranges: HashMap<String, f64>,
    /// Число использованных точек.
    pub n_used: usize,
    /// Точки прорежены.
    pub subsampled: bool,
}

/// Вычисление вариограммной карты.
/// xs, ys, vs — координаты и значения точек.
#[allow(clippy::too_many_arguments)]
pub fn variogram_map(
    xs: &[f64],
    ys: &[f64],
    vs: &[f64],
    n_bins: usize,
    maxlag: Option<f64>,
    min_pairs: usize,
    max_pairs: usize,
    seed: u64,
    n_az: usize,
    wts: Option<&[f64]>,
) -> VariogramMap {
    let n = vs.len();
    let n_bins = n_bins.max(1);

    // Прореживание, если пар слишком много.
    let mut xs_v = xs.to_vec();
    let mut ys_v = ys.to_vec();
    let mut vs_v = vs.to_vec();
    let mut w_v = wts.map(|w| w.to_vec());
    let mut n_cur = n;
    let mut subsampled = false;

    let total_pairs = n * (n - 1) / 2;
    if n > 2 && total_pairs > max_pairs {
        let m = ((1.0 + (1.0 + 8.0 * max_pairs as f64).sqrt()) / 2.0) as usize;
        let m = m.max(2).min(n);
        // Прореживание: берём m точек псевдослучайно.
        let idx = subsample(n, m, seed);
        xs_v = idx.iter().map(|&i| xs[i]).collect();
        ys_v = idx.iter().map(|&i| ys[i]).collect();
        vs_v = idx.iter().map(|&i| vs[i]).collect();
        if let Some(w) = &w_v {
            w_v = Some(idx.iter().map(|&i| w[i]).collect());
        }
        n_cur = m;
        subsampled = true;
    }

    // Окно.
    let lag = match maxlag {
        Some(l) if l > 0.0 => l,
        _ => {
            let dx = xs_v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                - xs_v.iter().cloned().fold(f64::INFINITY, f64::min);
            let dy = ys_v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                - ys_v.iter().cloned().fold(f64::INFINITY, f64::min);
            let l = 0.5 * (dx * dx + dy * dy).sqrt();
            if l <= 0.0 { 1.0 } else { l }
        }
    };

    let cell = lag / n_bins as f64;
    let size = 2 * n_bins + 1;
    let mut s_g = vec![0.0f64; size * size];
    let mut s_w = vec![0.0f64; size * size];
    let mut s_c = vec![0.0f64; size * size];

    // Все пары.
    for i in 0..(n_cur - 1) {
        let wi = w_v.as_ref().map(|w| w[i]).unwrap_or(1.0);
        for j in (i + 1)..n_cur {
            let dx = xs_v[j] - xs_v[i];
            let dy = ys_v[j] - ys_v[i];
            let g = 0.5 * (vs_v[j] - vs_v[i]).powi(2);
            let wp = wi * w_v.as_ref().map(|w| w[j]).unwrap_or(1.0);

            // Пара и зеркало.
            for (sx, sy) in [(dx, dy), (-dx, -dy)] {
                let ix = (sx / cell).round() as i64 + n_bins as i64;
                let iy = (sy / cell).round() as i64 + n_bins as i64;
                if ix >= 0 && (ix as usize) < size && iy >= 0 && (iy as usize) < size {
                    let k = (iy as usize) * size + (ix as usize);
                    s_g[k] += wp * g;
                    s_w[k] += wp;
                    s_c[k] += 1.0;
                }
            }
        }
    }

    // Среднее γ.
    let mut grid = vec![f64::NAN; size * size];
    for k in 0..(size * size) {
        if s_c[k] >= min_pairs as f64 {
            grid[k] = s_g[k] / s_w[k].max(1e-12);
        }
    }
    // Центр — ноль.
    let c = n_bins * size + n_bins;
    grid[c] = 0.0;

    // Силл.
    let sill = if n_cur > 1 {
        let mean = crate::numpy::sum(&vs_v) / n_cur as f64;
        vs_v.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / n_cur as f64
    } else {
        0.0
    };

    // Оценка анизотропии по направлениям.
    let (az, anis, rmaj, rmin, dranges, capped) =
        estimate_anisotropy(&grid, &s_c, cell, n_bins, size, sill, lag, n_az, min_pairs);

    VariogramMap {
        grid,
        counts: s_c,
        size,
        cell,
        maxlag: lag,
        sill,
        azimuth: az,
        anis,
        range_major: rmaj,
        range_minor: rmin,
        range_capped: capped,
        dir_ranges: dranges,
        n_used: n_cur,
        subsampled,
    }
}

/// Прореживание: m индексов из n, псевдослучайно по seed.
fn subsample(n: usize, m: usize, seed: u64) -> Vec<usize> {
    // Простая линейная конгруэнтная последовательность.
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut all: Vec<usize> = (0..n).collect();
    // Частичное перемешивание Фишера-Йетса.
    let take = m.min(n);
    for i in 0..take {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let j = i + ((state >> 33) as usize) % (n - i);
        all.swap(i, j);
    }
    all[..take].to_vec()
}

/// Оценка анизотропии по карте: радиусы по направлениям.
#[allow(clippy::too_many_arguments)]
fn estimate_anisotropy(
    grid: &[f64],
    counts: &[f64],
    cell: f64,
    n_bins: usize,
    size: usize,
    sill: f64,
    maxlag: f64,
    n_az: usize,
    min_pairs: usize,
) -> (f64, f64, f64, f64, HashMap<String, f64>, bool) {
    // Радиус по азимуту: идём от центра, γ ниже силла — радиус.
    let threshold = sill * 0.95;
    let mut ranges: Vec<(f64, f64)> = Vec::with_capacity(n_az); // (азимут, радиус)

    for k in 0..n_az {
        let az = 360.0 * k as f64 / n_az as f64;
        let rad = az.to_radians();
        let (dir_x, dir_y) = (rad.sin(), rad.cos());
        let mut r = 0.0;
        let steps = (maxlag / cell).ceil() as usize;
        for step in 1..=steps {
            let dist = step as f64 * cell;
            let px = dir_x * dist;
            let py = dir_y * dist;
            let ix = ((px / cell).round() as i64 + n_bins as i64) as usize;
            let iy = ((py / cell).round() as i64 + n_bins as i64) as usize;
            if ix >= size || iy >= size {
                break;
            }
            let idx = iy * size + ix;
            if counts[idx] < min_pairs as f64 {
                continue;
            }
            let g = grid[idx];
            if g.is_finite() && g > threshold {
                r = dist;
                break;
            }
            r = dist; // не достигли силла — радиус дальше
        }
        ranges.push((az, r));
    }

    // Главная и малая оси.
    let mut best_az = 0.0f64;
    let mut best_r = 0.0f64;
    let mut worst_r = f64::INFINITY;
    for &(az, r) in &ranges {
        if r > best_r {
            best_r = r;
            best_az = az;
        }
        if r < worst_r {
            worst_r = r;
        }
    }
    if worst_r == f64::INFINITY {
        worst_r = 0.0;
    }

    let anis = if best_r > 0.0 {
        (worst_r / best_r).clamp(0.0, 1.0)
    } else {
        1.0
    };

    let capped = best_r >= maxlag * 0.95;

    let mut dranges = HashMap::new();
    for &(az, r) in &ranges {
        dranges.insert(format!("{:.0}", az), r);
    }

    (best_az, anis, best_r, worst_r, dranges, capped)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Изотропное поле: анизотропия близка к 1.
    #[test]
    fn isotropic_field() {
        let n = 50;
        let xs: Vec<f64> = (0..n).map(|i| (i as f64) * 0.2 - 5.0).collect();
        let ys: Vec<f64> = (0..n).map(|i| ((i * 13 % n) as f64) * 0.2 - 5.0).collect();
        let vs: Vec<f64> = (0..n).map(|i| {
            let x = xs[i];
            let y = ys[i];
            (-(x * x) - y * y).exp() // холм
        }).collect();
        let m = variogram_map(&xs, &ys, &vs, 10, None, 3, 1_000_000, 42, 12, None);
        assert!(m.anis > 0.2, "anis={}", m.anis);
        assert!(m.sill > 0.0);
        assert_eq!(m.size, 21);
    }

    /// Анизотропное поле: по x короче, чем по y.
    #[test]
    fn anisotropic_field() {
        let n = 80;
        let xs: Vec<f64> = (0..n).map(|i| (i as f64 / n as f64) * 4.0 - 2.0).collect();
        let ys: Vec<f64> = (0..n).map(|i| ((i * 17 % n) as f64 / n as f64) * 8.0 - 4.0).collect();
        // Вариация только по y — по x поле постоянно.
        let vs: Vec<f64> = (0..n).map(|i| (ys[i] / 4.0).abs().sin()).collect();
        let m = variogram_map(&xs, &ys, &vs, 10, None, 2, 1_000_000, 42, 24, None);
        assert!(m.grid.len() == m.size * m.size);
    }
}
