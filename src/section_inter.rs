// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 4.04–4.09 Пересечения и проекции на разрез, порт `section_core.py`.
//!
//! Кажущийся угол падения, цепочки сечений TIN, стенки пластов
//! четырёхугольниками, уклон по азимуту.

use std::collections::HashMap;

/// Кажущийся угол падения на вертикальном разрезе.
/// tg(видимый) = tg(истинный) · cos(азимут_падения - азимут_разреза).
/// Возвращает (dz_dd, app_deg): уклон следа и видимый угол.
pub fn apparent_dip(dip_deg: f64, dip_az_deg: f64, section_az_deg: f64) -> (f64, f64) {
    let d = dip_deg;
    if d <= 0.0 {
        return (0.0, 0.0);
    }
    if d >= 90.0 {
        return (f64::INFINITY, 90.0);
    }
    let k = (dip_az_deg - section_az_deg).to_radians().cos();
    let dz_dd = -d.to_radians().tan() * k;
    (dz_dd, dz_dd.atan().abs().to_degrees())
}

/// Азимут звена ломаной в точке на расстоянии dist от начала.
pub fn azimuth_at_distance(vertices: &[(f64, f64)], dist: f64) -> f64 {
    let mut acc = 0.0;
    for i in 1..vertices.len() {
        let dx = vertices[i].0 - vertices[i - 1].0;
        let dy = vertices[i].1 - vertices[i - 1].1;
        let len = (dx * dx + dy * dy).sqrt();
        if acc + len >= dist {
            let az = dy.atan2(dx).to_degrees();
            return (az + 360.0) % 360.0;
        }
        acc += len;
    }
    // Последнее звено.
    let n = vertices.len();
    if n >= 2 {
        let dx = vertices[n - 1].0 - vertices[n - 2].0;
        let dy = vertices[n - 1].1 - vertices[n - 2].1;
        let az = dy.atan2(dx).to_degrees();
        (az + 360.0) % 360.0
    } else {
        0.0
    }
}

/// Цепочка сечения: вершины, замкнутость, зазор.
#[derive(Debug, Clone)]
pub struct Chain {
    /// Вершины (d, z) в осях разреза.
    pub pts: Vec<(f64, f64)>,
    /// Сомкнулась.
    pub closed: bool,
    /// Расстояние между концами у незамкнутой.
    pub gap: f64,
}

