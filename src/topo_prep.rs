// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 2.04 Подготовка рельефа: заполнение понижений и сглаживание.
//! Порт `topo_smooth.py` модуля QGIS.

/// Заполнение понижений (priority-flood).
/// Каждая ячейка приподнимается до минимального уровня, при котором
/// из неё есть путь стока к краю растра.
pub fn fill_depressions(
    z: &mut [f64],
    rows: usize,
    cols: usize,
    nodata: f64,
) {
    let n = rows * cols;
    if n == 0 {
        return;
    }

    // Приоритетная очередь (бинарная куча): (высота, индекс).
    let mut visited = vec![false; n];
    let mut heap: Vec<(f64, usize)> = Vec::with_capacity(n);

    // Край заливается первым.
    for r in 0..rows {
        for c in 0..cols {
            if r == 0 || r == rows - 1 || c == 0 || c == cols - 1 {
                let k = r * cols + c;
                if z[k] != nodata && z[k].is_finite() {
                    heap.push((z[k], k));
                    visited[k] = true;
                }
            }
        }
    }
    heap.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    const OFF: [(isize, isize); 8] = [
        (-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1),
    ];

    while let Some((zh, k)) = heap_pop(&mut heap) {
        let r = k / cols;
        let c = k % cols;
        for &(dr, dc) in OFF.iter() {
            let nr = r as isize + dr;
            let nc = c as isize + dc;
            if nr < 0 || nr >= rows as isize || nc < 0 || nc >= cols as isize {
                continue;
            }
            let nk = nr as usize * cols + nc as usize;
            if visited[nk] || z[nk] == nodata || !z[nk].is_finite() {
                continue;
            }
            visited[nk] = true;
            // Ячейка приподнимается до уровня соседа, если она ниже.
            if z[nk] < zh {
                z[nk] = zh;
            }
            heap_push(&mut heap, (z[nk], nk));
        }
    }
}

/// Сглаживание с ограничением: гаусс с обрезкой выбросов.
/// sigma — радиус в ячейках, max_change — предел изменения высоты.
pub fn smooth_clamped(
    z: &[f64],
    rows: usize,
    cols: usize,
    nodata: f64,
    sigma: f64,
    max_change: f64,
) -> Vec<f64> {
    let n = rows * cols;
    let mut out = z.to_vec();
    if sigma <= 0.0 || n == 0 {
        return out;
    }

    let radius = (sigma * 2.0).ceil() as usize;
    // Гауссовы веса.
    let mut gw: Vec<Vec<f64>> = Vec::with_capacity(2 * radius + 1);
    for dr in -(radius as isize)..=(radius as isize) {
        let mut row = Vec::with_capacity(2 * radius + 1);
        for dc in -(radius as isize)..=(radius as isize) {
            let d = ((dr * dr + dc * dc) as f64).sqrt();
            row.push((-(d * d) / (2.0 * sigma * sigma)).exp());
        }
        gw.push(row);
    }

    for r in 0..rows {
        for c in 0..cols {
            let k = r * cols + c;
            if z[k] == nodata || !z[k].is_finite() {
                continue;
            }
            let mut sum = 0.0;
            let mut wsum = 0.0;
            for dr in -(radius as isize)..=(radius as isize) {
                for dc in -(radius as isize)..=(radius as isize) {
                    let nr = r as isize + dr;
                    let nc = c as isize + dc;
                    if nr < 0 || nr >= rows as isize || nc < 0 || nc >= cols as isize {
                        continue;
                    }
                    let nk = nr as usize * cols + nc as usize;
                    if z[nk] == nodata || !z[nk].is_finite() {
                        continue;
                    }
                    let w = gw[(dr + radius as isize) as usize][(dc + radius as isize) as usize];
                    sum += w * z[nk];
                    wsum += w;
                }
            }
            if wsum > 0.0 {
                let smooth = sum / wsum;
                let change = smooth - z[k];
                // Ограничение: изменение не больше max_change.
                let clamped = if change.abs() > max_change {
                    z[k] + max_change * change.signum()
                } else {
                    smooth
                };
                out[k] = clamped;
            }
        }
    }
    out
}

/// Извлечение минимального элемента из кучи.
fn heap_pop(heap: &mut Vec<(f64, usize)>) -> Option<(f64, usize)> {
    if heap.is_empty() {
        return None;
    }
    let top = heap[0];
    let last = heap.pop().unwrap();
    if !heap.is_empty() {
        heap[0] = last;
        sift_down(heap, 0);
    }
    Some(top)
}

/// Добавление элемента в кучу.
fn heap_push(heap: &mut Vec<(f64, usize)>, item: (f64, usize)) {
    heap.push(item);
    let mut i = heap.len() - 1;
    while i > 0 {
        let parent = (i - 1) / 2;
        if heap[i].0 <= heap[parent].0 {
            break;
        }
        heap.swap(i, parent);
        i = parent;
    }
}

/// Просеивание вниз.
fn sift_down(heap: &mut [(f64, usize)], start: usize) {
    let n = heap.len();
    let mut i = start;
    loop {
        let left = 2 * i + 1;
        let right = 2 * i + 2;
        let mut best = i;
        if left < n && heap[left].0 > heap[best].0 {
            best = left;
        }
        if right < n && heap[right].0 > heap[best].0 {
            best = right;
        }
        if best == i {
            break;
        }
        heap.swap(i, best);
        i = best;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Котловина заполняется до уровня стока.
    #[test]
    fn fill_pit() {
        let rows = 7;
        let cols = 7;
        let mut z = vec![10.0f64; rows * cols];
        // Котловина в центре.
        z[3 * cols + 3] = 0.0;
        fill_depressions(&mut z, rows, cols, -9999.0);
        // Центр приподнят до уровня соседей.
        assert!(
            z[3 * cols + 3] >= 9.0,
            "pit filled to {}",
            z[3 * cols + 3]
        );
    }

    /// Сглаживание: выброс уменьшается.
    #[test]
    fn smooth_outlier() {
        let rows = 5;
        let cols = 5;
        let z = vec![10.0f64; rows * cols];
        // Один выброс.
        let mut z2 = z.clone();
        z2[2 * cols + 2] = 100.0;
        let s = smooth_clamped(&z2, rows, cols, -9999.0, 1.0, 5.0);
        // Выброс уменьшился, но не более чем на max_change.
        assert!(
            s[2 * cols + 2] < 100.0 && s[2 * cols + 2] > 90.0,
            "smoothed={}",
            s[2 * cols + 2]
        );
    }
}
