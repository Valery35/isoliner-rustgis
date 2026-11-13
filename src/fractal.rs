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

/// Итог размерности Минковского.
#[derive(Debug, Clone, PartialEq)]
pub struct Minkowski {
    pub d: f64,
    pub r2: f64,
    pub sizes: Vec<f64>,
    pub counts: Vec<usize>,
}

/// Размерность Минковского набора ломаных (`minkowski_dimension`):
/// покрытие ячейками по лесенке размеров от 1/8 диагонали вниз вдвое, для
/// каждой ступени минимум по `offsets` сдвигам сетки (генератор NumPy с
/// зерном `seed`), точки вдоль звеньев с шагом `densify` размера ячейки.
pub fn minkowski_dimension(
    polylines: &[Vec<[f64; 2]>],
    n_sizes: usize,
    offsets: usize,
    densify: f64,
    seed: u128,
) -> Minkowski {
    let segs: Vec<&Vec<[f64; 2]>> = polylines.iter().filter(|p| p.len() >= 2).collect();
    let nan = Minkowski {
        d: f64::NAN,
        r2: f64::NAN,
        sizes: Vec::new(),
        counts: Vec::new(),
    };
    let (mut xmin, mut ymin) = (f64::INFINITY, f64::INFINITY);
    let (mut xmax, mut ymax) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in &segs {
        for q in p.iter() {
            xmin = xmin.min(q[0]);
            xmax = xmax.max(q[0]);
            ymin = ymin.min(q[1]);
            ymax = ymax.max(q[1]);
        }
    }
    if segs.is_empty() || ![xmin, xmax, ymin, ymax].iter().all(|v| v.is_finite()) {
        return nan;
    }
    let diag = (xmax - xmin).hypot(ymax - ymin);
    let top = diag / 8.0;
    let sizes: Vec<f64> = (0..n_sizes).map(|k| top / 2f64.powi(k as i32)).collect();
    let mut rng = crate::npy_rng::Rng::new(seed);
    let offs = offsets.max(1);
    let mut counts = Vec::new();
    for &s in &sizes {
        let mut shifts = vec![(0.0, 0.0)];
        for _ in 0..offs - 1 {
            let a = rng.random() * s;
            let b = rng.random() * s;
            shifts.push((a, b));
        }
        let step = (densify > 0.0).then_some(s * densify);
        let mut best: Option<usize> = None;
        for (ox, oy) in shifts {
            let mut cells = std::collections::HashSet::new();
            for p in &segs {
                for w in p.windows(2) {
                    let (x0, y0) = (w[0][0], w[0][1]);
                    let (dx, dy) = (w[1][0] - x0, w[1][1] - y0);
                    let l = (dx * dx + dy * dy).sqrt();
                    let pts: Vec<(f64, f64)> = match step {
                        Some(st) => {
                            let n = ((l / st) as usize).max(1);
                            crate::numpy::linspace(0.0, 1.0, n + 1)
                                .into_iter()
                                .map(|t| (x0 + t * dx, y0 + t * dy))
                                .collect()
                        }
                        None => vec![(x0, y0), (x0 + dx, y0 + dy)],
                    };
                    for (x, y) in pts {
                        let i = ((x - xmin + ox) / s).floor() as i64;
                        let j = ((y - ymin + oy) / s).floor() as i64;
                        cells.insert((i, j));
                    }
                }
            }
            let c = cells.len();
            best = Some(best.map_or(c, |b| b.min(c)));
        }
        counts.push(best.unwrap_or(0));
    }
    let ok: Vec<(f64, usize)> = sizes
        .iter()
        .zip(&counts)
        .filter(|(_, c)| **c > 4)
        .map(|(s, c)| (*s, *c))
        .collect();
    if ok.len() < 3 {
        return Minkowski {
            d: f64::NAN,
            r2: f64::NAN,
            sizes,
            counts,
        };
    }
    let x: Vec<f64> = ok.iter().map(|(s, _)| (1.0 / s).ln()).collect();
    let y: Vec<f64> = ok.iter().map(|(_, c)| (*c as f64).ln()).collect();
    let (mx, my) = (numpy::mean(&x), numpy::mean(&y));
    let xc: Vec<f64> = x.iter().map(|v| v - mx).collect();
    let num: Vec<f64> = xc.iter().zip(&y).map(|(a, b)| a * (b - my)).collect();
    let den: Vec<f64> = xc.iter().map(|a| a * a).collect();
    let d = numpy::sum(&num) / numpy::sum(&den);
    let res: Vec<f64> = y
        .iter()
        .zip(&xc)
        .map(|(b, a)| {
            let e = b - (my + d * a);
            e * e
        })
        .collect();
    let tot: Vec<f64> = y.iter().map(|b| (b - my) * (b - my)).collect();
    let ss_tot = numpy::sum(&tot);
    let r2 = if ss_tot > 0.0 {
        1.0 - numpy::sum(&res) / ss_tot
    } else {
        f64::NAN
    };
    Minkowski {
        d,
        r2,
        sizes,
        counts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Отрезок по Минковскому - около 1.
    #[test]
    fn minkowski_of_a_segment() {
        let m = minkowski_dimension(&[vec![[0.0, 0.0], [1000.0, 300.0]]], 8, 3, 0.5, 0);
        assert!((m.d - 1.0).abs() < 0.1, "{m:?}");
        assert_eq!(m.counts.len(), 8);
    }

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
