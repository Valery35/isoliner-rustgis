// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 2.03 Topo2Raster: гидрологически осмысленная ЦМР из векторных ограничений.
//! Порт `topo_t2r.py` модуля QGIS.
//!
//! Точки и изолинии — жёсткие узлы, тальвеги — монотонное падение,
//! обрывы — барьеры сглаживания, озёра — плоскости или переменный урез.

const DEFAULT_ITERATIONS: usize = 60;
const DEFAULT_MIN_DROP: f64 = 0.01;
const COARSEST: usize = 64;
const K_RING_NEAREST: usize = 8;

type StreamPair = (Vec<(f64, f64)>, Vec<usize>);
type LakeMask = (Vec<bool>, Option<f64>, Option<Vec<f64>>);

/// Озеро: кольца, опциональная высота, опциональные высоты вершин, признак плоскости.
#[derive(Debug, Clone, Default)]
pub struct Lake {
    pub rings: Vec<Vec<(f64, f64)>>,
    pub z: Option<f64>,
    pub ring_z: Option<Vec<Vec<f64>>>,
    pub as_plane: bool,
}

impl Lake {
    pub fn new(rings: Vec<Vec<(f64, f64)>>) -> Self {
        Self {
            rings,
            z: None,
            ring_z: None,
            as_plane: true,
        }
    }

    pub fn with_z(mut self, z: f64) -> Self {
        self.z = Some(z);
        self
    }

    pub fn with_ring_z(mut self, ring_z: Vec<Vec<f64>>) -> Self {
        self.ring_z = Some(ring_z);
        self
    }

    pub fn as_plane(mut self, plane: bool) -> Self {
        self.as_plane = plane;
        self
    }
}

#[inline]
fn fcmp(a: f64, b: f64) -> std::cmp::Ordering {
    match a.partial_cmp(&b) {
        Some(ord) => ord,
        None => std::cmp::Ordering::Equal,
    }
}

// -----------------------------------------------------------------------
// Геометрия на сетке
// -----------------------------------------------------------------------

fn world_to_cell(x: f64, y: f64, x0: f64, y_top: f64, cell: f64) -> (i64, i64) {
    let c = ((x - x0) / cell).floor() as i64;
    let r = ((y_top - y) / cell).floor() as i64;
    (r, c)
}

fn densify(xy: &[(f64, f64)], step: f64) -> Vec<(f64, f64)> {
    if xy.len() < 2 {
        return xy.to_vec();
    }
    let mut out = vec![xy[0]];
    for w in xy.windows(2) {
        let a = w[0];
        let b = w[1];
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let dist = dx.hypot(dy);
        let n = (dist / step).ceil().max(1.0) as usize;
        for k in 1..=n {
            let t = k as f64 / n as f64;
            out.push((a.0 + dx * t, a.1 + dy * t));
        }
    }
    out
}

fn polyline_cells(
    xy: &[(f64, f64)],
    x0: f64,
    y_top: f64,
    cell: f64,
    rows: usize,
    cols: usize,
) -> Vec<usize> {
    let dense = densify(xy, cell / 3.0);
    let mut out = Vec::with_capacity(dense.len());
    let mut prev: Option<usize> = None;
    for &(x, y) in &dense {
        let (r, c) = world_to_cell(x, y, x0, y_top, cell);
        if r < 0 || r >= rows as i64 || c < 0 || c >= cols as i64 {
            continue;
        }
        let k = r as usize * cols + c as usize;
        if prev != Some(k) {
            out.push(k);
            prev = Some(k);
        }
    }
    out
}

fn polygon_mask(
    rings: &[Vec<(f64, f64)>],
    x0: f64,
    y_top: f64,
    cell: f64,
    rows: usize,
    cols: usize,
) -> Vec<bool> {
    let mut mask = vec![false; rows * cols];
    let mut edges: Vec<(f64, f64, f64, f64)> = Vec::new();
    for ring in rings {
        if ring.len() < 3 {
            continue;
        }
        let closed = if ring.first() == ring.last() {
            ring.to_vec()
        } else {
            let mut v = ring.to_vec();
            v.push(ring[0]);
            v
        };
        for w in closed.windows(2) {
            let a = w[0];
            let b = w[1];
            if (a.1 - b.1).abs() > 1e-12 {
                edges.push((a.0, a.1, b.0, b.1));
            }
        }
    }
    if edges.is_empty() {
        return mask;
    }
    for r in 0..rows {
        let yc = y_top - (r as f64 + 0.5) * cell;
        let mut xs: Vec<f64> = Vec::new();
        for &(ax, ay, bx, by) in &edges {
            let lo = ay.min(by);
            let hi = ay.max(by);
            if yc >= lo && yc < hi {
                let t = (yc - ay) / (by - ay);
                xs.push(ax + t * (bx - ax));
            }
        }
        if xs.len() < 2 {
            continue;
        }
        xs.sort_by(|a, b| fcmp(*a, *b));
        for i in (0..xs.len() - 1).step_by(2) {
            let c0 = ((xs[i] - x0) / cell - 0.5).ceil() as i64;
            let c1 = ((xs[i + 1] - x0) / cell - 0.5).floor() as i64;
            let c0 = c0.max(0) as usize;
            let c1 = c1.min(cols as i64 - 1).max(0) as usize;
            if c1 >= c0 {
                for c in c0..=c1 {
                    mask[r * cols + c] = true;
                }
            }
        }
    }
    mask
}

// -----------------------------------------------------------------------
// Ограничения на уровне сетки
// -----------------------------------------------------------------------

