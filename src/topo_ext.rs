// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 2.09 Вершины и ямы: локальные экстремумы с фильтром превышения.
//! Порт `topo_surface.py::peaks_pits` модуля QGIS.
//!
//! 2.10 Демо-рельеф: детерминированный синтетический грид.
//!
//! 2.24 Снятие мостов с ЦМР: замена ячеек на мостах интерполяцией русла.

/// Вершина или яма.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Extremum {
    Peak,
    Pit,
}

/// Найденный экстремум.
#[derive(Debug, Clone)]
pub struct PeakPit {
    pub kind: Extremum,
    pub row: usize,
    pub col: usize,
    /// Высота.
    pub z: f64,
    /// Превышение над наименьшим соседом в окне.
    pub prominence: f64,
}

/// Локальные экстремумы с фильтром превышения.
/// z — растр (rows × cols), nodata — пропуск, window — окно (нечётное),
/// min_prominence — минимальное превышение.
pub fn peaks_pits(
    z: &[f64],
    rows: usize,
    cols: usize,
    nodata: f64,
    window: usize,
    min_prominence: f64,
) -> Vec<PeakPit> {
    let w = if window % 2 == 0 { window + 1 } else { window };
    let half = (w / 2) as isize;
    let mut out = Vec::new();

    for r in 0..rows {
        for c in 0..cols {
            let k = r * cols + c;
            if z[k] == nodata || !z[k].is_finite() {
                continue;
            }
            let zc = z[k];
            let mut is_peak = true;
            let mut is_pit = true;
            let mut min_neighbor = f64::INFINITY;
            let mut max_neighbor = f64::NEG_INFINITY;

            for dr in -half..=half {
                for dc in -half..=half {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let nr = r as isize + dr;
                    let nc = c as isize + dc;
                    if nr < 0 || nr >= rows as isize || nc < 0 || nc >= cols as isize {
                        continue;
                    }
                    let nk = nr as usize * cols + nc as usize;
                    if z[nk] == nodata || !z[nk].is_finite() {
                        continue;
                    }
                    let zn = z[nk];
                    if zn >= zc {
                        is_peak = false;
                    }
                    if zn <= zc {
                        is_pit = false;
                    }
                    min_neighbor = min_neighbor.min(zn);
                    max_neighbor = max_neighbor.max(zn);
                }
            }

            if is_peak && min_neighbor.is_finite() {
                let prom = zc - min_neighbor;
                if prom >= min_prominence {
                    out.push(PeakPit {
                        kind: Extremum::Peak,
                        row: r,
                        col: c,
                        z: zc,
                        prominence: prom,
                    });
                }
            }
            if is_pit && max_neighbor.is_finite() {
                let prom = max_neighbor - zc;
                if prom >= min_prominence {
                    out.push(PeakPit {
                        kind: Extremum::Pit,
                        row: r,
                        col: c,
                        z: zc,
                        prominence: prom,
                    });
                }
            }
        }
    }
    out
}

/// 2.10 Демо-рельеф: детерминированный синтетический грид.
/// Синусоидальные холмы и долины для проверки инструментов рельефа.
pub fn demo_terrain(rows: usize, cols: usize) -> Vec<f64> {
    let mut z = Vec::with_capacity(rows * cols);
    for r in 0..rows {
        for c in 0..cols {
            let x = c as f64 / cols as f64 * 4.0 * std::f64::consts::PI;
            let y = r as f64 / rows as f64 * 4.0 * std::f64::consts::PI;
            // Два холма и долина.
            let h1 = 50.0 * (x * 0.7).sin() * (y * 0.5).cos();
            let h2 = 30.0 * (x * 1.3 + 1.0).cos() * (y * 0.8).sin();
            z.push(h1 + h2 + 100.0);
        }
    }
    z
}

/// 2.24 Снятие мостов с ЦМР: замена ячеек выше русла интерполяцией.
/// Мост — ячейки, которые выше окрестности и создают искусственную
/// запруду в стоке. Заменяются билинейной интерполяцией соседей.
pub fn remove_bridges(
    z: &mut [f64],
    rows: usize,
    cols: usize,
    nodata: f64,
    threshold: f64,
) -> usize {
    let orig = z.to_vec();
    let mut replaced = 0;

    // Ядро: сравнение с окрестностью 3×3.
    for r in 1..(rows - 1) {
        for c in 1..(cols - 1) {
            let k = r * cols + c;
            if orig[k] == nodata || !orig[k].is_finite() {
                continue;
            }
            // Среднее валидных соседей.
            let mut sum = 0.0;
            let mut cnt = 0;
            for dr in -1isize..=1 {
                for dc in -1isize..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let nk = (r as isize + dr) as usize * cols + (c as isize + dc) as usize;
                    if orig[nk] != nodata && orig[nk].is_finite() {
                        sum += orig[nk];
                        cnt += 1;
                    }
                }
            }
            if cnt < 4 {
                continue; // мало соседей — не трогаем.
            }
            let mean = sum / cnt as f64;
            // Ячейка значительно выше среднего соседа — мост.
            if orig[k] - mean > threshold {
                z[k] = mean;
                replaced += 1;
            }
        }
    }
    replaced
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Один холм — одна вершина.
    #[test]
    fn one_peak() {
        let rows = 15;
        let cols = 15;
        let mut z = vec![0.0f64; rows * cols];
        z[7 * cols + 7] = 20.0; // холм
        let pp = peaks_pits(&z, rows, cols, -9999.0, 3, 1.0);
        let peaks = pp.iter().filter(|p| p.kind == Extremum::Peak).count();
        assert!(peaks >= 1, "peaks={}", peaks);
    }

    /// Одна яма.
    #[test]
    fn one_pit() {
        let rows = 15;
        let cols = 15;
        let mut z = vec![10.0f64; rows * cols];
        z[7 * cols + 7] = 0.0; // яма
        let pp = peaks_pits(&z, rows, cols, -9999.0, 3, 1.0);
        let pits = pp.iter().filter(|p| p.kind == Extremum::Pit).count();
        assert!(pits >= 1, "pits={}", pits);
    }

    /// Демо-рельеф: не плоский.
    #[test]
    fn demo_not_flat() {
        let z = demo_terrain(50, 50);
        let mn = z.iter().cloned().fold(f64::INFINITY, f64::min);
        let mx = z.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert!(mx > mn, "mx={} mn={}", mx, mn);
    }

    /// Мост снимается.
    #[test]
    fn bridge_removed() {
        let rows = 7;
        let cols = 7;
        let mut z = vec![10.0f64; rows * cols];
        z[3 * cols + 3] = 50.0; // мост
        let n = remove_bridges(&mut z, rows, cols, -9999.0, 5.0);
        assert!(n >= 1, "removed={}", n);
        assert!(z[3 * cols + 3] < 50.0, "bridge={}", z[3 * cols + 3]);
    }
}
