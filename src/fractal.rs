// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Размерность линии методом циркуля Ричардсона, часть
//! `grid_isolines/fractal.py` модуля (`divider_dimension`).

use crate::numpy;

/// Длина ломаной: длины звеньев складываются как у `np.sum`.
fn polyline_length(p: &[[f64; 2]]) -> f64 {
    let seg: Vec<f64> = p
        .windows(2)
        .map(|w| {
            let (dx, dy) = (w[1][0] - w[0][0], w[1][1] - w[0][1]);
            (dx * dx + dy * dy).sqrt()
        })
        .collect();
    numpy::sum(&seg)
}

/// Число шагов циркуля раствора `r` вдоль ломаной (`_divider_steps`).
fn divider_steps(p: &[[f64; 2]], r: f64) -> usize {
    if p.len() < 2 || r <= 0.0 {
        return 0;
    }
    let mut steps = 0;
    let mut cur = p[0];
    let mut i = 0;
    loop {
        // первая точка ломаной на расстоянии r от cur
        let mut j = i;
        let mut found = false;
        while j < p.len() - 1 {
            let a = if j > i { p[j] } else { cur };
            let b = p[j + 1];
            let da = [a[0] - cur[0], a[1] - cur[1]];
            let db = [b[0] - a[0], b[1] - a[1]];
            let aa = db[0] * db[0] + db[1] * db[1];
            let bb = 2.0 * (da[0] * db[0] + da[1] * db[1]);
            let cc = (da[0] * da[0] + da[1] * da[1]) - r * r;
            if aa > 0.0 {
                let disc = bb * bb - 4.0 * aa * cc;
                if disc >= 0.0 {
                    let tt = (-bb + disc.sqrt()) / (2.0 * aa);
                    if (0.0..=1.0).contains(&tt) {
                        cur = [a[0] + tt * db[0], a[1] + tt * db[1]];
                        steps += 1;
                        i = j;
                        found = true;
                        break;
                    }
                }
            }
            j += 1;
        }
        if !found {
            break;
        }
    }
    steps
}

/// Размерность линии методом циркуля: log N(r) ~ -D log r. Возвращает
/// D (NaN, если раствор не набрал трёх годных точек) и шаги по растворам.
pub fn divider_dimension(p: &[[f64; 2]], n_rulers: usize) -> (f64, Vec<usize>) {
    let l = polyline_length(p);
    if l <= 0.0 {
        return (f64::NAN, Vec::new());
    }
    let rulers: Vec<f64> = numpy::linspace((l / 200.0).ln(), (l / 8.0).ln(), n_rulers)
        .into_iter()
        .map(f64::exp)
        .collect();
    let steps: Vec<usize> = rulers.iter().map(|&r| divider_steps(p, r)).collect();
    let ok: Vec<(f64, usize)> = rulers
        .iter()
        .zip(&steps)
        .filter(|(_, s)| **s >= 2)
        .map(|(r, s)| (*r, *s))
        .collect();
    if ok.len() < 3 {
        return (f64::NAN, steps);
    }
    let x: Vec<f64> = ok.iter().map(|(r, _)| r.ln()).collect();
    let y: Vec<f64> = ok.iter().map(|(_, s)| (*s as f64).ln()).collect();
    let mx = numpy::mean(&x);
    let my = numpy::mean(&y);
    let x: Vec<f64> = x.iter().map(|v| v - mx).collect();
    let num: Vec<f64> = x.iter().zip(&y).map(|(a, b)| a * (b - my)).collect();
    let den: Vec<f64> = x.iter().map(|a| a * a).collect();
    (-(numpy::sum(&num)) / numpy::sum(&den), steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Прямая: размерность около 1. Шаги считаются целыми, неполный
    /// последний шаг теряется, отсюда 1.03, у модуля так же.
    #[test]
    fn straight_line_is_one() {
        let p: Vec<[f64; 2]> = (0..=100).map(|i| [f64::from(i), 0.0]).collect();
        let (d, steps) = divider_dimension(&p, 6);
        assert!((d - 1.0).abs() < 0.05, "{d}");
        assert_eq!(steps.len(), 6);
    }
}
