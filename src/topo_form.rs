// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 2.19–2.22 Бровки, подошвы, профилирование откосов.
//! Порт `topo_form.py` модуля QGIS.
//!
//! Бровка — линия перегиба вверху откоса, подошва — внизу.
//! Находятся по нормали к градиенту: где уклон резко меняется.

/// Кандидат бровки или подошвы.
#[derive(Debug, Clone)]
pub struct BrowSole {
    pub kind: BrowSoleKind,
    pub row: usize,
    pub col: usize,
    /// Высота.
    pub z: f64,
    /// Уклон до перегиба (градусы).
    pub slope_above: f64,
    /// Уклон после перегиба (градусы).
    pub slope_below: f64,
    /// Контраст: разница уклонов.
    pub contrast: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BrowSoleKind {
    Brow,
    Sole,
}

/// Поиск кандидатов бровок и подошв по ЦМР.
/// threshold — минимальный контраст уклонов (в градусах).
pub fn brow_sole_candidates(
    z: &[f64],
    rows: usize,
    cols: usize,
    nodata: f64,
    cell: f64,
    threshold: f64,
) -> Vec<BrowSole> {
    let n = rows * cols;
    let mut out = Vec::new();

    // Уклоны по Horn 3×3.
    let (slope, _aspect) = crate::topo_surface::slope_aspect(z, &vec![false; n], cols, rows, cell);

    // Сглаженный уклон (окно 5×5) для фона.
    let smooth_slope = smooth_5x5(&slope, rows, cols, nodata);

    for r in 1..(rows - 1) {
        for c in 1..(cols - 1) {
            let k = r * cols + c;
            if z[k] == nodata || !z[k].is_finite() {
                continue;
            }
            let s = slope[k];
            if !s.is_finite() {
                continue;
            }
            let sm = smooth_slope[k];
            if !sm.is_finite() {
                continue;
            }
            let contrast = s - sm;

            if contrast >= threshold && s > sm {
                // Уклон выше фона — бровка.
                out.push(BrowSole {
                    kind: BrowSoleKind::Brow,
                    row: r,
                    col: c,
                    z: z[k],
                    slope_above: s,
                    slope_below: sm,
                    contrast,
                });
            } else if -contrast >= threshold && s < sm {
                // Уклон ниже фона — подошва.
                out.push(BrowSole {
                    kind: BrowSoleKind::Sole,
                    row: r,
                    col: c,
                    z: z[k],
                    slope_above: sm,
                    slope_below: s,
                    contrast: -contrast,
                });
            }
        }
    }
    out
}

/// Сглаживание массива окном 5×5.
fn smooth_5x5(a: &[f64], rows: usize, cols: usize, nodata: f64) -> Vec<f64> {
    let n = rows * cols;
    let mut out = vec![nodata; n];
    for r in 2..(rows - 2) {
        for c in 2..(cols - 2) {
            let k = r * cols + c;
            let mut sum = 0.0;
            let mut cnt = 0;
            for dr in -2isize..=2 {
                for dc in -2isize..=2 {
                    let nk = (r as isize + dr) as usize * cols + (c as isize + dc) as usize;
                    if a[nk].is_finite() {
                        sum += a[nk];
                        cnt += 1;
                    }
                }
            }
            if cnt > 0 {
                out[k] = sum / cnt as f64;
            }
        }
    }
    out
}

/// Профилирование откоса: отметки вдоль линии на ЦМР.
/// vertices — линия на плане, z — ЦМР.
/// Отдаёт (расстояние, отметка) в точках вдоль линии.
pub fn profile_along(
    vertices: &[(f64, f64)],
    z: &[f64],
    rows: usize,
    cols: usize,
    gt: &[f64; 6],
    step: f64,
    nodata: f64,
) -> Vec<(f64, f64)> {
    let st = crate::section::stations(vertices, step);
    if st.is_empty() {
        return Vec::new();
    }
    let xs: Vec<f64> = st.iter().map(|s| s.1).collect();
    let ys: Vec<f64> = st.iter().map(|s| s.2).collect();
    let vals = crate::section::sample_grid_points(z, rows, cols, gt, &xs, &ys, true);

    let mut out = Vec::new();
    for k in 0..st.len() {
        let v = vals[k];
        if v.is_finite() && v != nodata {
            out.push((st[k].0, v));
        }
    }
    out
}

/// 2.21 Демо-карьер: рельеф с известными бровками и подошвами.
pub fn demo_quarry(rows: usize, cols: usize) -> Vec<f64> {
    let mut z = Vec::with_capacity(rows * cols);
    for r in 0..rows {
        for c in 0..cols {
            let x = c as f64;
            let y = r as f64;
            let cx = cols as f64 / 2.0;
            let cy = rows as f64 / 2.0;
            let d = ((x - cx) * (x - cx) + (y - cy) * (y - cy)).sqrt();
            // Карьер: воронка с крутыми стенками.
            let depth = if d < 20.0 {
                -80.0 * (1.0 - d / 20.0)
            } else {
                0.0
            };
            z.push(100.0 + depth);
        }
    }
    z
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ступень на склоне: бровка и подошва.
    #[test]
    fn step_brow_sole() {
        let rows = 30;
        let cols = 30;
        let mut z = vec![0.0f64; rows * cols];
        // Пологий склон, затем крутая ступень, затем пологий.
        for r in 0..rows {
            for c in 0..cols {
                z[r * cols + c] = if r < 15 { r as f64 * 0.5 } else { 15.0 * 0.5 + (r - 15) as f64 * 3.0 };
            }
        }
        let cands = brow_sole_candidates(&z, rows, cols, -9999.0, 1.0, 5.0);
        assert!(!cands.is_empty(), "candidates={}", cands.len());
    }

    /// Демо-карьер: не плоский.
    #[test]
    fn quarry_not_flat() {
        let z = demo_quarry(50, 50);
        let mn = z.iter().cloned().fold(f64::INFINITY, f64::min);
        let mx = z.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert!(mx > mn, "mx={} mn={}", mx, mn);
    }

    /// Профиль вдоль линии на склоне.
    #[test]
    fn profile_slope() {
        let rows = 50;
        let cols = 50;
        let z: Vec<f64> = (0..rows)
            .flat_map(|r| (0..cols).map(move |_c| 100.0 - r as f64))
            .collect();
        let gt = [0.0, 1.0, 0.0, 50.0, 0.0, -1.0];
        let line = [(5.0, 45.0), (5.0, 5.0)]; // вниз по склону
        let prof = profile_along(&line, &z, rows, cols, &gt, 5.0, -9999.0);
        assert!(prof.len() > 3, "len={}", prof.len());
        // Высота убывает.
        assert!(prof[0].1 > prof[prof.len() - 1].1);
    }
}
