// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Минимальная кривизна с натяжением, часть `grid_isolines/mincurv.py`
//! модуля (без разломов): `grid_points`, `solve`, `sample_bilinear`,
//! `loo_estimates`. SOR девятью цветами по (i%3, j%3), натуральная
//! граница линейной экстраполяцией фиктивных узлов. Порядок обновлений
//! и арифметика те же, что у модуля.

/// Сетка: строки с севера, узлы в центрах ячеек.
#[derive(Debug, Clone)]
pub struct Grid {
    pub nx: usize,
    pub ny: usize,
    pub z: Vec<f64>,
}

impl Grid {
    fn at(&self, i: usize, j: usize) -> f64 {
        self.z[i * self.nx + j]
    }

    /// Значение строки `i` (с фиктивными строками сверху и снизу).
    fn row(&self, i: isize, j: usize) -> f64 {
        let ny = self.ny as isize;
        if i < 0 {
            let k = (-i) as f64;
            (k + 1.0) * self.at(0, j) - k * self.at(1, j)
        } else if i >= ny {
            let k = (i - ny + 1) as f64;
            (k + 1.0) * self.at(self.ny - 1, j) - k * self.at(self.ny - 2, j)
        } else {
            self.at(i as usize, j)
        }
    }

    /// Значение с фиктивными узлами по краю (`_pad_natural`, pad = 2):
    /// сначала строки, затем столбцы по уже дополненным строкам.
    fn padded(&self, i: isize, j: isize) -> f64 {
        let nx = self.nx as isize;
        if j < 0 {
            let k = (-j) as f64;
            (k + 1.0) * self.row(i, 0) - k * self.row(i, 1)
        } else if j >= nx {
            let k = (j - nx + 1) as f64;
            (k + 1.0) * self.row(i, self.nx - 1) - k * self.row(i, self.nx - 2)
        } else {
            self.row(i, j as usize)
        }
    }
}

/// Целевое значение узла (`_targets` без разломов).
fn target(g: &Grid, i: usize, j: usize, t: f64, aniso: f64) -> f64 {
    let (i, j) = (i as isize, j as isize);
    let p = |di: isize, dj: isize| g.padded(i + di, j + dj);
    let (n, s, e, w) = (p(-1, 0), p(1, 0), p(0, 1), p(0, -1));
    let (ne, nw, se, sw) = (p(-1, 1), p(-1, -1), p(1, 1), p(1, -1));
    let (nn, ss, ee, ww) = (p(-2, 0), p(2, 0), p(0, 2), p(0, -2));
    let bih = (8.0 * (n + s + e + w) - 2.0 * (ne + nw + se + sw) - (nn + ss + ee + ww)) / 20.0;
    let (wx, wy) = (1.0, aniso);
    let lap = (wx * (e + w) + wy * (n + s)) / (2.0 * wx + 2.0 * wy);
    (1.0 - t) * bih + t * lap
}

/// Параметры решателя.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    pub tension: f64,
    pub boundary_tension: f64,
    pub relax: f64,
    pub aniso: f64,
    pub tol: f64,
}

/// SOR к поверхности минимальной кривизны (`solve`). Узлы `fixed` держатся.
pub fn solve(mut g: Grid, fixed: &[bool], p: &Params, max_iter: usize) -> Grid {
    let (nx, ny) = (g.nx, g.ny);
    if nx < 3 || ny < 3 {
        return g;
    }
    let tb = p.tension.max(p.boundary_tension);
    let tfield = |i: usize, j: usize| {
        if i == 0 || i == ny - 1 || j == 0 || j == nx - 1 {
            tb
        } else {
            p.tension
        }
    };
    let colors: Vec<Vec<usize>> = (0..9)
        .map(|c| {
            (0..ny * nx)
                .filter(|&k| {
                    let (i, j) = (k / nx, k % nx);
                    (i % 3) * 3 + (j % 3) == c && !fixed[k]
                })
                .collect()
        })
        .collect();
    let mut buf = Vec::new();
    for _ in 0..max_iter.max(1) {
        let prev = g.z.clone();
        for m in &colors {
            if m.is_empty() {
                continue;
            }
            buf.clear();
            buf.extend(m.iter().map(|&k| {
                let (i, j) = (k / nx, k % nx);
                target(&g, i, j, tfield(i, j), p.aniso)
            }));
            for (&k, &t) in m.iter().zip(&buf) {
                g.z[k] = g.z[k] + p.relax * (t - g.z[k]);
            }
        }
        let change =
            g.z.iter()
                .zip(&prev)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f64::max);
        if change < p.tol {
            break;
        }
    }
    g
}

/// Привязка сетки: левый нижний угол и ячейка.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub xmin: f64,
    pub ymin: f64,
    pub cell: f64,
    pub nx: usize,
    pub ny: usize,
}