/// Сборка отрезков сечения в цепочки (замкнутые кольца и открытые линии).
/// segs — отрезки (d1, z1, d2, z2) в осях разреза, вразнобой.
pub fn chain_segments(segs: &[(f64, f64, f64, f64)], tol: f64) -> Vec<Chain> {
    let tol = if tol <= 0.0 { 1e-9 } else { tol };
    let key = |x: f64, y: f64| -> (i64, i64) {
        ((x / tol).round() as i64, (y / tol).round() as i64)
    };

    // Узел → список (номер отрезка, ключ другого конца).
    let mut nodes: HashMap<(i64, i64), Vec<(usize, (i64, i64))>> = HashMap::new();
    let mut pts_of: HashMap<(i64, i64), (f64, f64)> = HashMap::new();
    let mut alive: Vec<bool> = Vec::new();
    let mut segments: Vec<((i64, i64), (i64, i64))> = Vec::new();

    for s in segs {
        let a = key(s.0, s.1);
        let b = key(s.2, s.3);
        if a == b {
            continue;
        }
        pts_of.entry(a).or_insert((s.0, s.1));
        pts_of.entry(b).or_insert((s.2, s.3));
        let idx = alive.len();
        alive.push(true);
        segments.push((a, b));
        nodes.entry(a).or_default().push((idx, b));
        nodes.entry(b).or_default().push((idx, a));
    }

    // Идёт от узла, пока есть неиспользованные отрезки.
    let walk = |start: &(i64, i64),
                alive: &mut Vec<bool>,
                nodes: &HashMap<(i64, i64), Vec<(usize, (i64, i64))>>|
     -> Vec<(i64, i64)> {
        let mut chain = vec![*start];
        let mut cur = *start;
        loop {
            let mut nxt = None;
            if let Some(list) = nodes.get(&cur) {
                for &(idx, other) in list {
                    if alive[idx] {
                        alive[idx] = false;
                        nxt = Some(other);
                        break;
                    }
                }
            }
            match nxt {
                Some(n) => {
                    chain.push(n);
                    cur = n;
                }
                None => return chain,
            }
        }
    };

    let mut out = Vec::new();

    // Сперва от узлов нечётной степени (концы открытых цепочек).
    let odd: Vec<(i64, i64)> = nodes
        .iter()
        .filter(|(_, v)| v.len() % 2 == 1)
        .map(|(k, _)| *k)
        .collect();
    let all_keys: Vec<(i64, i64)> = nodes.keys().copied().collect();

    for start in odd.iter().chain(all_keys.iter()) {
        loop {
            let has_alive = nodes
                .get(start)
                .map(|v| v.iter().any(|&(idx, _)| alive[idx]))
                .unwrap_or(false);
            if !has_alive {
                break;
            }
            let chain = walk(start, &mut alive, &nodes);
            if chain.len() < 2 {
                continue;
            }
            let pts: Vec<(f64, f64)> =
                chain.iter().filter_map(|k| pts_of.get(k).copied()).collect();
            if pts.len() < 2 {
                continue;
            }
            let closed = chain[0] == *chain.last().unwrap() && chain.len() >= 4;
            let trimmed: Vec<(f64, f64)> = if closed {
                pts[..pts.len() - 1].to_vec()
            } else {
                pts
            };
            let gap = if closed {
                0.0
            } else {
                let (d0, z0) = trimmed[0];
                let (dn, zn) = *trimmed.last().unwrap();
                ((d0 - dn).powi(2) + (z0 - zn).powi(2)).sqrt()
            };
            out.push(Chain { pts: trimmed, closed, gap });
        }
    }
    out
}

/// Стенка пласта четырёхугольниками: по одному на звено трассы.
/// Возвращает список колец по пять точек (четыре + замыкающая).
/// Каждое в своей вертикальной плоскости (x, y, z).
pub fn bed_wall_quads(
    xs: &[f64],
    ys: &[f64],
    ztop: &[f64],
    zbot: &[f64],
    i0: usize,
    i1: usize,
) -> Vec<Vec<(f64, f64, f64)>> {
    let mut out = Vec::new();
    for i in i0..i1 {
        let j = i + 1;
        if j >= xs.len() {
            break;
        }
        let (x0, y0) = (xs[i], ys[i]);
        let (x1, y1) = (xs[j], ys[j]);
        let (zt0, zt1) = (ztop[i], ztop[j]);
        let (zb0, zb1) = (zbot[i], zbot[j]);
        if !zt0.is_finite() || !zt1.is_finite() || !zb0.is_finite() || !zb1.is_finite() {
            continue;
        }
        if x0 == x1 && y0 == y1 {
            continue;
        }
        let quad = [
            (x0, y0, zt0),
            (x1, y1, zt1),
            (x1, y1, zb1),
            (x0, y0, zb0),
            (x0, y0, zt0), // замыкающая
        ];
        out.push(quad.to_vec());
    }
    out
}

/// Профиль из набора линий: нижняя или верхняя огибающая.
/// parts — линии [(d, z), ...], keep_high — верхняя огибающая.
pub fn profile_from_lines(parts: &[Vec<(f64, f64)>], keep_high: bool) -> Vec<(f64, f64)> {
    // Собираем все точки, сортируем по d.
    let mut all: Vec<(f64, f64)> = parts.iter().flatten().copied().collect();
    all.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    // Убираем дубли по d, беря max или min.
    let mut out: Vec<(f64, f64)> = Vec::new();
    for &(d, z) in &all {
        if let Some(last) = out.last_mut() {
            if (last.0 - d).abs() < 1e-12 {
                if keep_high {
                    last.1 = last.1.max(z);
                } else {
                    last.1 = last.1.min(z);
                }
                continue;
            }
        }
        out.push((d, z));
    }
    out
}