fn pin_from_points(
    pts: &[(f64, f64, f64, f64)],
    x0: f64,
    y_top: f64,
    cell: f64,
    rows: usize,
    cols: usize,
) -> (Vec<bool>, Vec<f64>) {
    let n = rows * cols;
    let mut val = vec![0.0; n];
    let mut cnt = vec![0.0; n];
    for &(x, y, z, w) in pts {
        let (r, c) = world_to_cell(x, y, x0, y_top, cell);
        if r < 0 || r >= rows as i64 || c < 0 || c >= cols as i64 || !z.is_finite() {
            continue;
        }
        let k = r as usize * cols + c as usize;
        let ww = if w.is_finite() && w > 0.0 { w } else { 1.0 };
        val[k] += z * ww;
        cnt[k] += ww;
    }
    let mut pin = vec![false; n];
    for k in 0..n {
        if cnt[k] > 0.0 {
            pin[k] = true;
            val[k] /= cnt[k];
        }
    }
    (pin, val)
}

fn barrier_mask(
    breaklines: &[Vec<(f64, f64)>],
    x0: f64,
    y_top: f64,
    cell: f64,
    rows: usize,
    cols: usize,
) -> Vec<bool> {
    let mut mask = vec![false; rows * cols];
    for xy in breaklines {
        for &k in &polyline_cells(xy, x0, y_top, cell, rows, cols) {
            mask[k] = true;
        }
    }
    mask
}

fn erode4(mask: &[bool], rows: usize, cols: usize) -> Vec<bool> {
    let mut out = mask.to_vec();
    for r in 0..rows {
        for c in 0..cols {
            let k = r * cols + c;
            if !mask[k] {
                continue;
            }
            if r > 0 && !mask[(r - 1) * cols + c] {
                out[k] = false;
                continue;
            }
            if r + 1 < rows && !mask[(r + 1) * cols + c] {
                out[k] = false;
                continue;
            }
            if c > 0 && !mask[r * cols + c - 1] {
                out[k] = false;
                continue;
            }
            if c + 1 < cols && !mask[r * cols + c + 1] {
                out[k] = false;
                continue;
            }
        }
    }
    out
}

fn dilate(mask: &[bool], steps: usize, rows: usize, cols: usize) -> Vec<bool> {
    let mut m = mask.to_vec();
    for _ in 0..steps {
        let mut g = m.clone();
        for r in 0..rows {
            for c in 0..cols {
                let k = r * cols + c;
                if !m[k] {
                    continue;
                }
                if r > 0 {
                    g[(r - 1) * cols + c] = true;
                }
                if r + 1 < rows {
                    g[(r + 1) * cols + c] = true;
                }
                if c > 0 {
                    g[r * cols + c - 1] = true;
                }
                if c + 1 < cols {
                    g[r * cols + c + 1] = true;
                }
            }
        }
        m = g;
    }
    m
}

#[allow(dead_code)]
fn point_to_segment_dist2(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> (f64, f64) {
    let dx = bx - ax;
    let dy = by - ay;
    let den = dx * dx + dy * dy;
    if den <= 0.0 {
        let d2 = (px - ax).powi(2) + (py - ay).powi(2);
        return (d2, 0.0);
    }
    let s = ((px - ax) * dx + (py - ay) * dy) / den;
    let s = s.clamp(0.0, 1.0);
    let cx = ax + s * dx;
    let cy = ay + s * dy;
    let d2 = (px - cx).powi(2) + (py - cy).powi(2);
    (d2, s)
}

#[allow(dead_code)]
fn profile_ring_from_points(
    rings: &[Vec<(f64, f64)>],
    points: &[(f64, f64, f64)],
    tol: f64,
    min_pts: usize,
) -> Option<(Vec<Vec<f64>>, usize)> {
    if points.is_empty() {
        return None;
    }
    let mut best: Vec<f64> = points.iter().map(|_| f64::INFINITY).collect();
    for ring in rings {
        if ring.len() < 2 {
            continue;
        }
        let closed = if ring.first() == ring.last() {
            ring.to_vec()
        } else {
            let mut v = ring.to_vec();
            v.push(ring[0]);
            v
        };
        for w in closed.windows(2) {
            let a = w[0];
            let b = w[1];
            for (i, &(px, py, _)) in points.iter().enumerate() {
                let (d2, _) = point_to_segment_dist2(px, py, a.0, a.1, b.0, b.1);
                if d2 < best[i] {
                    best[i] = d2;
                }
            }
        }
    }
    let tol2 = tol * tol;
    let mut selected: Vec<(f64, f64, f64)> = Vec::new();
    for (i, &p) in points.iter().enumerate() {
        if best[i] <= tol2 {
            selected.push(p);
        }
    }
    if selected.len() < min_pts {
        return Some((Vec::new(), selected.len()));
    }
    let mut out: Vec<Vec<f64>> = Vec::new();
    for ring in rings {
        let mut rz: Vec<f64> = Vec::with_capacity(ring.len());
        for &(rx, ry) in ring {
            let k = K_RING_NEAREST.min(selected.len());
            let mut dists: Vec<(f64, f64)> = selected
                .iter()
                .map(|&(sx, sy, sz)| {
                    let d2 = (rx - sx).powi(2) + (ry - sy).powi(2);
                    let d2 = d2.max(1e-6);
                    (d2, sz)
                })
                .collect();
            dists.select_nth_unstable_by(k - 1, |a, b| fcmp(a.0, b.0));
            let mut wsum = 0.0;
            let mut num = 0.0;
            for &(d2, sz) in dists.iter().take(k) {
                let w = 1.0 / d2;
                wsum += w;
                num += w * sz;
            }
            rz.push(if wsum > 0.0 { num / wsum } else { f64::NAN });
        }
        out.push(rz);
    }
    Some((out, selected.len()))
}

fn interp_ring_surface(
    rings: &[Vec<(f64, f64)>],
    ring_z: &[Vec<f64>],
    mask: &[bool],
    x0: f64,
    y_top: f64,
    cell: f64,
    rows: usize,
    cols: usize,
) -> Option<Vec<f64>> {
    let mut pts: Vec<(f64, f64)> = Vec::new();
    let mut vals: Vec<f64> = Vec::new();
    for (ring, zr) in rings.iter().zip(ring_z.iter()) {
        for (i, &(x, y)) in ring.iter().enumerate() {
            if i < zr.len() && zr[i].is_finite() {
                pts.push((x, y));
                vals.push(zr[i]);
            }
        }
    }
    if pts.is_empty() {
        return None;
    }
    let npts = pts.len();
    let k = K_RING_NEAREST.min(npts);
    let mut surf = vec![0.0; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            let kidx = r * cols + c;
            if !mask[kidx] {
                continue;
            }
            let cx = x0 + (c as f64 + 0.5) * cell;
            let cy = y_top - (r as f64 + 0.5) * cell;
            let mut dists: Vec<(f64, f64)> = pts
                .iter()
                .zip(vals.iter())
                .map(|(&(px, py), &vz)| {
                    let d2 = (cx - px).powi(2) + (cy - py).powi(2);
                    let d2 = d2.max(1e-6);
                    (d2, vz)
                })
                .collect();
            dists.select_nth_unstable_by(k - 1, |a, b| fcmp(a.0, b.0));
            let mut wsum = 0.0;
            let mut num = 0.0;
            for &(d2, vz) in dists.iter().take(k) {
                let w = 1.0 / d2;
                wsum += w;
                num += w * vz;
            }
            surf[kidx] = if wsum > 0.0 { num / wsum } else { f64::NAN };
        }
    }
    Some(surf)
}

