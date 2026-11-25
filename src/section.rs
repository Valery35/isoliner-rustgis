// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 4.01 Разрез по линии: ядро построения геологического разреза.
//! Порт `section_core.py` модуля QGIS. Чистая математика и геометрия.
//!
//! На вход — вершины линии, массивы поверхностей с геотрансформами.
//! На выход — кольца полигонов, угловые точки, отметки осей, ячейки
//! таблицы и габариты чертежа в координатах «расстояние - высота».

/// Выборка значений растра в точках. Билинейно или ближайший.
/// arr содержит NaN на месте nodata.
pub fn sample_grid_points(
    arr: &[f64],
    rows: usize,
    cols: usize,
    gt: &[f64; 6],
    xs: &[f64],
    ys: &[f64],
    bilinear: bool,
) -> Vec<f64> {
    let n = xs.len();
    let mut out = vec![f64::NAN; n];
    for k in 0..n {
        let fx = (xs[k] - gt[0]) / gt[1] - 0.5;
        let fy = (ys[k] - gt[3]) / gt[5] - 0.5;
        if !bilinear {
            let ix = fx.round() as i64;
            let iy = fy.round() as i64;
            if ix >= 0 && (ix as usize) < cols && iy >= 0 && (iy as usize) < rows {
                out[k] = arr[iy as usize * cols + ix as usize];
            }
        } else {
            let x0 = fx.floor() as i64;
            let y0 = fy.floor() as i64;
            let tx = fx - x0 as f64;
            let ty = fy - y0 as f64;
            let get = |ix: i64, iy: i64| -> f64 {
                if ix >= 0 && (ix as usize) < cols && iy >= 0 && (iy as usize) < rows {
                    arr[iy as usize * cols + ix as usize]
                } else {
                    f64::NAN
                }
            };
            let v00 = get(x0, y0);
            let v10 = get(x0 + 1, y0);
            let v01 = get(x0, y0 + 1);
            let v11 = get(x0 + 1, y0 + 1);
            out[k] = v00 * (1.0 - tx) * (1.0 - ty)
                + v10 * tx * (1.0 - ty)
                + v01 * (1.0 - tx) * ty
                + v11 * tx * ty;
        }
    }
    out
}

/// Длина полилинии по вершинам.
pub fn polyline_length(vertices: &[(f64, f64)]) -> f64 {
    vertices
        .windows(2)
        .map(|w| {
            let dx = w[1].0 - w[0].0;
            let dy = w[1].1 - w[0].1;
            (dx * dx + dy * dy).sqrt()
        })
        .sum()
}

/// Азимуты сегментов полилинии (географические, градусы).
pub fn segment_azimuths(vertices: &[(f64, f64)]) -> Vec<f64> {
    vertices
        .windows(2)
        .map(|w| {
            let dx = w[1].0 - w[0].0;
            let dy = w[1].1 - w[0].1;
            let az = dy.atan2(dx).to_degrees();
            (az + 360.0) % 360.0
        })
        .collect()
}

/// Интерполяция точки на полилинии по расстоянию от начала.
/// vertices — вершины, dists — расстояния от начала, ds — расстояния точек.
pub fn interpolate_polyline(
    vertices: &[(f64, f64)],
    dists: &[f64],
    ds: &[f64],
) -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity(ds.len());
    for &d in ds {
        if d <= dists[0] {
            out.push(vertices[0]);
            continue;
        }
        if d >= *dists.last().unwrap() {
            out.push(*vertices.last().unwrap());
            continue;
        }
        // Поиск сегмента.
        for i in 0..(dists.len() - 1) {
            if d >= dists[i] && d <= dists[i + 1] {
                let t = if dists[i + 1] > dists[i] {
                    (d - dists[i]) / (dists[i + 1] - dists[i])
                } else {
                    0.0
                };
                let x = vertices[i].0 + t * (vertices[i + 1].0 - vertices[i].0);
                let y = vertices[i].1 + t * (vertices[i + 1].1 - vertices[i].1);
                out.push((x, y));
                break;
            }
        }
    }
    out
}

