// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 2.15–2.17 Отчёт по створу, водосбор линии, готовый водосбор.
//! Порт `topo_gauge.py` модуля QGIS.
//!
//! Морфометрия водосбора от точки замыкания: площадь, длина,
//! уклон, форма, высоты, lake-индекс, time-of-concentration.

/// Отчёт по створу.
#[derive(Debug, Clone, Default)]
pub struct GaugeReport {
    /// Площадь водосбора (м²).
    pub area: f64,
    /// Длина главного водотока (м).
    pub flow_length: f64,
    /// Средний уклон водотока (доли).
    pub slope_mean: f64,
    /// Уклон по 10-85% (доли).
    pub slope_1085: f64,
    /// Высшая точка водосбора (м).
    pub zmax: f64,
    /// Низшая точка (м).
    pub zmin: f64,
    /// Отметка створа (м).
    pub zoutlet: f64,
    /// Число ячеек водосбора.
    pub cells: usize,
    /// Периметр (м).
    pub perimeter: f64,
    /// Гравитационный индекс (м).
    pub relief_ratio: f64,
    /// Time of concentration по Кирпичу (с).
    pub tc_kirpich: f64,
    /// Форма водосбора: длина/ширина.
    pub elongation: f64,
}

/// 2.15 Отчёт по створу: морфометрия водосбора от точки.
#[allow(clippy::too_many_arguments)]
pub fn gauge_report(
    z: &[f64],
    dir: &[i32],
    basin: &[bool],
    rows: usize,
    cols: usize,
    outlet_row: usize,
    outlet_col: usize,
    cell: f64,
    nodata: f64,
) -> GaugeReport {
    let n = rows * cols;
    let mut rep = GaugeReport::default();
    rep.cells = basin.iter().filter(|&&b| b).count();
    rep.area = rep.cells as f64 * cell * cell;

    if rep.cells == 0 {
        return rep;
    }

    // Высоты.
    let mut zmax = f64::NEG_INFINITY;
    let mut zmin = f64::INFINITY;
    for k in 0..n {
        if basin[k] && z[k] != nodata && z[k].is_finite() {
            zmax = zmax.max(z[k]);
            zmin = zmin.min(z[k]);
        }
    }
    let outlet = outlet_row * cols + outlet_col;
    rep.zoutlet = z[outlet];
    rep.zmax = if zmax == f64::NEG_INFINITY { rep.zoutlet } else { zmax };
    rep.zmin = if zmin == f64::INFINITY { rep.zoutlet } else { zmin };

    // Длина водотока: от створа вверх до истока по D8.
    rep.flow_length = trace_upstream(dir, basin, rows, cols, outlet_row, outlet_col, cell);

    // Уклоны.
    if rep.flow_length > 0.0 {
        let drop = rep.zmax - rep.zoutlet;
        rep.slope_mean = drop / rep.flow_length;
        // Уклон по 10-85% длины.
        let p10 = 0.10 * rep.flow_length;
        let p85 = 0.85 * rep.flow_length;
        let z10 = sample_upstream(dir, z, basin, rows, cols, outlet_row, outlet_col, cell, p10, nodata);
        let z85 = sample_upstream(dir, z, basin, rows, cols, outlet_row, outlet_col, cell, p85, nodata);
        if let (Some(a), Some(b)) = (z10, z85) {
            let d1085 = p85 - p10;
            if d1085 > 0.0 {
                rep.slope_1085 = (a - b) / d1085;
            }
        }
    }

    // Периметр: длина границы водосбора.
    rep.perimeter = basin_perimeter(basin, rows, cols, cell);

    // Отношение relief/length.
    if rep.flow_length > 0.0 {
        rep.relief_ratio = (rep.zmax - rep.zmin) / rep.flow_length;
    }

    // Time of concentration (Кирпич): tc = 0.0078 * L^0.77 / S^0.385.
    if rep.slope_mean > 0.0 {
        rep.tc_kirpich = 0.0078 * rep.flow_length.powf(0.77) / rep.slope_mean.powf(0.385);
    }

    // Вытянутость: длина / sqrt(площадь).
    let width = (rep.area / std::f64::consts::PI).sqrt() * 2.0;
    if width > 0.0 {
        rep.elongation = rep.flow_length / width;
    }

    rep
}

/// Длина главного водотока: от створа вверх по D8.
fn trace_upstream(
    dir: &[i32],
    basin: &[bool],
    rows: usize,
    cols: usize,
    outlet_row: usize,
    outlet_col: usize,
    cell: f64,
) -> f64 {
    const OFF: [(isize, isize); 8] = [
        (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0),
    ];
    // Обратные: кто вливается в кого.
    let n = rows * cols;
    let mut inflow: Vec<Vec<usize>> = vec![Vec::new(); n];
    for k in 0..n {
        if !basin[k] || dir[k] < 0 {
            continue;
        }
        let d = dir[k];
        let r = k / cols;
        let c = k % cols;
        let (dr, dc) = OFF[d as usize];
        let nr = r as isize + dr;
        let nc = c as isize + dc;
        if nr >= 0 && nr < rows as isize && nc >= 0 && nc < cols as isize {
            inflow[nr as usize * cols + nc as usize].push(k);
        }
    }

    // Идём вверх от створа, выбирая самый длинный путь.
    let outlet = outlet_row * cols + outlet_col;
    let mut visited = vec![false; n];
    visited[outlet] = true;
    let mut best = 0.0f64;

    // DFS: для каждого соседа вливающегося в текущий, рекурсивно.
    #[allow(clippy::too_many_arguments)]
    fn dfs(
        k: usize,
        dir: &[i32],
        inflow: &[Vec<usize>],
        basin: &[bool],
        visited: &mut Vec<bool>,
        acc: f64,
        best: &mut f64,
    ) {
        if acc > *best {
            *best = acc;
        }
        for &up in &inflow[k] {
            if !basin[up] || visited[up] {
                continue;
            }
            visited[up] = true;
            let d = dir[up];
            let dist = if d % 2 == 0 {
                std::f64::consts::SQRT_2
            } else {
                1.0
            };
            dfs(up, dir, inflow, basin, visited, acc + dist, best);
        }
    }

    dfs(outlet, dir, &inflow, basin, &mut visited, 0.0, &mut best);
    best * cell
}