fn shore_ring(mask: &[bool], rows: usize, cols: usize) -> Vec<bool> {
    let mut grow = mask.to_vec();
    for r in 0..rows {
        for c in 0..cols {
            let k = r * cols + c;
            if !mask[k] {
                continue;
            }
            if r > 0 {
                grow[(r - 1) * cols + c] = true;
            }
            if r + 1 < rows {
                grow[(r + 1) * cols + c] = true;
            }
            if c > 0 {
                grow[r * cols + c - 1] = true;
            }
            if c + 1 < cols {
                grow[r * cols + c + 1] = true;
            }
        }
    }
    grow.iter()
        .zip(mask.iter())
        .map(|(&g, &m)| g && !m)
        .collect()
}

fn mask_edge(m: &[bool], rows: usize, cols: usize) -> Vec<bool> {
    let mut e = vec![false; rows * cols];
    if !m.iter().any(|&x| x) {
        return e;
    }
    for r in 0..rows {
        for c in 0..cols {
            let k = r * cols + c;
            if !m[k] {
                continue;
            }
            if r == 0 || r == rows - 1 || c == 0 || c == cols - 1 {
                e[k] = true;
                continue;
            }
            if !m[(r - 1) * cols + c]
                || !m[(r + 1) * cols + c]
                || !m[r * cols + c - 1]
                || !m[r * cols + c + 1]
            {
                e[k] = true;
            }
        }
    }
    e
}

// -----------------------------------------------------------------------
// Сглаживание
// -----------------------------------------------------------------------

fn relax(
    z: &[f64],
    pin_mask: &[bool],
    pin_val: &[f64],
    barrier: &[bool],
    iters: usize,
    streams_flat: &[Vec<usize>],
    min_drop: f64,
    stream_bnds: &Option<Vec<(Vec<f64>, Vec<f64>)>>,
    rows: usize,
    cols: usize,
) -> Vec<f64> {
    let mut z = z.to_vec();
    let dirs: [(isize, isize); 4] = [(0, 1), (0, -1), (1, 0), (-1, 0)];
    for it in 0..iters {
        let mut acc = vec![0.0; rows * cols];
        let mut cnt = vec![0.0; rows * cols];
        for (dr, dc) in dirs {
            let src_r_start = dr.max(0) as usize;
            let src_r_end = (rows as isize + dr.min(0)) as usize;
            let dst_r_start = (-dr).max(0) as usize;
            let _dst_r_end = (rows as isize + (-dr).min(0)) as usize;
            let src_c_start = dc.max(0) as usize;
            let src_c_end = (cols as isize + dc.min(0)) as usize;
            let dst_c_start = (-dc).max(0) as usize;
            let _dst_c_end = (cols as isize + (-dc).min(0)) as usize;
            for sr in src_r_start..src_r_end {
                for sc in src_c_start..src_c_end {
                    let src_idx = sr * cols + sc;
                    if barrier[src_idx] {
                        continue;
                    }
                    let dr_off = sr - src_r_start;
                    let dc_off = sc - src_c_start;
                    let dst_idx = (dst_r_start + dr_off) * cols + (dst_c_start + dc_off);
                    acc[dst_idx] += z[src_idx];
                    cnt[dst_idx] += 1.0;
                }
            }
        }
        for k in 0..rows * cols {
            if cnt[k] > 0.0 {
                z[k] = acc[k] / cnt[k];
            }
        }
        for k in 0..rows * cols {
            if pin_mask[k] {
                z[k] = pin_val[k];
            }
        }
        if !streams_flat.is_empty() && (it % 5 == 4 || it == iters - 1) {
            enforce_streams(&mut z, streams_flat, min_drop, stream_bnds);
            for k in 0..rows * cols {
                if pin_mask[k] {
                    z[k] = pin_val[k];
                }
            }
        }
    }
    z
}