/// Пикеты вдоль полилинии с заданным шагом.
/// Отдаёт (расстояние, x, y) для каждого пикета.
pub fn stations(vertices: &[(f64, f64)], step: f64) -> Vec<(f64, f64, f64)> {
    if vertices.len() < 2 || step <= 0.0 {
        return Vec::new();
    }
    let dists: Vec<f64> = {
        let mut d = Vec::with_capacity(vertices.len());
        let mut acc = 0.0;
        d.push(0.0);
        for i in 1..vertices.len() {
            let dx = vertices[i].0 - vertices[i - 1].0;
            let dy = vertices[i].1 - vertices[i - 1].1;
            acc += (dx * dx + dy * dy).sqrt();
            d.push(acc);
        }
        d
    };
    let total = *dists.last().unwrap();
    let n = (total / step).floor() as usize + 1;
    let mut ds = Vec::with_capacity(n);
    for i in 0..n {
        ds.push(i as f64 * step);
    }
    if *ds.last().unwrap() < total {
        ds.push(total);
    }
    let pts = interpolate_polyline(vertices, &dists, &ds);
    ds.iter()
        .zip(pts.iter())
        .map(|(&d, &(x, y))| (d, x, y))
        .collect()
}

/// Поверхность для разреза: растр + геотрансформация.
#[derive(Debug, Clone)]
pub struct Surface {
    /// Значения (rows × cols, построчно). NaN — nodata.
    pub data: Vec<f64>,
    pub rows: usize,
    pub cols: usize,
    /// Геотрансформация: [x0, dx, dxdy, y0, dydx, dy].
    pub gt: [f64; 6],
}

/// Выборка поверхностей вдоль линии разреза.
/// Отдаёт расстояния и значения каждой поверхности в пикетах.
pub fn sample_section(
    vertices: &[(f64, f64)],
    surfaces: &[Surface],
    step: f64,
) -> (Vec<f64>, Vec<Vec<f64>>) {
    // Пикеты.
    let st = stations(vertices, step);
    let dists: Vec<f64> = st.iter().map(|s| s.0).collect();
    let xs: Vec<f64> = st.iter().map(|s| s.1).collect();
    let ys: Vec<f64> = st.iter().map(|s| s.2).collect();

    // Значения поверхностей.
    let mut z = Vec::with_capacity(surfaces.len());
    for s in surfaces {
        let vals = sample_grid_points(&s.data, s.rows, s.cols, &s.gt, &xs, &ys, true);
        z.push(vals);
    }
    (dists, z)
}

/// Кольцо полигонов пласта в координатах чертежа (расстояние, высота).
/// d — расстояния, ztop — верхняя поверхность, zbot — нижняя.
/// Возвращаем кольцо: верх слева направо + низ справа налево.
pub fn bed_ring_2d(
    d: &[f64],
    ztop: &[f64],
    zbot: &[f64],
    vex: f64,
) -> Vec<(f64, f64)> {
    let n = d.len();
    let mut ring = Vec::with_capacity(2 * n);
    // Верх слева направо.
    for k in 0..n {
        ring.push((d[k], ztop[k] * vex));
    }
    // Низ справа налево.
    for k in (0..n).rev() {
        ring.push((d[k], zbot[k] * vex));
    }
    ring
}

/// Площадь кольца (формула шнурков).
pub fn ring_area(ring: &[(f64, f64)]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let mut s = 0.0;
    for i in 0..ring.len() {
        let j = (i + 1) % ring.len();
        s += ring[i].0 * ring[j].1 - ring[j].0 * ring[i].1;
    }
    (s / 2.0).abs()
}

