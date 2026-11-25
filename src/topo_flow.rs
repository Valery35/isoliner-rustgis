// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 2.05–2.07 Сток, аккумуляция, речная сеть, водосборы.
//! Порт `topo_flow.py` модуля QGIS.
//!
//! D8: вода из каждой ячейки течёт в самого низкого из 8 соседей.
//! Аккумуляция: сколько ячеек выше по стоку вливаются в данную.
//! Речная сеть: ячейки с аккумуляцией выше порога.
//! Водосбор: все ячейки, из которых вода приходит в данную точку.

/// Направление стока D8: номер соседа (0-7) или -1 (нет стока, котловина).
/// Соседи: 0=СВ, 1=В, 2=ЮВ, 3=Ю, 4=ЮЗ, 5=З, 6=СЗ, 7=С.
pub fn d8_flow(
    z: &[f64],
    rows: usize,
    cols: usize,
    nodata: f64,
) -> Vec<i32> {
    let n = rows * cols;
    let mut dir = vec![-1i32; n];
    // Смещения соседей: [dr, dc] по часовой от СВ.
    const OFF: [(isize, isize); 8] = [
        (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0),
    ];
    // Веса расстояний: диагональ √2.
    const DIST: [f64; 8] = [
        std::f64::consts::SQRT_2, 1.0, std::f64::consts::SQRT_2, 1.0,
        std::f64::consts::SQRT_2, 1.0, std::f64::consts::SQRT_2, 1.0,
    ];

    for r in 0..rows {
        for c in 0..cols {
            let k = r * cols + c;
            if z[k] == nodata || !z[k].is_finite() {
                continue;
            }
            let zc = z[k];
            let mut best_slope = 0.0f64;
            let mut best_dir = -1i32;
            for (d, &(dr, dc)) in OFF.iter().enumerate() {
                let nr = r as isize + dr;
                let nc = c as isize + dc;
                if nr < 0 || nr >= rows as isize || nc < 0 || nc >= cols as isize {
                    continue;
                }
                let nk = nr as usize * cols + nc as usize;
                if z[nk] == nodata || !z[nk].is_finite() {
                    continue;
                }
                let slope = (zc - z[nk]) / DIST[d];
                if slope > best_slope {
                    best_slope = slope;
                    best_dir = d as i32;
                }
            }
            dir[k] = best_dir;
        }
    }
    dir
}

/// Аккумуляция: число ячеек, вливающихся в данную (включая себя).
/// Веса ячеек можно задать (например, осадки).
pub fn flow_accumulation(
    dir: &[i32],
    weights: Option<&[f64]>,
    rows: usize,
    cols: usize,
    nodata: f64,
) -> Vec<f64> {
    let n = rows * cols;
    let mut acc = weights.map(|w| w.to_vec()).unwrap_or_else(|| vec![1.0; n]);
    // Обратные зависимости: для каждой ячейки — кто в неё вливается.
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
    const OFF: [(isize, isize); 8] = [
        (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0),
    ];
    for k in 0..n {
        let d = dir[k];
        if d < 0 {
            continue;
        }
        let r = k / cols;
        let c = k % cols;
        let (dr, dc) = OFF[d as usize];
        let nr = r as isize + dr;
        let nc = c as isize + dc;
        if nr >= 0 && nr < rows as isize && nc >= 0 && nc < cols as isize {
            deps[nr as usize * cols + nc as usize].push(k);
        }
    }
    // Топологическая сортировка: ячейки без впадающих обрабатываются первыми.
    let mut indeg: Vec<usize> = deps.iter().map(|d| d.len()).collect();
    let mut queue: Vec<usize> = (0..n).filter(|&k| indeg[k] == 0).collect();
    let mut visited = vec![false; n];
    let mut total = vec![0.0f64; n];
    for &k in &queue {
        total[k] = acc[k];
    }
    let mut qi = 0;
    while qi < queue.len() {
        let k = queue[qi];
        qi += 1;
        if visited[k] {
            continue;
        }
        visited[k] = true;
        let d = dir[k];
        if d < 0 {
            continue;
        }
        let r = k / cols;
        let c = k % cols;
        let (dr, dc) = OFF[d as usize];
        let nr = r as isize + dr;
        let nc = c as isize + dc;
        if nr < 0 || nr >= rows as isize || nc < 0 || nc >= cols as isize {
            continue;
        }
        let nk = nr as usize * cols + nc as usize;
        acc[nk] += total[k];
        indeg[nk] -= 1;
        if indeg[nk] == 0 {
            queue.push(nk);
        }
    }
    // Ячейки в циклах (не должно быть после заполнения понижений) — NaN.
    for k in 0..n {
        if !visited[k] {
            acc[k] = nodata;
        } else {
            acc[k] = total[k];
        }
    }
    acc
}