fn enforce_streams(
    z: &mut [f64],
    streams_flat: &[Vec<usize>],
    min_drop: f64,
    bounds: &Option<Vec<(Vec<f64>, Vec<f64>)>>,
) {
    for (k, chain) in streams_flat.iter().enumerate() {
        if chain.len() < 2 {
            continue;
        }
        let n = chain.len();
        let mut v: Vec<f64> = chain.iter().map(|&i| z[i]).collect();
        let step: Vec<f64> = (0..n).map(|i| i as f64 * min_drop).collect();
        let mut acc = f64::INFINITY;
        for i in 0..n {
            let s = v[i] + step[i];
            if s < acc {
                acc = s;
            }
            v[i] = acc - step[i];
        }
        for i in 0..n {
            if v[i] > z[chain[i]] {
                v[i] = z[chain[i]];
            }
        }
        if let Some(bnds) = bounds && k < bnds.len() {
            let (cap, floor) = &bnds[k];
            for i in 0..n {
                if i < cap.len() && v[i] > cap[i] {
                    v[i] = cap[i];
                }
                if i < floor.len() && v[i] < floor[i] {
                    v[i] = floor[i];
                }
            }
        }
        for i in 0..n {
            z[chain[i]] = v[i];
        }
    }
}

fn project_depth_points(
    axis_xy: &[(f64, f64)],
    points: &[(f64, f64, f64, f64)],
    tol: f64,
) -> (Vec<(f64, f64)>, usize) {
    if points.is_empty() || axis_xy.len() < 2 || tol <= 0.0 {
        return (Vec::new(), 0);
    }
    let mut cum = vec![0.0; axis_xy.len()];
    for i in 1..axis_xy.len() {
        let dx = axis_xy[i].0 - axis_xy[i - 1].0;
        let dy = axis_xy[i].1 - axis_xy[i - 1].1;
        cum[i] = cum[i - 1] + dx.hypot(dy);
    }
    let total = cum.last().copied().unwrap_or(0.0);
    if total == 0.0 {
        return (Vec::new(), 0);
    }
    let mut best_d: Vec<f64> = points.iter().map(|_| f64::INFINITY).collect();
    let mut best_s: Vec<f64> = vec![0.0; points.len()];
    for k in 0..axis_xy.len() - 1 {
        let ax = axis_xy[k].0;
        let ay = axis_xy[k].1;
        let bx = axis_xy[k + 1].0;
        let by = axis_xy[k + 1].1;
        let dx = bx - ax;
        let dy = by - ay;
        let den = dx * dx + dy * dy;
        if den <= 0.0 {
            continue;
        }
        let seg_len = cum[k + 1] - cum[k];
        for (i, &(px, py, _, _)) in points.iter().enumerate() {
            let s = (((px - ax) * dx + (py - ay) * dy) / den).clamp(0.0, 1.0);
            let d = ((px - (ax + s * dx)).powi(2) + (py - (ay + s * dy)).powi(2)).sqrt();
            if d < best_d[i] {
                best_d[i] = d;
                best_s[i] = cum[k] + s * seg_len;
            }
        }
    }
    let mut map: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
    let mut used = 0;
    for (i, &(_, _, z, _)) in points.iter().enumerate() {
        if best_d[i] <= tol {
            used += 1;
            let key = (best_s[i] * 1_000_000.0).round() as i64;
            let entry = map.entry(key).or_insert(z);
            if z < *entry {
                *entry = z;
            }
        }
    }
    let mut out: Vec<(f64, f64)> = map
        .into_iter()
        .map(|(k, z)| (k as f64 / 1_000_000.0, z))
        .collect();
    out.sort_by(|a, b| fcmp(a.0, b.0));
    (out, used)
}

#[allow(dead_code)]
fn against_the_fall(anchors: &[(f64, f64)]) -> (usize, f64) {
    let mut ups = 0;
    let mut worst = 0.0;
    for w in anchors.windows(2) {
        let z0 = w[0].1;
        let z1 = w[1].1;
        if z1 > z0 {
            ups += 1;
            let d = z1 - z0;
            if d > worst {
                worst = d;
            }
        }
    }
    (ups, worst)
}

fn stream_bounds(
    chain: &[usize],
    pin_mask: &[bool],
    pin_val: &[f64],
    min_drop: f64,
    extra: &[(f64, f64)],
) -> (Vec<f64>, Vec<f64>) {
    let n = chain.len();
    let step: Vec<f64> = (0..n).map(|i| i as f64 * min_drop).collect();
    let mut pinned: Vec<bool> = chain.iter().map(|&k| pin_mask[k]).collect();
    let mut vals: Vec<f64> = chain.iter().map(|&k| pin_val[k]).collect();
    if !extra.is_empty() {
        let last = if n > 0 { n - 1 } else { 0 };
        let smax = extra.iter().map(|&(s, _)| s).fold(0.0, |a: f64, b: f64| a.max(b));
        let scale = if smax > 0.0 { last as f64 / smax } else { 0.0 };
        for &(s, z) in extra {
            let mut i = (s * scale).round() as isize;
            if i < 0 {
                i = 0;
            }
            if i >= n as isize {
                i = n as isize - 1;
            }
            let ui = i as usize;
            if pinned[ui] {
                if z < vals[ui] {
                    vals[ui] = z;
                }
            } else {
                pinned[ui] = true;
                vals[ui] = z;
            }
        }
    }
    let mut cap = vec![f64::INFINITY; n];
    let mut floor = vec![f64::NEG_INFINITY; n];
    if pinned.iter().any(|&p| p) {
        let mut run = f64::INFINITY;
        for i in 0..n {
            if pinned[i] {
                run = vals[i] + step[i];
            }
            cap[i] = run;
        }
        for i in 0..n {
            cap[i] -= step[i];
        }
        let idx: Vec<usize> = pinned
            .iter()
            .enumerate()
            .filter_map(|(i, &p)| if p { Some(i) } else { None })
            .collect();
        for w in idx.windows(2) {
            let a = w[0];
            let b = w[1];
            if b > a {
                let m = b - a;
                for j in 0..=m {
                    let t = j as f64 / m as f64;
                    let v = vals[a] * (1.0 - t) + vals[b] * t;
                    floor[a + j] = v;
                    cap[a + j] = v;
                }
            }
        }
        let first = idx[0];
        for i in 0..first {
            floor[i] = vals[first] + (step[first] - step[i]);
        }
        for &i in &idx {
            floor[i] = vals[i];
        }
    }
    (cap, floor)
}