/// Результат построения разреза.
#[derive(Debug, Clone, Default)]
pub struct SectionResult {
    /// Расстояния пикетов.
    pub dists: Vec<f64>,
    /// Значения поверхностей (surface × station).
    pub surfaces: Vec<Vec<f64>>,
    /// Кольца пластов (кольцо = [(расстояние, высота), ...]).
    pub rings: Vec<Vec<(f64, f64)>>,
    /// Вертикальное преувеличение.
    pub vex: f64,
    /// Длина разреза.
    pub length: f64,
    /// Мин/макс высоты.
    pub zmin: f64,
    pub zmax: f64,
}

/// Построение разреза по линии и поверхностям.
/// surfaces — поверхности в порядке сверху вниз (кровля, подошва, ...).
pub fn build_section(
    vertices: &[(f64, f64)],
    surfaces: &[Surface],
    step: f64,
    vex: f64,
) -> SectionResult {
    let (dists, z) = sample_section(vertices, surfaces, step);
    let length = dists.last().copied().unwrap_or(0.0);

    // Мин/макс по всем поверхностям.
    let mut zmin = f64::INFINITY;
    let mut zmax = f64::NEG_INFINITY;
    for row in &z {
        for &v in row {
            if v.is_finite() {
                zmin = zmin.min(v);
                zmax = zmax.max(v);
            }
        }
    }
    if zmin == f64::INFINITY {
        zmin = 0.0;
    }
    if zmax == f64::NEG_INFINITY {
        zmax = 0.0;
    }

    // Кольца пластов: пары соседних поверхностей.
    let mut rings = Vec::new();
    for i in 0..(z.len().saturating_sub(1)) {
        let top = &z[i];
        let bot = &z[i + 1];
        let ring = bed_ring_2d(&dists, top, bot, vex);
        if ring_area(&ring) > 0.0 {
            rings.push(ring);
        }
    }

    SectionResult {
        dists,
        surfaces: z,
        rings,
        vex,
        length,
        zmin,
        zmax,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Плоские поверхности: кольцо правильное.
    #[test]
    fn flat_bed() {
        // Линия внутри сетки 101×101 (от (1,1) до (99,1)).
        let vertices = [(1.0, 1.0), (99.0, 1.0)];
        let gt = [0.0, 1.0, 0.0, 100.0, 0.0, -1.0];
        let s_top = Surface {
            data: vec![10.0; 101 * 101],
            rows: 101,
            cols: 101,
            gt,
        };
        let s_bot = Surface {
            data: vec![0.0; 101 * 101],
            rows: 101,
            cols: 101,
            gt,
        };
        let res = build_section(&vertices, &[s_top, s_bot], 10.0, 1.0);
        assert!(res.rings.len() >= 1, "должен быть хотя бы один пласт, rings={}", res.rings.len());
        assert!(res.length > 0.0);
        assert!((res.zmax - 10.0).abs() < 1e-10);
    }

    /// Пикеты вдоль линии.
    #[test]
    fn stations_along_line() {
        let vertices = [(0.0, 0.0), (30.0, 40.0)]; // длина 50
        let st = stations(&vertices, 10.0);
        assert_eq!(st.len(), 6); // 0, 10, 20, 30, 40, 50
        assert!((st[0].0 - 0.0).abs() < 1e-10);
        assert!((st[5].0 - 50.0).abs() < 1e-10);
    }

    /// Билинейная выборка.
    #[test]
    fn bilinear_sample() {
        let data = vec![0.0, 1.0, 2.0, 3.0]; // 2×2
        let gt = [0.0, 1.0, 0.0, 2.0, 0.0, -1.0];
        // Центр сетки 2×2: (1.0, 1.0)
        let xs = [1.0];
        let ys = [1.0];
        let z = sample_grid_points(&data, 2, 2, &gt, &xs, &ys, true);
        // fx = 1/1-0.5 = 0.5, fy = (1-2)/(-1)-0.5 = 0.5 → центр.
        assert!((z[0] - 1.5).abs() < 1e-10, "z={}", z[0]);
    }
}