/// Точки к узлам (`grid_points`): в узлах-данных среднее, в свободных -
/// значение ближайшего узла-данных. Возвращает сетку и маску данных.
pub fn grid_points(xs: &[f64], ys: &[f64], vs: &[f64], f: &Frame) -> (Grid, Vec<bool>) {
    let (nx, ny) = (f.nx, f.ny);
    let top = f.ymin + ny as f64 * f.cell;
    let mut sums = vec![0.0; nx * ny];
    let mut cnts = vec![0.0; nx * ny];
    for k in 0..xs.len() {
        let col = (((xs[k] - f.xmin) / f.cell) as i64).clamp(0, nx as i64 - 1) as usize;
        let row = (((top - ys[k]) / f.cell) as i64).clamp(0, ny as i64 - 1) as usize;
        let flat = row * nx + col;
        sums[flat] += vs[k];
        cnts[flat] += 1.0;
    }
    let fixed: Vec<bool> = cnts.iter().map(|c| *c > 0.0).collect();
    let mean = if vs.is_empty() {
        0.0
    } else {
        crate::numpy::mean(vs)
    };
    let mut z: Vec<f64> = (0..nx * ny)
        .map(|k| if fixed[k] { sums[k] / cnts[k] } else { mean })
        .collect();
    let data: Vec<usize> = (0..nx * ny).filter(|&k| fixed[k]).collect();
    let free: Vec<usize> = (0..nx * ny).filter(|&k| !fixed[k]).collect();
    if !data.is_empty() && !free.is_empty() && data.len() * free.len() <= 60_000_000 {
        let dv: Vec<f64> = data.iter().map(|&k| z[k]).collect();
        for &k in &free {
            let (fi, fj) = ((k / nx) as i64, (k % nx) as i64);
            let mut best = 0;
            let mut bd = i64::MAX;
            for (q, &d) in data.iter().enumerate() {
                let (di, dj) = ((d / nx) as i64 - fi, (d % nx) as i64 - fj);
                let d2 = di * di + dj * dj;
                if d2 < bd {
                    bd = d2;
                    best = q;
                }
            }
            z[k] = dv[best];
        }
    }
    (Grid { nx, ny, z }, fixed)
}

/// Билинейное значение в точке (`sample_bilinear`).
pub fn sample_bilinear(g: &Grid, f: &Frame, x: f64, y: f64) -> f64 {
    let top = f.ymin + f.ny as f64 * f.cell;
    let fx = (x - f.xmin) / f.cell - 0.5;
    let fy = (top - y) / f.cell - 0.5;
    let (j0f, i0f) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - j0f, fy - i0f);
    let j0 = (j0f as i64).clamp(0, f.nx as i64 - 1) as usize;
    let i0 = (i0f as i64).clamp(0, f.ny as i64 - 1) as usize;
    let j1 = (j0 + 1).min(f.nx - 1);
    let i1 = (i0 + 1).min(f.ny - 1);
    let (a, b) = (g.at(i0, j0), g.at(i0, j1));
    let (c, d) = (g.at(i1, j0), g.at(i1, j1));
    (a * (1.0 - tx) + b * tx) * (1.0 - ty) + (c * (1.0 - tx) + d * tx) * ty
}

/// Скользящий контроль (`loo_estimates`): полное решение служит тёплым
/// стартом, для каждой точки из `val_idx` исключаются она сама и точки в
/// буфере `excl`. NaN - точек не осталось.
#[allow(clippy::too_many_arguments)]
pub fn loo_estimates(
    xs: &[f64],
    ys: &[f64],
    vs: &[f64],
    f: &Frame,
    val_idx: &[usize],
    p: &Params,
    base_iter: usize,
    loo_iter: usize,
    excl: (f64, f64),
) -> Vec<f64> {
    let (z0, fixed) = grid_points(xs, ys, vs, f);
    let full = solve(z0, &fixed, p, base_iter);
    let mut ests = vec![f64::NAN; val_idx.len()];
    for (k, &i) in val_idx.iter().enumerate() {
        let keep: Vec<usize> = (0..xs.len())
            .filter(|&q| {
                let drop = q == i
                    || ((excl.0 > 0.0 || excl.1 > 0.0)
                        && (xs[q] - xs[i]).abs() <= excl.0
                        && (ys[q] - ys[i]).abs() <= excl.1);
                !drop
            })
            .collect();
        if keep.len() < 3 {
            continue;
        }
        let pick = |a: &[f64]| keep.iter().map(|&q| a[q]).collect::<Vec<f64>>();
        let (z0i, fxi) = grid_points(&pick(xs), &pick(ys), &pick(vs), f);
        let mut start = full.clone();
        for (q, fx) in fxi.iter().enumerate() {
            if *fx {
                start.z[q] = z0i.z[q];
            }
        }
        let zi = solve(start, &fxi, p, loo_iter);
        ests[k] = sample_bilinear(&zi, f, xs[i], ys[i]);
    }
    ests
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Плоскость воспроизводится точно: натуральная граница.
    #[test]
    fn plane_is_reproduced() {
        let f = Frame {
            xmin: 0.0,
            ymin: 0.0,
            cell: 10.0,
            nx: 12,
            ny: 10,
        };
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for k in 0..9 {
            xs.push(5.0 + f64::from(k % 3) * 50.0);
            ys.push(5.0 + f64::from(k / 3) * 40.0);
        }
        let vs: Vec<f64> = xs.iter().zip(&ys).map(|(x, y)| 2.0 * x - y + 7.0).collect();
        let p = Params {
            tension: 0.0,
            boundary_tension: 0.0,
            relax: 1.85,
            aniso: 1.0,
            tol: 1e-9,
        };
        let (z0, fixed) = grid_points(&xs, &ys, &vs, &f);
        let g = solve(z0, &fixed, &p, 20000);
        let v = sample_bilinear(&g, &f, 65.0, 35.0);
        assert!((v - (2.0 * 65.0 - 35.0 + 7.0)).abs() < 1e-3, "{v}");
    }
}