fn polish(
    z: &[f64],
    pin_mask: &[bool],
    pin_val: &[f64],
    barrier: &[bool],
    iters: usize,
    streams_flat: &[Vec<usize>],
    min_drop: f64,
    stream_bnds: &Option<Vec<(Vec<f64>, Vec<f64>)>>,
    rows: usize,
    cols: usize,
) -> Vec<f64> {
    if iters == 0 {
        return z.to_vec();
    }
    let mut z = z.to_vec();
    let frozen = if barrier.iter().any(|&b| b) {
        Some(dilate(barrier, 2, rows, cols))
    } else {
        None
    };
    let omega = 0.5;
    for it in 0..iters {
        let mut znew = vec![0.0; rows * cols];
        for r in 0..rows {
            for c in 0..cols {
                let ri = r as isize;
                let ci = c as isize;
                let at = |rr: isize, cc: isize| -> f64 {
                    let rr = rr.clamp(0, rows as isize - 1) as usize;
                    let cc = cc.clamp(0, cols as isize - 1) as usize;
                    z[rr * cols + cc]
                };
                let v = (8.0
                    * (at(ri - 1, ci)
                        + at(ri + 1, ci)
                        + at(ri, ci - 1)
                        + at(ri, ci + 1))
                    - 2.0
                        * (at(ri - 1, ci - 1)
                            + at(ri - 1, ci + 1)
                            + at(ri + 1, ci - 1)
                            + at(ri + 1, ci + 1))
                    - (at(ri - 2, ci)
                        + at(ri + 2, ci)
                        + at(ri, ci - 2)
                        + at(ri, ci + 2)))
                    / 20.0;
                znew[r * cols + c] = v;
            }
        }
        for k in 0..rows * cols {
            let upd = (1.0 - omega) * z[k] + omega * znew[k];
            z[k] = match frozen {
                Some(ref f) => {
                    if f[k] {
                        z[k]
                    } else {
                        upd
                    }
                }
                None => upd,
            };
        }
        for k in 0..rows * cols {
            if pin_mask[k] {
                z[k] = pin_val[k];
            }
        }
        if !streams_flat.is_empty() && (it % 10 == 9 || it == iters - 1) {
            enforce_streams(&mut z, streams_flat, min_drop, stream_bnds);
            for k in 0..rows * cols {
                if pin_mask[k] {
                    z[k] = pin_val[k];
                }
            }
        }
    }
    z
}

// -----------------------------------------------------------------------
// Заполнение и растяжение
// -----------------------------------------------------------------------

fn initial_fill(
    z: &[f64],
    known: &[bool],
    rows: usize,
    cols: usize,
) -> Result<Vec<f64>, String> {
    let mut z = z.to_vec();
    let mut filled = known.to_vec();
    if !filled.iter().any(|&k| k) {
        return Err("нет ни одного узла с высотой".to_string());
    }
    for k in 0..rows * cols {
        if !filled[k] {
            z[k] = 0.0;
        }
    }
    let dirs: [(isize, isize); 4] = [(0, 1), (0, -1), (1, 0), (-1, 0)];
    loop {
        let mut acc = vec![0.0; rows * cols];
        let mut cnt = vec![0.0; rows * cols];
        for (dr, dc) in dirs {
            let src_r_start = dr.max(0) as usize;
            let src_r_end = (rows as isize + dr.min(0)) as usize;
            let dst_r_start = (-dr).max(0) as usize;
            let _dst_r_end = (rows as isize + (-dr).min(0)) as usize;
            let src_c_start = dc.max(0) as usize;
            let src_c_end = (cols as isize + dc.min(0)) as usize;
            let dst_c_start = (-dc).max(0) as usize;
            let _dst_c_end = (cols as isize + (-dc).min(0)) as usize;
            for sr in src_r_start..src_r_end {
                for sc in src_c_start..src_c_end {
                    let src_idx = sr * cols + sc;
                    if !filled[src_idx] {
                        continue;
                    }
                    let dr_off = sr - src_r_start;
                    let dc_off = sc - src_c_start;
                    let dst_idx = (dst_r_start + dr_off) * cols + (dst_c_start + dc_off);
                    acc[dst_idx] += z[src_idx];
                    cnt[dst_idx] += 1.0;
                }
            }
        }
        let mut any_new = false;
        for k in 0..rows * cols {
            if !filled[k] && cnt[k] > 0.0 {
                z[k] = acc[k] / cnt[k];
                filled[k] = true;
                any_new = true;
            }
        }
        if !any_new {
            if !filled.iter().all(|&f| f) {
                let mut sum = 0.0;
                let mut count = 0.0;
                for k in 0..rows * cols {
                    if filled[k] && z[k].is_finite() {
                        sum += z[k];
                        count += 1.0;
                    }
                }
                let mean = if count > 0.0 { sum / count } else { 0.0 };
                for k in 0..rows * cols {
                    if !filled[k] {
                        z[k] = mean;
                    }
                }
            }
            break;
        }
        if filled.iter().all(|&f| f) {
            break;
        }
    }
    Ok(z)
}