/// Значение профиля в точке x (линейная интерполяция).
pub fn profile_y_at(prof: &[(f64, f64)], x: f64) -> f64 {
    if prof.is_empty() {
        return f64::NAN;
    }
    if x <= prof[0].0 {
        return prof[0].1;
    }
    if x >= prof[prof.len() - 1].0 {
        return prof[prof.len() - 1].1;
    }
    for i in 0..(prof.len() - 1) {
        if x >= prof[i].0 && x <= prof[i + 1].0 {
            let t = (x - prof[i].0) / (prof[i + 1].0 - prof[i].0);
            return prof[i].1 + t * (prof[i + 1].1 - prof[i].1);
        }
    }
    f64::NAN
}

/// Полоса между двумя профилями в интервале [x1, x2].
/// Отдаёт узловые точки (d, bot, top) в интервале.
pub fn band_nodes(
    bot: &[(f64, f64)],
    top: &[(f64, f64)],
    x1: f64,
    x2: f64,
) -> Vec<(f64, f64, f64)> {
    // Собираем все x из обоих профилей плюс границы.
    let mut xs: Vec<f64> = vec![x1, x2];
    for &(d, _) in bot.iter().chain(top.iter()) {
        if d > x1 && d < x2 {
            xs.push(d);
        }
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-12);

    xs.iter()
        .map(|&x| (x, profile_y_at(bot, x), profile_y_at(top, x)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Разрез вкрест простирания: видимый угол равен истинному.
    #[test]
    fn dip_along_section() {
        let (dz, app) = apparent_dip(30.0, 45.0, 45.0);
        assert!((app - 30.0).abs() < 1e-10, "app={}", app);
    }

    /// Разрез по простиранию: видимый угол ноль.
    #[test]
    fn dip_perpendicular() {
        let (dz, app) = apparent_dip(30.0, 45.0, 135.0);
        assert!(app.abs() < 1e-10, "app={}", app);
    }

    /// Сечения TIN: цепочка из двух треугольников.
    #[test]
    fn chain_two_triangles() {
        // Два отрезка образуют ломаную из трёх точек.
        let segs = [
            (0.0, 0.0, 5.0, 3.0),
            (5.0, 3.0, 10.0, 1.0),
        ];
        let chains = chain_segments(&segs, 1e-6);
        assert_eq!(chains.len(), 1);
        assert!(!chains[0].closed);
        assert_eq!(chains[0].pts.len(), 3);
    }

    /// Замкнутое кольцо из четырёх отрезков.
    #[test]
    fn chain_closed_ring() {
        let segs = [
            (0.0, 0.0, 4.0, 0.0),
            (4.0, 0.0, 4.0, 4.0),
            (4.0, 4.0, 0.0, 4.0),
            (0.0, 4.0, 0.0, 0.0),
        ];
        let chains = chain_segments(&segs, 1e-6);
        assert_eq!(chains.len(), 1);
        assert!(chains[0].closed, "gap={}", chains[0].gap);
        assert_eq!(chains[0].pts.len(), 4);
    }

    /// Стенка пласта: два звена → два четырёхугольника.
    #[test]
    fn wall_two_segments() {
        let xs = [0.0, 5.0, 10.0];
        let ys = [0.0, 0.0, 0.0];
        let ztop = [10.0, 10.0, 10.0];
        let zbot = [0.0, 0.0, 0.0];
        let quads = bed_wall_quads(&xs, &ys, &ztop, &zbot, 0, 2);
        assert_eq!(quads.len(), 2);
        // Каждый четырёхугольник: 5 точек (4 + замыкающая).
        for q in &quads {
            assert_eq!(q.len(), 5);
        }
    }
}