/// Речная сеть: ячейки с аккумуляцией >= threshold.
/// Отдаёт (маска_сети, порядок_Стралера).
pub fn river_network(
    dir: &[i32],
    acc: &[f64],
    rows: usize,
    cols: usize,
    threshold: f64,
) -> (Vec<bool>, Vec<i32>) {
    let n = rows * cols;
    let mut network = vec![false; n];
    let mut strahler = vec![0i32; n];

    const OFF: [(isize, isize); 8] = [
        (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0),
    ];

    // Сеть: аккумуляция >= порога.
    for k in 0..n {
        network[k] = acc[k] >= threshold && acc[k].is_finite();
    }

    // Порядок Стралера: голова = 1, слияние одинаковых = +1, разных = max.
    // Обход в порядке убывания аккумуляции (от истоков вниз).
    let mut order: Vec<usize> = (0..n)
        .filter(|&k| network[k])
        .collect();
    order.sort_by(|&a, &b| acc[b].partial_cmp(&acc[a]).unwrap_or(std::cmp::Ordering::Equal));

    // Обратные: для каждой ячейки — кто в неё вливается.
    let mut inflow: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &k in &order {
        let d = dir[k];
        if d < 0 {
            continue;
        }
        let r = k / cols;
        let c = k % cols;
        let (dr, dc) = OFF[d as usize];
        let nr = r as isize + dr;
        let nc = c as isize + dc;
        if nr >= 0 && nr < rows as isize && nc >= 0 && nc < cols as isize {
            let nk = nr as usize * cols + nc as usize;
            if network[nk] {
                inflow[nk].push(k);
            }
        }
    }

    // Стралер по порядку убывания (истоки первыми).
    for &k in &order {
        let sources: Vec<i32> = inflow[k].iter().map(|&up| strahler[up]).collect();
        if sources.is_empty() {
            strahler[k] = 1; // голова
        } else {
            let max1 = sources.iter().max().copied().unwrap_or(0);
            let count_max = sources.iter().filter(|&&s| s == max1).count();
            if count_max >= 2 {
                strahler[k] = max1 + 1; // слияние одинаковых
            } else {
                strahler[k] = max1; // продолжение
            }
        }
    }

    (network, strahler)
}

/// Водосбор от точки: все ячейки, из которых вода приходит в данную.
/// Возвращает маску водосбора.
pub fn watershed(
    dir: &[i32],
    rows: usize,
    cols: usize,
    outlet_row: usize,
    outlet_col: usize,
) -> Vec<bool> {
    let n = rows * cols;
    let mut basin = vec![false; n];
    const OFF: [(isize, isize); 8] = [
        (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0),
    ];

    // Обход в ширину от замыкающего створа.
    let outlet = outlet_row * cols + outlet_col;
    let mut stack = vec![outlet];
    basin[outlet] = true;

    // Обратные зависимости: кто вливается в кого.
    let mut inflow: Vec<Vec<usize>> = vec![Vec::new(); n];
    for k in 0..n {
        let d = dir[k];
        if d < 0 {
            continue;
        }
        let r = k / cols;
        let c = k % cols;
        let (dr, dc) = OFF[d as usize];
        let nr = r as isize + dr;
        let nc = c as isize + dc;
        if nr >= 0 && nr < rows as isize && nc >= 0 && nc < cols as isize {
            inflow[nr as usize * cols + nc as usize].push(k);
        }
    }

    while let Some(k) = stack.pop() {
        for &up in &inflow[k] {
            if !basin[up] {
                basin[up] = true;
                stack.push(up);
            }
        }
    }
    basin
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Холм: вода стекает со склона.
    #[test]
    fn d8_hill() {
        let rows = 11;
        let cols = 11;
        let mut z = vec![0.0f64; rows * cols];
        for r in 0..rows {
            for c in 0..cols {
                let x = c as f64 - 5.0;
                let y = r as f64 - 5.0;
                z[r * cols + c] = 10.0 - (x * x + y * y).sqrt();
            }
        }
        let dir = d8_flow(&z, rows, cols, -9999.0);
        // Центр холма — не котловина (все 8 соседей ниже, вода уходит).
        let c0 = 5 * cols + 5;
        assert!(dir[c0] >= 0, "центр: dir={}", dir[c0]);
        // Склон — сток есть.
        let cs = 5 * cols + 8;
        assert!(dir[cs] >= 0, "склон: dir={}", dir[cs]);
    }

    /// Аккумуляция: на холме вершина получает минимум.
    #[test]
    fn accumulation_hill() {
        let rows = 11;
        let cols = 11;
        let mut z = vec![0.0f64; rows * cols];
        for r in 0..rows {
            for c in 0..cols {
                let x = c as f64 - 5.0;
                let y = r as f64 - 5.0;
                z[r * cols + c] = 10.0 - (x * x + y * y).sqrt();
            }
        }
        let dir = d8_flow(&z, rows, cols, -9999.0);
        let acc = flow_accumulation(&dir, None, rows, cols, -9999.0);
        // Вершина — аккумуляция 1 (только себя).
        let top = 5 * cols + 5;
        assert!((acc[top] - 1.0).abs() < 1e-10, "top acc={}", acc[top]);
    }

    /// Водосбор от створа.
    #[test]
    fn watershed_from_outlet() {
        // Единственный склон: вода течёт на восток.
        let rows = 5;
        let cols = 5;
        let z: Vec<f64> = (0..rows)
            .flat_map(|r| (0..cols).map(move |c| 10.0 - c as f64))
            .collect();
        let dir = d8_flow(&z, rows, cols, -9999.0);
        let acc = flow_accumulation(&dir, None, rows, cols, -9999.0);
        // Водосбор от правого нижнего угла.
        let ws = watershed(&dir, rows, cols, 2, 4);
        // Правый край — всё вливается в него.
        let out = 2 * cols + 4;
        assert!(ws[out], "створ в водосборе");
        // Левый край — тоже (вода течёт через всю строку).
        assert!(ws[2 * cols], "исток в водосборе");
    }
}