fn upsample(
    z: &[f64],
    old_rows: usize,
    old_cols: usize,
    new_rows: usize,
    new_cols: usize,
) -> Vec<f64> {
    let mut out = vec![0.0; new_rows * new_cols];
    if old_rows == 0 || old_cols == 0 || new_rows == 0 || new_cols == 0 {
        return out;
    }
    let ry_max = (old_rows as f64 - 1.0).max(1.0);
    let rx_max = (old_cols as f64 - 1.0).max(1.0);
    let ny_denom = (new_rows as f64 - 1.0).max(1.0);
    let nx_denom = (new_cols as f64 - 1.0).max(1.0);
    for r in 0..new_rows {
        let ry = r as f64 * ry_max / ny_denom;
        let r0 = ry.floor() as usize;
        let r1 = (r0 + 1).min(old_rows - 1);
        let fr = ry - r0 as f64;
        for c in 0..new_cols {
            let rx = c as f64 * rx_max / nx_denom;
            let c0 = rx.floor() as usize;
            let c1 = (c0 + 1).min(old_cols - 1);
            let fc = rx - c0 as f64;
            let top = z[r0 * old_cols + c0] * (1.0 - fc) + z[r0 * old_cols + c1] * fc;
            let bot = z[r1 * old_cols + c0] * (1.0 - fc) + z[r1 * old_cols + c1] * fc;
            out[r * new_cols + c] = top * (1.0 - fr) + bot * fr;
        }
    }
    out
}

// -----------------------------------------------------------------------
// Основной вход
// -----------------------------------------------------------------------