/// Высота на заданном расстоянии вверх по водотоку.
#[allow(clippy::too_many_arguments)]
fn sample_upstream(
    dir: &[i32],
    z: &[f64],
    basin: &[bool],
    rows: usize,
    cols: usize,
    outlet_row: usize,
    outlet_col: usize,
    cell: f64,
    target: f64,
    nodata: f64,
) -> Option<f64> {
    const OFF: [(isize, isize); 8] = [
        (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0),
    ];
    let outlet = outlet_row * cols + outlet_col;
    // Простой обход до нужного расстояния.
    let mut k = outlet;
    let mut dist = 0.0;
    while dist < target {
        // Ищем соседа, вливающегося в k.
        let r = k / cols;
        let c = k % cols;
        let mut next = None;
        for (d, (dr, dc)) in OFF.iter().enumerate() {
            let (dr, dc) = (*dr, *dc);
            let nr = r as isize + dr;
            let nc = c as isize + dc;
            if nr < 0 || nr >= rows as isize || nc < 0 || nc >= cols as isize {
                continue;
            }
            let nk = nr as usize * cols + nc as usize;
            if basin[nk] && dir[nk] == d as i32 {
                next = Some(nk);
                break;
            }
        }
        match next {
            Some(nk) => {
                let step = if dir[nk] % 2 == 0 {
                    std::f64::consts::SQRT_2 * cell
                } else {
                    cell
                };
                dist += step;
                k = nk;
            }
            None => break,
        }
    }
    if z[k] != nodata && z[k].is_finite() {
        Some(z[k])
    } else {
        None
    }
}

/// Периметр водосбора: длина границы между basin=true и false.
fn basin_perimeter(basin: &[bool], rows: usize, cols: usize, cell: f64) -> f64 {
    let mut edges = 0.0;
    for r in 0..rows {
        for c in 0..cols {
            let k = r * cols + c;
            if !basin[k] {
                continue;
            }
            // Граница: сосед не в водосборе.
            if r == 0 || !basin[(r - 1) * cols + c] {
                edges += 1.0;
            }
            if r == rows - 1 || !basin[(r + 1) * cols + c] {
                edges += 1.0;
            }
            if c == 0 || !basin[r * cols + c - 1] {
                edges += 1.0;
            }
            if c == cols - 1 || !basin[r * cols + c + 1] {
                edges += 1.0;
            }
        }
    }
    edges * cell
}

/// 2.16 Водосбор линии: все ячейки, из которых вода приходит
/// в любую точку линии. Объединение водосборов всех точек.
pub fn line_catchment(
    dir: &[i32],
    rows: usize,
    cols: usize,
    line: &[(f64, f64)],
    gt: &[f64; 6],
) -> Vec<bool> {
    let mut basin = vec![false; rows * cols];
    for &(x, y) in line {
        let c = ((x - gt[0]) / gt[1]).round() as usize;
        let r = ((y - gt[3]) / gt[5]).round() as usize;
        if r < rows && c < cols {
            let ws = crate::topo_flow::watershed(dir, rows, cols, r, c);
            for k in 0..basin.len() {
                basin[k] |= ws[k];
            }
        }
    }
    basin
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topo_flow::d8_flow;

    /// Один холм: водосбор охватывает весь склон.
    #[test]
    fn hill_gauge() {
        let rows = 21;
        let cols = 21;
        let mut z = vec![0.0f64; rows * cols];
        for r in 0..rows {
            for c in 0..cols {
                let x = c as f64 - 10.0;
                let y = r as f64 - 10.0;
                z[r * cols + c] = 20.0 - (x * x + y * y).sqrt();
            }
        }
        let dir = d8_flow(&z, rows, cols, -9999.0);
        // Створ на краю.
        let ws = crate::topo_flow::watershed(&dir, rows, cols, 10, 20);
        let rep = gauge_report(&z, &dir, &ws, rows, cols, 10, 20, 1.0, -9999.0);
        assert!(rep.cells > 0, "cells={}", rep.cells);
        assert!(rep.area > 0.0);
        assert!(rep.zmax > rep.zoutlet, "zmax={} zout={}", rep.zmax, rep.zoutlet);
    }

    /// Линия на склоне: водосбор не пустой.
    #[test]
    fn line_catchment_not_empty() {
        let rows = 11;
        let cols = 11;
        let z: Vec<f64> = (0..rows)
            .flat_map(|r| (0..cols).map(move |_c| 10.0 - r as f64))
            .collect();
        let dir = d8_flow(&z, rows, cols, -9999.0);
        let gt = [0.0, 1.0, 0.0, 11.0, 0.0, -1.0];
        let line = [(2.0, 8.0), (8.0, 8.0)];
        let basin = line_catchment(&dir, rows, cols, &line, &gt);
        let count = basin.iter().filter(|&&b| b).count();
        assert!(count > 0, "basin cells={}", count);
    }
}