pub fn topo2raster(
    points: &[(f64, f64, f64, f64)],
    streams: &[Vec<(f64, f64)>],
    breaklines: &[Vec<(f64, f64)>],
    lakes: &[Lake],
    extent: (f64, f64, f64, f64),
    cell: f64,
    iterations: Option<usize>,
    min_drop: Option<f64>,
    depth_tol: Option<f64>,
) -> Result<(Vec<f64>, usize, usize, f64, f64), String> {
    let (xmin, ymin, xmax, ymax) = extent;
    if !(xmax > xmin && ymax > ymin) {
        return Err("пустой охват".to_string());
    }
    if cell <= 0.0 || !cell.is_finite() {
        return Err("размер ячейки должен быть положительным".to_string());
    }
    let nx = ((xmax - xmin) / cell).ceil().max(4.0) as usize;
    let ny = ((ymax - ymin) / cell).ceil().max(4.0) as usize;
    let x0 = xmin;
    let y_top = ymax;
    if points.is_empty() {
        return Err("нужен хотя бы один узел с высотой".to_string());
    }

    let iterations = iterations.unwrap_or(DEFAULT_ITERATIONS);
    let min_drop = min_drop.unwrap_or(DEFAULT_MIN_DROP);
    let depth_tol = depth_tol.unwrap_or(0.0);

    let mut factors = vec![1usize];
    while (ny.max(nx) / factors[factors.len() - 1]) > COARSEST {
        let next = factors[factors.len() - 1] * 2;
        factors.push(next);
    }
    factors.reverse();

    let mut z_opt: Option<Vec<f64>> = None;
    let mut prev_rows = 0usize;
    let mut prev_cols = 0usize;
    let n_levels = factors.len();

    for (li, &f) in factors.iter().enumerate() {
        let lcell = cell * f as f64;
        let lrows = (ny as f64 / f as f64).ceil().max(4.0) as usize;
        let lcols = (nx as f64 / f as f64).ceil().max(4.0) as usize;

        let (mut pin, mut pval) = pin_from_points(points, x0, y_top, lcell, lrows, lcols);
        let mut barrier = barrier_mask(breaklines, x0, y_top, lcell, lrows, lcols);
        barrier = erode4(&barrier, lrows, lcols);

        let mut pairs: Vec<StreamPair> = Vec::new();
        for xy in streams {
            let s = polyline_cells(xy, x0, y_top, lcell, lrows, lcols);
            if s.len() > 1 {
                pairs.push((xy.clone(), s));
            }
        }
        let streams_flat: Vec<Vec<usize>> = pairs.iter().map(|(_, s)| s.clone()).collect();
        let stream_extra: Vec<Vec<(f64, f64)>> = pairs
            .iter()
            .map(|(xy, _)| {
                if depth_tol > 0.0 {
                    project_depth_points(xy, points, depth_tol).0
                } else {
                    Vec::new()
                }
            })
            .collect();

        let mut lake_masks: Vec<LakeMask> = Vec::new();
        for lake in lakes {
            let m = polygon_mask(&lake.rings, x0, y_top, lcell, lrows, lcols);
            if m.iter().any(|&x| x) {
                let surf = if let Some(rz) = lake.ring_z.as_ref() {
                    interp_ring_surface(&lake.rings, rz, &m, x0, y_top, lcell, lrows, lcols)
                } else {
                    None
                };
                let mm = if lake.as_plane {
                    m
                } else {
                    mask_edge(&m, lrows, lcols)
                };
                lake_masks.push((mm, lake.z, surf));
            }
        }

        let mut z = match z_opt {
            None => initial_fill(&pval, &pin, lrows, lcols)?,
            Some(prev) => {
                let mut up = upsample(&prev, prev_rows, prev_cols, lrows, lcols);
                for k in 0..lrows * lcols {
                    if pin[k] {
                        up[k] = pval[k];
                    }
                }
                up
            }
        };

        for (m, lz, surf) in &lake_masks {
            if let Some(s) = surf {
                for k in 0..lrows * lcols {
                    if m[k] {
                        pin[k] = true;
                        pval[k] = s[k];
                        z[k] = s[k];
                    }
                }
            } else if let Some(level) = lz {
                for k in 0..lrows * lcols {
                    if m[k] {
                        pin[k] = true;
                        pval[k] = *level;
                        z[k] = *level;
                    }
                }
            }
        }

        let iters = if li < n_levels - 1 {
            (iterations / (li + 1)).max(10)
        } else {
            iterations
        };

        let stream_bnds: Option<Vec<(Vec<f64>, Vec<f64>)>> = if streams_flat.is_empty() {
            None
        } else {
            Some(
                streams_flat
                    .iter()
                    .enumerate()
                    .map(|(i, ch)| stream_bounds(ch, &pin, &pval, min_drop, &stream_extra[i]))
                    .collect(),
            )
        };

        z = relax(
            &z,
            &pin,
            &pval,
            &barrier,
            (iters / 2).max(5),
            &streams_flat,
            min_drop,
            &stream_bnds,
            lrows,
            lcols,
        );

        for (m, lz, surf) in &lake_masks {
            if surf.is_none() && lz.is_none() {
                let ring = shore_ring(m, lrows, lcols);
                let level = if ring.iter().any(|&x| x) {
                    let mut best = f64::INFINITY;
                    for k in 0..lrows * lcols {
                        if ring[k] && z[k].is_finite() {
                            best = best.min(z[k]);
                        }
                    }
                    best
                } else {
                    let mut best = f64::INFINITY;
                    for k in 0..lrows * lcols {
                        if m[k] && z[k].is_finite() {
                            best = best.min(z[k]);
                        }
                    }
                    best
                };
                for k in 0..lrows * lcols {
                    if m[k] {
                        pin[k] = true;
                        pval[k] = level;
                        z[k] = level;
                    }
                }
            }
        }

        let stream_bnds: Option<Vec<(Vec<f64>, Vec<f64>)>> = if streams_flat.is_empty() {
            None
        } else {
            Some(
                streams_flat
                    .iter()
                    .enumerate()
                    .map(|(i, ch)| stream_bounds(ch, &pin, &pval, min_drop, &stream_extra[i]))
                    .collect(),
            )
        };

        z = relax(
            &z,
            &pin,
            &pval,
            &barrier,
            iters - (iters / 2).max(5) + 1,
            &streams_flat,
            min_drop,
            &stream_bnds,
            lrows,
            lcols,
        );
        z = polish(
            &z,
            &pin,
            &pval,
            &barrier,
            2 * iters,
            &streams_flat,
            min_drop,
            &stream_bnds,
            lrows,
            lcols,
        );

        z_opt = Some(z);
        prev_rows = lrows;
        prev_cols = lcols;
    }

    let z = match z_opt {
        Some(z) => z,
        None => return Err("нет ни одного уровня сетки".to_string()),
    };
    let z = if prev_rows != ny || prev_cols != nx {
        upsample(&z, prev_rows, prev_cols, ny, nx)
    } else {
        z
    };
    Ok((z, ny, nx, x0, y_top))
}

pub fn monotone_down(z: &[f64], min_drop: f64) -> Vec<f64> {
    if z.len() < 2 {
        return z.to_vec();
    }
    let step: Vec<f64> = (0..z.len()).map(|i| i as f64 * min_drop).collect();
    let mut acc = f64::INFINITY;
    let mut out = Vec::with_capacity(z.len());
    for i in 0..z.len() {
        let s = z[i] + step[i];
        if s < acc {
            acc = s;
        }
        out.push(acc - step[i]);
    }
    out
}

pub fn count_conflicts(pts_a: &[(f64, f64, f64)], pts_b: &[(f64, f64, f64)], cell: f64, tol: f64) -> usize {
    if pts_a.is_empty() || pts_b.is_empty() {
        return 0;
    }
    let cell = if cell == 0.0 { 1.0 } else { cell };
    let mut key_a: std::collections::HashMap<(i64, i64), f64> = std::collections::HashMap::new();
    for &(x, y, z) in pts_a {
        let k = ((x / cell).floor() as i64, (y / cell).floor() as i64);
        key_a.insert(k, z);
    }
    let mut seen: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
    let mut n = 0;
    for &(x, y, z) in pts_b {
        let k = ((x / cell).floor() as i64, (y / cell).floor() as i64);
        if let Some(&za) = key_a.get(&k) && (za - z).abs() > tol && seen.insert(k) {
            n += 1;
        }
    }
    n
}

// -----------------------------------------------------------------------
// Тесты
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_from_four_corners() {
        // Ячейка 20 м, точки в центрах ячеек углов и центра, чтобы не
        // выпадать за правую/верхнюю границу сетки.
        let points = vec![
            (10.0, 90.0, 10.0, 1.0), // NW
            (90.0, 90.0, 10.0, 1.0), // NE
            (10.0, 10.0, 10.0, 1.0), // SW
            (90.0, 10.0, 10.0, 1.0), // SE
            (50.0, 50.0, 10.0, 1.0), // center
        ];
        let (z, rows, cols, x0, y_top) =
            topo2raster(&points, &[], &[], &[], (0.0, 0.0, 100.0, 100.0), 20.0, None, None, None)
                .unwrap();
        assert_eq!(rows, 5);
        assert_eq!(cols, 5);
        assert!((x0 - 0.0).abs() < 1e-9);
        assert!((y_top - 100.0).abs() < 1e-9);
        // Все пять узлов задают горизонтальную плоскость z=10.
        let center = z[2 * cols + 2];
        assert!((center - 10.0).abs() < 1.0, "center was {}", center);
        assert!((z[0] - 10.0).abs() < 1.0, "NW corner was {}", z[0]);
        assert!((z[cols - 1] - 10.0).abs() < 1.0, "NE corner was {}", z[cols - 1]);
        assert!(
            (z[(rows - 1) * cols] - 10.0).abs() < 1.0,
            "SW corner was {}",
            z[(rows - 1) * cols]
        );
        assert!(
            (z[rows * cols - 1] - 10.0).abs() < 1.0,
            "SE corner was {}",
            z[rows * cols - 1]
        );
    }

    #[test]
    fn stream_forces_monotonic_drop() {
        // Прямой тальвег с северо-запада на юго-восток.
        let stream = vec![vec![
            (10.0, 90.0),
            (30.0, 70.0),
            (50.0, 50.0),
            (70.0, 30.0),
            (90.0, 10.0),
        ]];
        let points = vec![
            (10.0, 90.0, 30.0, 1.0),
            (50.0, 50.0, 20.0, 1.0),
            (90.0, 10.0, 10.0, 1.0),
        ];
        let (z, rows, cols, x0, y_top) =
            topo2raster(&points, &stream, &[], &[], (0.0, 0.0, 100.0, 100.0), 5.0, None, None, None)
                .unwrap();
        // Проверяем монотонность вдоль линии, идущей от начала к концу.
        let path = &stream[0];
        let mut last = f64::INFINITY;
        for &(px, py) in path {
            let c = ((px - x0) / 5.0).floor() as usize;
            let r = ((y_top - py) / 5.0).floor() as usize;
            if r < rows && c < cols {
                let v = z[r * cols + c];
                assert!(v <= last + 1e-6, "stream not monotone: {} then {}", last, v);
                last = v;
            }
        }
    }

    #[test]
    fn lake_becomes_flat_at_level() {
        let lake = Lake::new(vec![vec![
            (40.0, 60.0),
            (60.0, 60.0),
            (60.0, 40.0),
            (40.0, 40.0),
            (40.0, 60.0),
        ]])
        .with_z(50.0);
        let points = vec![
            (0.0, 0.0, 0.0, 1.0),
            (100.0, 0.0, 0.0, 1.0),
            (0.0, 100.0, 100.0, 1.0),
            (100.0, 100.0, 100.0, 1.0),
        ];
        let (z, rows, cols, x0, y_top) =
            topo2raster(&points, &[], &[], &[lake], (0.0, 0.0, 100.0, 100.0), 5.0, None, None, None)
                .unwrap();
        // Внутренние ячейки озера (примерно 40-60) должны быть близки к 50.
        let mut sum = 0.0;
        let mut n = 0.0;
        for r in 0..rows {
            for c in 0..cols {
                let px = x0 + c as f64 * 5.0;
                let py = y_top - r as f64 * 5.0;
                if px > 42.0 && px < 58.0 && py > 42.0 && py < 58.0 {
                    sum += z[r * cols + c];
                    n += 1.0;
                }
            }
        }
        assert!(n > 0.0);
        let mean = sum / n;
        assert!((mean - 50.0).abs() < 1.0, "lake mean was {}", mean);
    }

    #[test]
    fn polygon_mask_sanity() {
        let rings = vec![vec![
            (2.0, 2.0),
            (8.0, 2.0),
            (8.0, 8.0),
            (2.0, 8.0),
            (2.0, 2.0),
        ]];
        let m = polygon_mask(&rings, 0.0, 10.0, 1.0, 10, 10);
        // Центр 5x5 должен быть внутри.
        assert!(m[5 * 10 + 5]);
        // Углы снаружи.
        assert!(!m[0]);
        assert!(!m[9 * 10 + 9]);
        // Граница: (2,5) внутри, (1,5) снаружи.
        assert!(m[5 * 10 + 2]);
        assert!(!m[5 * 10 + 1]);
    }

    #[test]
    fn monotone_down_makes_non_increasing() {
        let z = vec![10.0, 9.5, 9.8, 8.0, 8.1, 7.0];
        let m = monotone_down(&z, 0.1);
        for w in m.windows(2) {
            assert!(w[1] <= w[0] + 1e-9, "not non-increasing: {:?}", m);
        }
    }

    #[test]
    fn count_conflicts_basic() {
        let a = vec![(0.0, 0.0, 10.0)];
        let b = vec![(0.0, 0.0, 10.0)];
        assert_eq!(count_conflicts(&a, &b, 1.0, 0.05), 0);
        let c = vec![(0.0, 0.0, 12.0)];
        assert_eq!(count_conflicts(&a, &c, 1.0, 0.05), 1);
        let d = vec![(0.0, 0.0, 10.04)];
        assert_eq!(count_conflicts(&a, &d, 1.0, 0.05), 0);
        // Другая ячейка — конфликта нет независимо от Z.
        let e = vec![(2.0, 0.0, 999.0)];
        assert_eq!(count_conflicts(&a, &e, 1.0, 0.05), 0);
    }
}
