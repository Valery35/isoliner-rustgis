// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Растровые инструменты модуля командами RustGIS.
//!
//! Инструменту расширения RustGIS растров не даёт, а команда может звать
//! любые команды RustGIS. Растр на вход выгружается `share.exportRaster`
//! во временный GeoTIFF, выход пишется GeoTIFF-ом той же сетки и
//! добавляется слоем `layer.addData`. Окно команды - `commands[].form`
//! в `plugin.json`.

use std::path::{Path, PathBuf};

use rustgis_kit::{Call, Lang};
use serde_json::{Value as J, json};

use crate::args;
use crate::geotiff;
use crate::krige::{self, PolyTrend, Structure, Variogram};
use crate::mincurv::{self, Frame, Params};
use crate::points;
use crate::pyfmt::{self, Arg};
use crate::report::Report;
use crate::topo_surface;

/// Значение nodata выходов, как у модуля.
const NODATA: f64 = -9999.0;

/// Строка на языке интерфейса.
fn t(lang: Lang, ru: &'static str, en: &'static str) -> &'static str {
    match lang {
        Lang::Ru => ru,
        Lang::En => en,
    }
}

/// Своя папка прогона во временной папке расширения.
fn run_dir(name: &str) -> Result<PathBuf, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dir = crate::product::home()
        .join("raster")
        .join(format!("{name}-{stamp}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn path_text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Выгрузить растровый слой RustGIS в GeoTIFF и прочитать его.
fn export_raster(host: &mut dyn Call, layer: &str, dir: &Path) -> Result<geotiff::Grid, String> {
    let path = dir.join("input.tif");
    host.call(
        "share.exportRaster",
        json!({"layer": layer, "path": path_text(&path)}),
    )?;
    geotiff::read(&path)
}

/// Добавить GeoTIFF слоем.
fn add_raster(host: &mut dyn Call, path: &Path) -> Result<J, String> {
    host.call("layer.addData", json!({"path": path_text(path)}))
}

/// Медиана как `np.median` массива float32.
fn median_f32(v: &mut [f32]) -> f64 {
    v.sort_by(f32::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        f64::from(v[n / 2])
    } else {
        f64::from(((f64::from(v[n / 2 - 1]) + f64::from(v[n / 2])) / 2.0) as f32)
    }
}

/// 2.05 Уклон и экспозиция: команда `slope-aspect` с полем `input`.
pub fn slope_aspect(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбрана входная ЦМР.", "No input DEM is chosen.").to_string())?;
    let dir = run_dir("slope-aspect")?;
    let g = export_raster(host, layer, &dir)?;
    let cell = geotiff::square_cell(g.cell, 1e-3).map_err(|(cx, cy)| {
        pyfmt::format(
            t(
                lang,
                "Ячейка ЦМР не квадратная: %.6g по X и %.6g по Y. Допуск 0.1 процента, переинтерполируйте грид в квадратную ячейку.",
                "The DEM cell is not square: %.6g in X and %.6g in Y. The tolerance is 0.1 percent, resample the grid to a square cell.",
            ),
            &[Arg::from(cx), Arg::from(cy)],
        )
    })?;
    let (slope, aspect) = topo_surface::slope_aspect(&g.z, &g.mask, g.width, g.height, cell);
    let to32 = |v: &[f64]| -> Vec<f32> {
        v.iter()
            .map(|&x| if x.is_finite() { x as f32 } else { NODATA as f32 })
            .collect()
    };
    let (s32, a32) = (to32(&slope), to32(&aspect));
    let p_slope = dir.join(format!("{}.tif", t(lang, "Уклон, градусы", "Slope, degrees")));
    let p_aspect = dir.join(format!(
        "{}.tif",
        t(lang, "Экспозиция, градусы", "Aspect, degrees")
    ));
    geotiff::write_like(&p_slope, &g, &s32, NODATA)?;
    geotiff::write_like(&p_aspect, &g, &a32, NODATA)?;
    add_raster(host, &p_slope)?;
    add_raster(host, &p_aspect)?;

    // два числа, по которым сразу видно, живой результат или мусор
    let nd = NODATA as f32;
    let mut ok: Vec<f32> = s32.iter().copied().filter(|&v| v != nd).collect();
    let mut lines = Vec::new();
    if ok.is_empty() {
        lines.push(
            t(
                lang,
                "Уклон не посчитан ни в одной ячейке. Во входной ЦМР нет данных, либо всё закрыто nodata.",
                "Slope was not computed in a single cell: the input DEM has no data, or everything is covered by nodata.",
            )
            .to_string(),
        );
    } else {
        let lo = ok.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = ok.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let n_ok = ok.len();
        let med = median_f32(&mut ok);
        lines.push(pyfmt::format(
            t(
                lang,
                "Уклон: от %.4g до %.4g градусов, медиана %.4g.",
                "Slope: from %.4g to %.4g degrees, median %.4g.",
            ),
            &[Arg::from(f64::from(lo)), Arg::from(f64::from(hi)), Arg::from(med)],
        ));
        let flat = s32
            .iter()
            .zip(&a32)
            .filter(|&(&s, &a)| s != nd && a == -1.0)
            .count();
        lines.push(pyfmt::format(
            t(
                lang,
                "Ячеек с данными %d из %d, из них плоских без экспозиции %d.",
                "Cells with data %d of %d, of them flat with no aspect %d.",
            ),
            &[Arg::from(n_ok), Arg::from(s32.len()), Arg::from(flat)],
        ));
    }
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// Числовой параметр команды, пусто или не число - умолчание.
fn param_f64(params: &J, key: &str, default: f64) -> f64 {
    match params.get(key) {
        Some(J::Number(n)) => n.as_f64().unwrap_or(default),
        Some(J::String(s)) => s.trim().parse().unwrap_or(default),
        _ => default,
    }
}

/// Целочисленный параметр команды, пусто или не число - умолчание.
fn param_i64(params: &J, key: &str, default: i64) -> i64 {
    match params.get(key) {
        Some(J::Number(n)) => n.as_i64().unwrap_or(default),
        Some(J::String(s)) => s.trim().parse().unwrap_or(default),
        _ => default,
    }
}

/// Булев параметр команды: true для true/1/yes/да и т.п.
fn param_bool(params: &J, key: &str, default: bool) -> bool {
    match params.get(key) {
        Some(J::Bool(b)) => *b,
        Some(J::Number(n)) => n.as_i64().map_or(default, |v| v != 0),
        Some(J::String(s)) => {
            let t = s.trim().to_lowercase();
            ["1", "true", "yes", "да", "y", "t", "истина"].contains(&t.as_str())
        }
        _ => default,
    }
}

/// Строковый параметр команды.
fn param_str<'a>(params: &'a J, key: &'a str) -> &'a str {
    params.get(key).and_then(J::as_str).unwrap_or("")
}

/// Охват по координатам точек.
fn points_bbox(xs: &[f64], ys: &[f64]) -> (f64, f64, f64, f64) {
    let mut xmin = f64::INFINITY;
    let mut ymin = f64::INFINITY;
    let mut xmax = f64::NEG_INFINITY;
    let mut ymax = f64::NEG_INFINITY;
    for (x, y) in xs.iter().zip(ys) {
        xmin = xmin.min(*x);
        ymin = ymin.min(*y);
        xmax = xmax.max(*x);
        ymax = ymax.max(*y);
    }
    (xmin, ymin, xmax, ymax)
}

/// Охват всех координат GeoJSON FeatureCollection.
fn geojson_bbox(geojson: &J) -> Result<(f64, f64, f64, f64), String> {
    let mut xmin = f64::INFINITY;
    let mut ymin = f64::INFINITY;
    let mut xmax = f64::NEG_INFINITY;
    let mut ymax = f64::NEG_INFINITY;
    let mut any = false;
    for f in geojson["features"].as_array().unwrap_or(&Vec::new()) {
        let geom = &f["geometry"];
        visit_coords(geom, &mut |x: f64, y: f64| {
            if x.is_finite() && y.is_finite() {
                xmin = xmin.min(x);
                ymin = ymin.min(y);
                xmax = xmax.max(x);
                ymax = ymax.max(y);
                any = true;
            }
        });
    }
    if any {
        Ok((xmin, ymin, xmax, ymax))
    } else {
        Err("Extent layer has no usable geometry.".to_string())
    }
}

/// Обойти координаты геометрии GeoJSON и вызвать `f(x, y)`.
fn visit_coords(geom: &J, visitor: &mut impl FnMut(f64, f64)) {
    let t = geom["type"].as_str().unwrap_or("");
    let coords = &geom["coordinates"];
    let xy = |a: &[J]| {
        (
            a.first().and_then(J::as_f64),
            a.get(1).and_then(J::as_f64),
        )
    };
    match t {
        "Point" => {
            if let Some(a) = coords.as_array() && let (Some(x), Some(y)) = xy(a) {
                visitor(x, y);
            }
        }
        "MultiPoint" | "LineString" => {
            if let Some(a) = coords.as_array() {
                for p in a {
                    if let Some(pa) = p.as_array()
                        && let (Some(x), Some(y)) = xy(pa)
                    {
                        visitor(x, y);
                    }
                }
            }
        }
        "MultiLineString" | "Polygon" => {
            if let Some(rings) = coords.as_array() {
                for ring in rings {
                    if let Some(pts) = ring.as_array() {
                        for p in pts {
                            if let Some(pa) = p.as_array()
                                && let (Some(x), Some(y)) = xy(pa)
                            {
                                visitor(x, y);
                            }
                        }
                    }
                }
            }
        }
        "MultiPolygon" => {
            if let Some(pols) = coords.as_array() {
                for pol in pols {
                    if let Some(rings) = pol.as_array() {
                        for ring in rings {
                            if let Some(pts) = ring.as_array() {
                                for p in pts {
                                    if let Some(pa) = p.as_array()
                                        && let (Some(x), Some(y)) = xy(pa)
                                    {
                                        visitor(x, y);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Прочитанные из GeoJSON точки и статистика.
struct GeoPoints {
    xs: Vec<f64>,
    ys: Vec<f64>,
    vs: Vec<f64>,
    skipped: usize,
}

/// Прочитать точки и значения из GeoJSON (система координат слоя).
/// `zfield` пусто - значение из Z геометрии, иначе из свойства.
fn read_geojson_points(geojson: &J, zfield: &str) -> Result<GeoPoints, String> {
    let use_field = !zfield.is_empty();
    let mut pts = GeoPoints {
        xs: Vec::new(),
        ys: Vec::new(),
        vs: Vec::new(),
        skipped: 0,
    };
    for f in geojson["features"].as_array().unwrap_or(&Vec::new()) {
        let geom = &f["geometry"];
        let props = &f["properties"];
        let coords: Vec<J> = match geom["type"].as_str() {
            Some("Point") => vec![geom["coordinates"].clone()],
            Some("MultiPoint") => {
                geom["coordinates"].as_array().cloned().unwrap_or_default().to_vec()
            }
            _ => {
                pts.skipped += 1;
                continue;
            }
        };
        for c in coords {
            let a = c.as_array().ok_or("Bad point coordinates in GeoJSON.")?;
            if a.len() < 2 {
                pts.skipped += 1;
                continue;
            }
            let x = a[0]
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or("Bad X coordinate.")?;
            let y = a[1]
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or("Bad Y coordinate.")?;
            let v = if use_field {
                points::num(&props[zfield])
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| format!("Point has no numeric value in field '{zfield}'."))?
            } else {
                a.get(2)
                    .and_then(|v| v.as_f64())
                    .filter(|v| v.is_finite())
                    .ok_or("Point geometry has no Z value.")?
            };
            pts.xs.push(x);
            pts.ys.push(y);
            pts.vs.push(v);
        }
    }
    if pts.xs.len() < 2 {
        return Err(format!(
            "Need at least 2 points with values, got {}.",
            pts.xs.len()
        ));
    }
    Ok(pts)
}

/// 1.02 Минимальная кривизна (точки → растр): команда `minimum-curvature`.
pub fn minimum_curvature(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let input = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран точечный слой.", "No point layer is chosen.").to_string())?;
    let zfield = params["zfield"].as_str().unwrap_or("");
    let extent_layer = params["extent"].as_str().filter(|s| !s.is_empty());

    let geo = host.call(
        "host.features",
        json!({"layer": input, "crs": "layer"}),
    )?;
    let pts = read_geojson_points(&geo, zfield)?;

    let (xmin, ymin, xmax, ymax) = if let Some(el) = extent_layer {
        let g = host.call("host.features", json!({"layer": el, "crs": "layer"}))?;
        geojson_bbox(&g)?
    } else {
        points_bbox(&pts.xs, &pts.ys)
    };

    let width = xmax - xmin;
    let height = ymax - ymin;
    if width <= 0.0 || height <= 0.0 {
        return Err(t(lang, "Охват сетки нулевой.", "Grid extent is zero.").to_string());
    }

    let mut cell = param_f64(params, "cell_size", 0.0);
    if cell <= 0.0 {
        cell = (width.min(height) / 50.0).max(1e-12);
    }
    let nx = ((width / cell).ceil() as i64).max(2) as usize;
    let ny = ((height / cell).ceil() as i64).max(2) as usize;

    let tension = param_f64(params, "tension", 0.0).clamp(0.0, 1.0);
    let boundary_tension = param_f64(params, "boundary_tension", 0.0).clamp(0.0, 1.0);
    let relax = param_f64(params, "relax", 1.85).clamp(0.1, 1.99);
    let aniso = param_f64(params, "aniso", 1.0).max(0.05);
    let max_iter = param_i64(params, "max_iter", 100_000).max(1) as usize;
    let max_res = param_f64(params, "max_residual", 0.0).max(0.0);
    let zrange = pts.vs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - pts.vs.iter().copied().fold(f64::INFINITY, f64::min);
    let zrange = if zrange == 0.0 { 1.0 } else { zrange };
    let tol = if max_res > 0.0 {
        max_res
    } else {
        (1e-4 * zrange).max(1e-6)
    };

    let frame = Frame {
        xmin,
        ymin,
        cell,
        nx,
        ny,
    };
    let p = Params {
        tension,
        boundary_tension,
        relax,
        aniso,
        tol,
    };

    let (z0, fixed) = mincurv::grid_points(&pts.xs, &pts.ys, &pts.vs, &frame);
    let solved = mincurv::solve(z0, &fixed, &p, max_iter);

    let dir = run_dir("minimum-curvature")?;
    let path = dir.join(format!(
        "{}.tif",
        t(lang, "Минимальная кривизна", "Minimum curvature")
    ));
    let out32: Vec<f32> = solved
        .z
        .iter()
        .map(|&v| if v.is_finite() { v as f32 } else { NODATA as f32 })
        .collect();
    let grid = geotiff::Grid::from_geotransform(nx, ny, xmin, ymin, cell);
    geotiff::write_like(&path, &grid, &out32, NODATA)?;
    add_raster(host, &path)?;

    let n_data = fixed.iter().filter(|&&b| b).count();
    let mut lines = Vec::new();
    lines.push(pyfmt::format(
        t(
            lang,
            "Сетка %d x %d, ячейка %.4g. Узлов-данных: %d из %d.",
            "Grid %d x %d, cell %.4g. Data nodes: %d of %d.",
        ),
        &[
            Arg::from(nx as f64),
            Arg::from(ny as f64),
            Arg::from(cell),
            Arg::from(n_data as f64),
            Arg::from(nx as f64 * ny as f64),
        ],
    ));
    if pts.skipped > 0 {
        lines.push(pyfmt::format(
            t(
                lang,
                "Пропущено объектов без точечной геометрии: %d.",
                "Skipped features without point geometry: %d.",
            ),
            &[Arg::from(pts.skipped as f64)],
        ));
    }
    let nd = NODATA as f32;
    let mut ok: Vec<f32> = out32.iter().copied().filter(|&v| v != nd).collect();
    if !ok.is_empty() {
        let lo = ok.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = ok.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let med = median_f32(&mut ok);
        lines.push(pyfmt::format(
            t(
                lang,
                "Грид: от %.4g до %.4g, медиана %.4g.",
                "Grid: from %.4g to %.4g, median %.4g.",
            ),
            &[Arg::from(f64::from(lo)), Arg::from(f64::from(hi)), Arg::from(med)],
        ));
    }
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// Перевести точки GeoJSON в `points::Points` для отсева и усреднения.
fn geojson_to_points(geojson: &J) -> Result<points::Points, String> {
    let mut p = points::Points::default();
    for f in geojson["features"].as_array().unwrap_or(&Vec::new()) {
        let geom = &f["geometry"];
        let coords: Vec<J> = match geom["type"].as_str() {
            Some("Point") => vec![geom["coordinates"].clone()],
            Some("MultiPoint") => {
                geom["coordinates"].as_array().cloned().unwrap_or_default().to_vec()
            }
            _ => continue,
        };
        for c in coords {
            let a = c.as_array().ok_or("Bad point coordinates in GeoJSON.")?;
            if a.len() < 2 {
                continue;
            }
            let x = a[0]
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or("Bad X coordinate.")?;
            let y = a[1]
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or("Bad Y coordinate.")?;
            let z = a.get(2).and_then(|v| v.as_f64()).unwrap_or(f64::NAN);
            p.xs.push(x);
            p.ys.push(y);
            p.vs.push(z);
            p.ids.push(f.get("id").cloned().unwrap_or(J::Null));
        }
    }
    if p.xs.len() < 2 {
        return Err("Need at least 2 points with values.".to_string());
    }
    Ok(p)
}

/// Распознать модель вариограммы по строке выбора.
fn parse_model(s: &str) -> Option<i64> {
    match s.trim().to_lowercase().as_str() {
        "spherical" | "сферическая" | "sph" | "1" => Some(1),
        "exponential" | "экспоненциальная" | "exp" | "2" => Some(2),
        "gaussian" | "гауссова" | "gau" | "3" => Some(3),
        "power" | "степенная" | "pow" | "4" => Some(4),
        _ => None,
    }
}

/// 1.01 2D Kriging (точки → растр): команда `kriging-2d`.
pub fn kriging_2d(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let input = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран точечный слой.", "No point layer is chosen.").to_string())?;
    let zfield = params["zfield"].as_str().unwrap_or("");
    let extent_layer = params["extent"].as_str().filter(|s| !s.is_empty());

    let geo = host.call(
        "host.features",
        json!({"layer": input, "crs": "layer"}),
    )?;
    let mut pts = geojson_to_points(&geo)?;

    // Поле значения: пусто - из Z геометрии, иначе из свойств.
    if !zfield.is_empty() {
        for (i, f) in geo["features"].as_array().unwrap_or(&Vec::new()).iter().enumerate() {
            if i >= pts.vs.len() {
                break;
            }
            pts.vs[i] = points::num(&f["properties"][zfield])
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("Point has no numeric value in field '{zfield}'."))?;
        }
    } else {
        // Z уже взято в geojson_to_points; проверим, что оно конечно.
        for (i, v) in pts.vs.iter().enumerate() {
            if !v.is_finite() {
                return Err(format!("Point {i} has no Z value in geometry."));
            }
        }
    }

    // Отсев ураганных проб.
    let mut report = Report::default();
    let val_min = params["val_min"].as_f64();
    let val_max = params["val_max"].as_f64();
    let val_pct = param_f64(params, "val_pct", 0.0).clamp(0.0, 49.0);
    let val_cap = param_bool(params, "val_cap", false);
    points::clip_outliers(
        &mut pts,
        points::Outliers {
            vmin: val_min,
            vmax: val_max,
            pct: val_pct,
            cap: val_cap,
        },
        &mut report,
    )
    .map_err(|e| e.text())?;
    points::merge_coincident(&mut pts, &mut report);

    // Логарифмирование.
    let log_transform = param_bool(params, "log_transform", false);
    if log_transform {
        let n = pts.vs.len();
        let mut n_drop = 0usize;
        for v in &mut pts.vs {
            if *v > 0.0 {
                *v = v.ln();
            } else {
                *v = f64::NAN;
                n_drop += 1;
            }
        }
        pts.xs = pts
            .xs
            .iter()
            .zip(&pts.vs)
            .filter(|(_, v)| v.is_finite())
            .map(|(x, _)| *x)
            .collect();
        pts.ys = pts
            .ys
            .iter()
            .zip(&pts.vs)
            .filter(|(_, v)| v.is_finite())
            .map(|(y, _)| *y)
            .collect();
        pts.ids = pts
            .ids
            .iter()
            .zip(&pts.vs)
            .filter(|(_, v)| v.is_finite())
            .map(|(id, _)| id.clone())
            .collect();
        pts.vs.retain(|v| v.is_finite());
        if pts.vs.len() < 2 {
            return Err(t(lang, "После логарифма осталось менее 2 точек.", "Less than 2 points after log transform.").to_string());
        }
        report.info(
            "Логарифмирование: оставлено %d точек из %d (значения ≤ 0 отброшены: %d).",
            args![pts.vs.len() as f64, n as f64, n_drop as f64],
        );
    }

    // Снятие тренда.
    let mut trend: Option<PolyTrend> = None;
    if param_bool(params, "detrend", false) {
        let deg_str = param_str(params, "detrend_deg");
        let degree: usize = if deg_str.starts_with('2') { 2 } else { 1 };
        let need = PolyTrend::n_terms(degree);
        if pts.vs.len() > need {
            let var0 = crate::numpy::var(&pts.vs);
            let tr = PolyTrend::fit(&pts.xs, &pts.ys, &pts.vs, degree);
            for (i, v) in pts.vs.iter_mut().enumerate() {
                *v -= tr.at(pts.xs[i], pts.ys[i]);
            }
            let var1 = crate::numpy::var(&pts.vs);
            let share = if var0 > 0.0 { 100.0 * (1.0 - var1 / var0) } else { 0.0 };
            report.info(
                "Снят тренд степени %d: убрано %.1f%% дисперсии.",
                args![degree as f64, share],
            );
            trend = Some(tr);
        } else {
            report.warn(
                "Снятие тренда отключено: точек %d, для степени %d нужно больше %d.",
                args![pts.vs.len() as f64, degree as f64, need as f64],
            );
        }
    }

    // Охват.
    let (xmin, ymin, xmax, ymax) = if let Some(el) = extent_layer {
        let g = host.call("host.features", json!({"layer": el, "crs": "layer"}))?;
        geojson_bbox(&g)?
    } else {
        points_bbox(&pts.xs, &pts.ys)
    };
    let width = xmax - xmin;
    let height = ymax - ymin;
    if width <= 0.0 || height <= 0.0 {
        return Err(t(lang, "Охват сетки нулевой.", "Grid extent is zero.").to_string());
    }

    // Сетка.
    let mut cell = param_f64(params, "cell_size", 0.0);
    if cell <= 0.0 {
        cell = (width.min(height) / 50.0).max(1e-12);
    }
    let nx = ((width / cell).ceil() as i64).max(2) as usize;
    let ny = ((height / cell).ceil() as i64).max(2) as usize;
    let xmn = xmin + 0.5 * cell;
    let ymn = ymin + 0.5 * cell;

    // Поиск.
    let mut radius = param_f64(params, "radius", 0.0);
    if radius <= 0.0 {
        radius = width.hypot(height);
    }
    let rad2 = radius * radius;
    let ndmin = param_i64(params, "min_points", 1).max(1) as usize;
    let ndmax = param_i64(params, "max_points", 24).max(1) as usize;
    let ktype = if param_str(params, "ktype").starts_with("Simple") { 0 } else { 1 };
    let skmean = param_f64(params, "skmean", 0.0);

    // Вариограмма.
    let nugget = param_f64(params, "nugget", 0.0).max(0.0);
    let model_code = parse_model(param_str(params, "s1_model")).unwrap_or(1);
    let sill = param_f64(params, "s1_sill", 1.0).max(0.0);
    let auto_range = width.max(height) / 3.0;
    let mut range = param_f64(params, "s1_range", 0.0);
    if model_code == 4 {
        if range <= 0.0 {
            range = 1.0;
        } else if !(0.0..2.0).contains(&range) {
            range = range.clamp(0.05, 1.999);
        }
    } else if range <= 0.0 {
        range = auto_range;
    }
    let azimuth = param_f64(params, "s1_azimuth", 0.0);
    let anis = param_f64(params, "s1_anis", 1.0).max(krige::EPS);
    let vg = Variogram::new(
        nugget,
        &[Structure {
            it: model_code,
            cc: sill,
            aa: range,
            ang: azimuth,
            anis,
        }],
    );

    report.info(
        "Сетка %d x %d, ячейка %.4g, точек %d, структур %d.",
        args![nx as f64, ny as f64, cell, pts.vs.len() as f64, 1.0],
    );
    report.info(
        "Дисперсия данных: %.4g. Ориентир: суммарный силл (C0 + C) близок к ней.",
        args![crate::numpy::var(&pts.vs)],
    );

    // Кригинг по ячейкам.
    let want_se = param_bool(params, "output_stderr", false);
    let mut est = vec![NODATA; nx * ny];
    let mut stderr = if want_se { Some(vec![NODATA; nx * ny]) } else { None };
    let mut n_idw = 0usize;
    for row in 0..ny {
        let iy = ny - row;
        let yloc = ymn + (iy - 1) as f64 * cell;
        for ix in 0..nx {
            let xloc = xmn + ix as f64 * cell;
            let (e, v) = krige::solve_point(
                xloc,
                yloc,
                &pts.xs,
                &pts.ys,
                &pts.vs,
                &vg,
                ktype,
                skmean,
                ndmin,
                ndmax,
                rad2,
                NODATA,
            );
            let idx = row * nx + ix;
            if e == NODATA {
                continue;
            }
            if e.is_finite() {
                est[idx] = e;
                if let Some(se) = stderr.as_mut() {
                    se[idx] = v.sqrt();
                }
            } else {
                // Запас IDW: обратные расстояния до тех же соседей.
                n_idw += 1;
                let mut wsum = 0.0;
                let mut num = 0.0;
                for i in 0..pts.xs.len() {
                    let dx = pts.xs[i] - xloc;
                    let dy = pts.ys[i] - yloc;
                    let h2 = dx * dx + dy * dy;
                    let w = 1.0 / h2.max(krige::EPS);
                    wsum += w;
                    num += w * pts.vs[i];
                }
                if wsum > 0.0 {
                    est[idx] = num / wsum;
                    if let Some(se) = stderr.as_mut() {
                        se[idx] = vg.maxcov.sqrt();
                    }
                }
            }
        }
    }

    // Возврат тренда.
    if let Some(tr) = &trend {
        let xs_cells: Vec<f64> = (0..nx).map(|ix| xmn + ix as f64 * cell).collect();
        for row in 0..ny {
            let iy = ny - row;
            let yloc = ymn + (iy - 1) as f64 * cell;
            for (ix, xloc) in xs_cells.iter().enumerate().take(nx) {
                let idx = row * nx + ix;
                if est[idx] != NODATA {
                    est[idx] += tr.at(*xloc, yloc);
                }
            }
        }
    }

    // Обратное логарифмирование.
    if log_transform {
        for v in est.iter_mut() {
            if *v != NODATA {
                *v = v.exp();
            }
        }
        if let Some(se) = stderr.as_mut() {
            for (i, v) in se.iter_mut().enumerate() {
                if *v != NODATA {
                    *v *= est[i];
                }
            }
        }
    }

    // Запись GeoTIFF.
    let dir = run_dir("kriging-2d")?;
    let out_path = dir.join(format!(
        "{}.tif",
        t(lang, "Кригинг 2D", "2D Kriging")
    ));
    let out32: Vec<f32> = est
        .iter()
        .map(|&v| if v.is_finite() { v as f32 } else { NODATA as f32 })
        .collect();
    let grid = geotiff::Grid::from_geotransform(nx, ny, xmin, ymin, cell);
    geotiff::write_like(&out_path, &grid, &out32, NODATA)?;
    add_raster(host, &out_path)?;

    if let Some(se) = stderr {
        let se_path = dir.join(format!(
            "{}.tif",
            t(lang, "Стандартная ошибка кригинга", "Kriging standard error")
        ));
        let se32: Vec<f32> = se
            .iter()
            .map(|&v| if v.is_finite() { v as f32 } else { NODATA as f32 })
            .collect();
        geotiff::write_like(&se_path, &grid, &se32, NODATA)?;
        add_raster(host, &se_path)?;
    }

    // Отчёт.
    let mut lines: Vec<String> = report.lines.iter().map(|l| l.text()).collect();
    let n_data = est.iter().filter(|&&v| v != NODATA).count();
    lines.push(pyfmt::format(
        t(
            lang,
            "Сетка %d x %d, ячейка %.4g. Оценённых ячеек: %d из %d.",
            "Grid %d x %d, cell %.4g. Estimated cells: %d of %d.",
        ),
        &[
            Arg::from(nx as f64),
            Arg::from(ny as f64),
            Arg::from(cell),
            Arg::from(n_data as f64),
            Arg::from(nx as f64 * ny as f64),
        ],
    ));
    if n_idw > 0 {
        lines.push(pyfmt::format(
            t(
                lang,
                "Ячеек, посчитанных запасом IDW: %d.",
                "Cells estimated by IDW fallback: %d.",
            ),
            &[Arg::from(n_idw as f64)],
        ));
    }
    let nd = NODATA as f32;
    let mut ok: Vec<f32> = out32.iter().copied().filter(|&v| v != nd).collect();
    if !ok.is_empty() {
        let lo = ok.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = ok.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let med = median_f32(&mut ok);
        lines.push(pyfmt::format(
            t(
                lang,
                "Грид: от %.4g до %.4g, медиана %.4g.",
                "Grid: from %.4g to %.4g, median %.4g.",
            ),
            &[Arg::from(f64::from(lo)), Arg::from(f64::from(hi)), Arg::from(med)],
        ));
    }
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// Аппроксимация erf(x) по Abramowitz & Stegun 7.1.26.
fn erf(x: f64) -> f64 {
    let a1 = 0.254_829_592;
    let a2 = -0.284_496_736;
    let a3 = 1.421_413_741;
    let a4 = -1.453_152_027;
    let a5 = 1.061_405_429;
    let p = 0.327_591_1;
    let sign = x.signum();
    let x = x.abs();
    let t = 1.0 / (1.0 + p * x);
    let y = 1.0
        - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1)
            * t
            * (-x * x).exp();
    sign * y
}

/// Стандартная нормальная CDF Φ(x).
fn normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

/// 3.03 Карта вероятности превышения: команда `probability-threshold`.
/// `side` = "above" даёт P(Z > t), "below" - P(Z < t).
pub fn probability_threshold(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let est_layer = params["estimate"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран растр оценки.", "No estimate raster is chosen.").to_string())?;
    let se_layer = params["stderr"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(
                lang,
                "Не выбран растр стандартной ошибки.",
                "No standard-error raster is chosen.",
            )
            .to_string()
        })?;
    let threshold = param_f64(params, "threshold", 0.0);
    let side = param_str(params, "side");
    let below = side.starts_with("Below");

    let dir = run_dir("probability-threshold")?;
    let est_grid = export_raster(host, est_layer, &dir)?;
    let se_grid = export_raster(host, se_layer, &dir)?;
    if est_grid.width != se_grid.width || est_grid.height != se_grid.height {
        return Err(t(
            lang,
            "Размеры растров оценки и ошибки не совпадают.",
            "Estimate and standard-error rasters have different sizes.",
        )
        .to_string());
    }
    let _cell = geotiff::square_cell(est_grid.cell, 1e-3).map_err(|_| {
        t(
            lang,
            "Ячейка растра не квадратная.",
            "The raster cell is not square.",
        )
        .to_string()
    })?;

    let nd = NODATA as f32;
    let mut prob: Vec<f32> = Vec::with_capacity(est_grid.z.len());
    let mut n_ok = 0usize;
    for i in 0..est_grid.z.len() {
        let e = est_grid.z[i];
        let s = se_grid.z[i];
        let valid = e.is_finite() && s.is_finite() && s > 0.0;
        if !valid || est_grid.mask[i] || se_grid.mask[i] {
            prob.push(nd);
            continue;
        }
        let z = if below {
            (threshold - e) / s
        } else {
            (e - threshold) / s
        };
        let p = normal_cdf(z).clamp(0.0, 1.0);
        prob.push(p as f32);
        n_ok += 1;
    }

    let out_path = dir.join(format!(
        "{}.tif",
        t(
            lang,
            "Вероятность превышения",
            "Probability of exceedance"
        )
    ));
    geotiff::write_like(&out_path, &est_grid, &prob, NODATA)?;
    add_raster(host, &out_path)?;

    let mut lines = Vec::new();
    lines.push(pyfmt::format(
        t(
            lang,
            "Ячеек с вероятностью: %d из %d. Порог %.4g, сторона %s.",
            "Cells with probability: %d of %d. Threshold %.4g, side %s.",
        ),
        &[
            Arg::from(n_ok as f64),
            Arg::from(prob.len() as f64),
            Arg::from(threshold),
            Arg::from(if below { "below" } else { "above" }),
        ],
    ));
    let mut ok: Vec<f32> = prob.iter().copied().filter(|&v| v != nd).collect();
    if !ok.is_empty() {
        let lo = ok.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = ok.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let med = median_f32(&mut ok);
        lines.push(pyfmt::format(
            t(
                lang,
                "Вероятности: от %.4g до %.4g, медиана %.4g.",
                "Probabilities: from %.4g to %.4g, median %.4g.",
            ),
            &[Arg::from(f64::from(lo)), Arg::from(f64::from(hi)), Arg::from(med)],
        ));
    }
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 3.04 Гидравлический градиент и направление потока: команда `flow-gradient`.
pub fn flow_gradient(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран растр напора.", "No head raster is chosen.").to_string())?;
    let dir = run_dir("flow-gradient")?;
    let g = export_raster(host, layer, &dir)?;
    let cell = geotiff::square_cell(g.cell, 1e-3).map_err(|(cx, cy)| {
        pyfmt::format(
            t(
                lang,
                "Ячейка растра не квадратная: %.6g по X и %.6g по Y.",
                "The raster cell is not square: %.6g in X and %.6g in Y.",
            ),
            &[Arg::from(cx), Arg::from(cy)],
        )
    })?;

    let (mag, az) = crate::hydro::head_gradient(&g.z, g.width, g.height, cell, cell, NODATA);
    let to32 = |v: &[f64]| -> Vec<f32> {
        v.iter()
            .map(|&x| if x.is_finite() { x as f32 } else { NODATA as f32 })
            .collect()
    };
    let (m32, a32) = (to32(&mag), to32(&az));
    let p_mag = dir.join(format!(
        "{}.tif",
        t(lang, "Гидравлический градиент", "Hydraulic gradient")
    ));
    let p_az = dir.join(format!(
        "{}.tif",
        t(lang, "Направление потока", "Flow direction")
    ));
    geotiff::write_like(&p_mag, &g, &m32, NODATA)?;
    geotiff::write_like(&p_az, &g, &a32, NODATA)?;
    add_raster(host, &p_mag)?;
    add_raster(host, &p_az)?;

    let nd = NODATA as f32;
    let mut ok: Vec<f32> = m32.iter().copied().filter(|&v| v != nd).collect();
    let mut lines = Vec::new();
    if ok.is_empty() {
        lines.push(
            t(
                lang,
                "Градиент не посчитан ни в одной ячейке. Во входном растре нет данных или всё на границе.",
                "Gradient was not computed in a single cell: the input raster has no data or everything is on the edge.",
            )
            .to_string(),
        );
    } else {
        let lo = ok.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = ok.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let n_ok = ok.len();
        let med = median_f32(&mut ok);
        lines.push(pyfmt::format(
            t(
                lang,
                "Градиент: от %.4g до %.4g, медиана %.4g.",
                "Gradient: from %.4g to %.4g, median %.4g.",
            ),
            &[Arg::from(f64::from(lo)), Arg::from(f64::from(hi)), Arg::from(med)],
        ));
        let n_flat = m32
            .iter()
            .zip(&a32)
            .filter(|&(&m, &a)| m != nd && a == nd)
            .count();
        lines.push(pyfmt::format(
            t(
                lang,
                "Ячеек с данными %d из %d, из них плоских без направления %d.",
                "Cells with data %d of %d, of them flat with no direction %d.",
            ),
            &[Arg::from(n_ok as f64), Arg::from(m32.len() as f64), Arg::from(n_flat as f64)],
        ));
    }
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 9.01 Наклоны и кривизна по оседаниям: команда `subsidence-tilt-curvature`.
pub fn subsidence_tilt_curvature(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран грид оседаний.", "No subsidence grid is chosen.").to_string())?;
    let units = param_str(params, "units");
    let sign = param_str(params, "sign");
    let base = param_f64(params, "base", 15.0).max(0.1);
    let reduce = param_bool(params, "reduce", true);

    let dir = run_dir("subsidence-tilt-curvature")?;
    let g = export_raster(host, layer, &dir)?;
    let cell = geotiff::square_cell(g.cell, 1e-3).map_err(|(cx, cy)| {
        pyfmt::format(
            t(
                lang,
                "Ячейка растра не квадратная: %.6g по X и %.6g по Y.",
                "The raster cell is not square: %.6g in X and %.6g in Y.",
            ),
            &[Arg::from(cx), Arg::from(cy)],
        )
    })?;

    // Перевод в метры, положительные вниз.
    let to_meters = if units.starts_with("метры") || units.starts_with("meters") {
        1.0
    } else {
        0.001
    };
    let flip = sign.starts_with("оседание со знаком минус")
        || sign.starts_with("negative")
        || sign.starts_with("minus");
    let mut z_m: Vec<f64> = g
        .z
        .iter()
        .map(|&v| {
            if v.is_finite() && v != NODATA {
                let s = if flip { -v } else { v };
                s * to_meters
            } else {
                f64::NAN
            }
        })
        .collect();
    if sign.starts_with("определить") || sign.starts_with("auto") {
        // Если большинство отрицательно — инвертируем (разность отметок).
        let finite: Vec<f64> = z_m.iter().copied().filter(|v| v.is_finite()).collect();
        if !finite.is_empty() {
            let neg = finite.iter().filter(|&&v| v < 0.0).count();
            if neg * 2 > finite.len() {
                for v in &mut z_m {
                    if v.is_finite() {
                        *v = -*v;
                    }
                }
            }
        }
    }

    let res = crate::subsidence::grid_deformations(&z_m, g.width, g.height, cell, base, reduce);
    let qi = if reduce { crate::subsidence::q_tilt(base) } else { 1.0 };
    let qk = if reduce { crate::subsidence::q_curv(base) } else { 1.0 };

    let to32 = |v: &[f64]| -> Vec<f32> {
        v.iter()
            .map(|&x| if x.is_finite() { x as f32 } else { NODATA as f32 })
            .collect()
    };
    let write = |name_key: &str, data: &[f64], scale: f64| -> Result<PathBuf, String> {
        let scaled: Vec<f64> = data.iter().map(|&v| if v.is_finite() { v * scale } else { v }).collect();
        let path = dir.join(format!("{}.tif", name_key));
        geotiff::write_like(&path, &g, &to32(&scaled), NODATA)?;
        Ok(path)
    };

    let p_tilt = write(
        t(lang, "Наклон", "Tilt"),
        &res.tilt,
        1000.0,
    )?;
    let p_az = write(
        t(lang, "Азимут наклона", "Tilt azimuth"),
        &res.azimuth,
        1.0,
    )?;
    let p_kdir = write(
        t(lang, "Кривизна вдоль наклона", "Curvature along tilt"),
        &res.k_dir,
        1e6,
    )?;
    let p_kmax = write(
        t(lang, "Главная кривизна max", "Max principal curvature"),
        &res.k_max,
        1e6,
    )?;
    let p_kmin = write(
        t(lang, "Главная кривизна min", "Min principal curvature"),
        &res.k_min,
        1e6,
    )?;

    add_raster(host, &p_tilt)?;
    add_raster(host, &p_az)?;
    add_raster(host, &p_kdir)?;
    add_raster(host, &p_kmax)?;
    add_raster(host, &p_kmin)?;

    let mut lines = Vec::new();
    lines.push(pyfmt::format(
        t(
            lang,
            "База %.4g м, ячейка %.4g м, qᵢ = %.4f, qₖ = %.4f.",
            "Base %.4g m, cell %.4g m, q_tilt = %.4f, q_curv = %.4f.",
        ),
        &[Arg::from(base), Arg::from(cell), Arg::from(qi), Arg::from(qk)],
    ));
    let nd = NODATA as f32;
    let mut ok: Vec<f32> = to32(&res.tilt).iter().copied().filter(|&v| v != nd).collect();
    if !ok.is_empty() {
        let lo = ok.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = ok.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let med = median_f32(&mut ok);
        lines.push(pyfmt::format(
            t(
                lang,
                "Наклон: от %.4g до %.4g мм/м, медиана %.4g мм/м.",
                "Tilt: from %.4g to %.4g mm/m, median %.4g mm/m.",
            ),
            &[Arg::from(f64::from(lo)), Arg::from(f64::from(hi)), Arg::from(med)],
        ));
    }
    let okk: Vec<f32> = to32(&res.k_dir).iter().copied().filter(|&v| v != nd).collect();
    if !okk.is_empty() {
        let lo = okk.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = okk.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        lines.push(pyfmt::format(
            t(
                lang,
                "Кривизна вдоль наклона: от %.4g до %.4g 10⁻⁶ 1/м.",
                "Curvature along tilt: from %.4g to %.4g 10⁻⁶ 1/m.",
            ),
            &[Arg::from(f64::from(lo)), Arg::from(f64::from(hi))],
        ));
    }
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 9.02 Горизонтальные деформации: команда `subsidence-horizontal`.
pub fn subsidence_horizontal(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["curv"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(
                lang,
                "Не выбран растр кривизны.",
                "No curvature raster is chosen.",
            )
            .to_string()
        })?;
    let formula = param_str(params, "formula");
    let half_l = param_f64(params, "half_l", 0.0);
    let depth = param_f64(params, "depth", 300.0).max(1.0);
    let boundary = param_str(params, "boundary");
    let l0 = param_f64(params, "l0", 15.0).max(0.1);

    let dir = run_dir("subsidence-horizontal")?;
    let g = export_raster(host, layer, &dir)?;
    let _cell = geotiff::square_cell(g.cell, 1e-3).map_err(|(cx, cy)| {
        pyfmt::format(
            t(
                lang,
                "Ячейка растра не квадратная: %.6g по X и %.6g по Y.",
                "The raster cell is not square: %.6g in X and %.6g in Y.",
            ),
            &[Arg::from(cx), Arg::from(cy)],
        )
    })?;

    let length = if formula.starts_with("ε = mₑ·K·l₀") || formula.starts_with("eps = m_e * K * l0") {
        l0
    } else if half_l > 0.0 {
        half_l
    } else {
        let delta = if boundary.starts_with("temporary") || boundary.starts_with("временная") {
            crate::subsidence::ANGLE_TEMPORARY
        } else {
            crate::subsidence::ANGLE_PERMANENT
        };
        crate::subsidence::half_trough_length(depth, delta, crate::subsidence::ANGLE_PSI)
    };

    let eps: Vec<f64> = g
        .z
        .iter()
        .map(|&v| {
            if v.is_finite() && v != NODATA {
                // вход в 10⁻⁶ 1/м -> в 1/м
                let curv = v * 1e-6;
                crate::subsidence::eps_from_curvature(curv, length)
            } else {
                f64::NAN
            }
        })
        .collect();

    let out32: Vec<f32> = eps
        .iter()
        .map(|&v| {
            if v.is_finite() {
                (v * 1000.0) as f32
            } else {
                NODATA as f32
            }
        })
        .collect();
    let out_path = dir.join(format!(
        "{}.tif",
        t(
            lang,
            "Горизонтальная деформация",
            "Horizontal deformation"
        )
    ));
    geotiff::write_like(&out_path, &g, &out32, NODATA)?;
    add_raster(host, &out_path)?;

    let mut lines = Vec::new();
    lines.push(pyfmt::format(
        t(
            lang,
            "Длина полумульды %.4g м. Деформация ε = mₑ·K·L.",
            "Half-trough length %.4g m. Deformation ε = mₑ·K·L.",
        ),
        &[Arg::from(length)],
    ));
    let nd = NODATA as f32;
    let ok: Vec<f32> = out32.iter().copied().filter(|&v| v != nd).collect();
    if !ok.is_empty() {
        let lo = ok.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = ok.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        lines.push(pyfmt::format(
            t(
                lang,
                "Деформация: сжатие до %.4g мм/м, растяжение до %.4g мм/м.",
                "Deformation: compression to %.4g mm/m, extension to %.4g mm/m.",
            ),
            &[Arg::from(f64::from(lo)), Arg::from(f64::from(hi))],
        ));
    }
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.04 Заполнение понижений: команда `fill-depressions`.
pub fn fill_depressions(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(
                lang,
                "Не выбран растр рельефа.",
                "No elevation raster is chosen.",
            )
            .to_string()
        })?;
    let dir = run_dir("fill-depressions")?;
    let g = export_raster(host, layer, &dir)?;
    let nodata = g.nodata.unwrap_or(f64::NAN);
    let mut z = g.z.clone();
    let before = z.clone();
    crate::topo_prep::fill_depressions(&mut z, g.height, g.width, nodata);
    let mut n_raised = 0usize;
    let mut max_raise = 0.0f64;
    let mut sum_raise = 0.0f64;
    for (a, b) in before.iter().zip(&z) {
        if a.is_finite() && b.is_finite() && *b > *a {
            n_raised += 1;
            let d = *b - *a;
            max_raise = max_raise.max(d);
            sum_raise += d;
        }
    }
    let out: Vec<f32> = z
        .iter()
        .map(|&v| if v.is_finite() { v as f32 } else { NODATA as f32 })
        .collect();
    let path = dir.join(format!(
        "{}.tif",
        t(lang, "Заполненные понижения", "Filled depressions")
    ));
    geotiff::write_like(&path, &g, &out, NODATA)?;
    add_raster(host, &path)?;
    let mut lines = Vec::new();
    if n_raised == 0 {
        lines.push(
            t(
                lang,
                "Понижений нет: растр остался прежним.",
                "No depressions: the raster is unchanged.",
            )
            .to_string(),
        );
    } else {
        lines.push(pyfmt::format(
            t(
                lang,
                "Заполнено понижений: %d ячеек.",
                "Depressions filled: %d cells.",
            ),
            &[Arg::from(n_raised as f64)],
        ));
        lines.push(pyfmt::format(
            t(
                lang,
                "Максимальный подъём: %.4g, средний: %.4g.",
                "Maximum raise: %.4g, mean: %.4g.",
            ),
            &[
                Arg::from(max_raise),
                Arg::from(sum_raise / n_raised as f64),
            ],
        ));
    }
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.05 Сток и аккумуляция D8: команда `flow-d8`.
pub fn flow_d8(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(
                lang,
                "Не выбрана входная ЦМР.",
                "No input DEM is chosen.",
            )
            .to_string()
        })?;
    let fill = params["fill"].as_bool().unwrap_or(true);
    let dir = run_dir("flow-d8")?;
    let g = export_raster(host, layer, &dir)?;
    let nodata = g.nodata.unwrap_or(f64::NAN);
    let mut z = g.z.clone();
    if fill {
        crate::topo_prep::fill_depressions(&mut z, g.height, g.width, nodata);
    }
    let d = crate::topo_flow::d8_flow(&z, g.height, g.width, nodata);
    let acc = crate::topo_flow::flow_accumulation(&d, None, g.height, g.width, nodata);
    // D8 index -> ArcGIS code: E=1, SE=2, S=4, SW=8, W=16, NW=32, N=64, NE=128.
    let code = |i: i32| -> f32 {
        match i {
            0 => 128.0,
            1 => 1.0,
            2 => 2.0,
            3 => 4.0,
            4 => 8.0,
            5 => 16.0,
            6 => 32.0,
            7 => 64.0,
            _ => 0.0,
        }
    };
    let mut d32: Vec<f32> = d.iter().map(|&i| code(i)).collect();
    let mut acc32: Vec<f32> = acc.iter().map(|&a| a as f32).collect();
    for (i, &m) in g.mask.iter().enumerate() {
        if m {
            d32[i] = 255.0;
            acc32[i] = -1.0;
        }
    }
    let p_dir = dir.join(format!(
        "{}.tif",
        t(lang, "Направления стока", "Flow directions")
    ));
    let p_acc = dir.join(format!(
        "{}.tif",
        t(lang, "Аккумуляция", "Flow accumulation")
    ));
    geotiff::write_like(&p_dir, &g, &d32, 255.0)?;
    geotiff::write_like(&p_acc, &g, &acc32, -1.0)?;
    add_raster(host, &p_dir)?;
    add_raster(host, &p_acc)?;
    let max_acc = acc.iter().cloned().fold(0.0f64, f64::max);
    let lines = vec![pyfmt::format(
        t(
            lang,
            "Максимальная аккумуляция: %d ячеек.",
            "Maximum accumulation: %d cells.",
        ),
        &[Arg::from(max_acc.round())],
    )];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.06 Речная сеть: команда `river-network`.
pub fn river_network(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(
                lang,
                "Не выбрана входная ЦМР.",
                "No input DEM is chosen.",
            )
            .to_string()
        })?;
    let threshold = params["threshold"].as_f64().unwrap_or(1000.0).max(1.0);
    let fill = params["fill"].as_bool().unwrap_or(true);
    let dir = run_dir("river-network")?;
    let g = export_raster(host, layer, &dir)?;
    let nodata = g.nodata.unwrap_or(f64::NAN);
    let mut z = g.z.clone();
    if fill {
        crate::topo_prep::fill_depressions(&mut z, g.height, g.width, nodata);
    }
    let d = crate::topo_flow::d8_flow(&z, g.height, g.width, nodata);
    let acc = crate::topo_flow::flow_accumulation(&d, None, g.height, g.width, nodata);
    let links = crate::topo_flow::river_links(&d, &acc, g.height, g.width, threshold);
    let (x0, y0) = g.origin();
    let (cx, cy) = g.cell;
    let mut features = Vec::new();
    for l in &links {
        let coords: Vec<J> = l
            .cells
            .iter()
            .map(|&k| {
                let r = k / g.width;
                let c = k % g.width;
                json!([x0 + (c as f64 + 0.5) * cx, y0 - (r as f64 + 0.5) * cy])
            })
            .collect();
        let mut length = 0.0f64;
        for w in l.cells.windows(2) {
            let (r1, c1) = (w[0] / g.width, w[0] % g.width);
            let (r2, c2) = (w[1] / g.width, w[1] % g.width);
            let dx = (c2 as f64 - c1 as f64) * cx;
            let dy = (r2 as f64 - r1 as f64) * cy;
            length += (dx * dx + dy * dy).sqrt();
        }
        features.push(json!({
            "type": "Feature",
            "geometry": {"type": "LineString", "coordinates": coords},
            "properties": {
                "order": l.order,
                "acc_out": l.acc_out,
                "length_m": length
            }
        }));
    }
    let fc = json!({"type": "FeatureCollection", "features": features});
    let name = t(lang, "Речная сеть", "River network");
    host.call("host.addLayer", json!({"name": name, "geojson": fc}))?;
    let lines = vec![
        pyfmt::format(
            t(lang, "Звеньев: %d.", "Links: %d."),
            &[Arg::from(links.len() as f64)],
        ),
        pyfmt::format(
            t(
                lang,
                "Порог аккумуляции: %.4g ячеек.",
                "Accumulation threshold: %.4g cells.",
            ),
            &[Arg::from(threshold)],
        ),
    ];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.09 Вершины и ямы: команда `peaks-pits`.
pub fn peaks_pits(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(
                lang,
                "Не выбран растр рельефа.",
                "No elevation raster is chosen.",
            )
            .to_string()
        })?;
    let window = params["window"].as_u64().unwrap_or(3).max(3) as usize;
    let min_prominence = params["min_prominence"].as_f64().unwrap_or(0.0);
    let dir = run_dir("peaks-pits")?;
    let g = export_raster(host, layer, &dir)?;
    let nodata = g.nodata.unwrap_or(f64::NAN);
    let found = crate::topo_ext::peaks_pits(
        &g.z,
        g.height,
        g.width,
        nodata,
        window,
        min_prominence,
    );
    let (x0, y0) = g.origin();
    let (cx, cy) = g.cell;
    let mut peaks = Vec::new();
    let mut pits = Vec::new();
    for p in &found {
        let x = x0 + (p.col as f64 + 0.5) * cx;
        let y = y0 - (p.row as f64 + 0.5) * cy;
        let f = json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [x, y]},
            "properties": {"z": p.z, "prominence": p.prominence}
        });
        match p.kind {
            crate::topo_ext::Extremum::Peak => peaks.push(f),
            crate::topo_ext::Extremum::Pit => pits.push(f),
        }
    }
    let peaks_name = t(lang, "Вершины", "Peaks");
    let pits_name = t(lang, "Ямы", "Pits");
    host.call(
        "host.addLayer",
        json!({"name": peaks_name, "geojson": {"type": "FeatureCollection", "features": peaks}}),
    )?;
    host.call(
        "host.addLayer",
        json!({"name": pits_name, "geojson": {"type": "FeatureCollection", "features": pits}}),
    )?;
    let lines = vec![pyfmt::format(
        t(lang, "Вершин: %d, ям: %d.", "Peaks: %d, pits: %d."),
        &[Arg::from(peaks.len() as f64), Arg::from(pits.len() as f64)],
    )];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.07 Бассейны и водоразделы: команда `basins` (растр меток).
/// Полигоны пока не строятся: выход - растр номеров бассейнов.
pub fn basins(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(
                lang,
                "Не выбрана входная ЦМР.",
                "No input DEM is chosen.",
            )
            .to_string()
        })?;
    let threshold = params["threshold"].as_f64().unwrap_or(1.0).max(1.0);
    let fill = params["fill"].as_bool().unwrap_or(true);
    let dir = run_dir("basins")?;
    let g = export_raster(host, layer, &dir)?;
    let nodata = g.nodata.unwrap_or(f64::NAN);
    let mut z = g.z.clone();
    if fill {
        crate::topo_prep::fill_depressions(&mut z, g.height, g.width, nodata);
    }
    let d = crate::topo_flow::d8_flow(&z, g.height, g.width, nodata);
    let acc = crate::topo_flow::flow_accumulation(&d, None, g.height, g.width, nodata);
    let n = g.width * g.height;
    // Устья: ячейки на рамке, откуда поток уходит с грида (нет нижнего
    // соседа внутри), с аккумуляцией не ниже порога. Внутренние плоские
    // участки без стока устьями не считаются.
    let mut outlets: Vec<usize> = (0..n)
        .filter(|&k| {
            if !acc[k].is_finite() || acc[k] < threshold || d[k] >= 0 {
                return false;
            }
            let r = k / g.width;
            let c = k % g.width;
            r == 0 || r == g.height - 1 || c == 0 || c == g.width - 1
        })
        .collect();
    outlets.sort_by(|&a, &b| {
        acc[b]
            .partial_cmp(&acc[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut labels = vec![0.0f32; n];
    let mut id = 1i32;
    for &k in &outlets {
        let r = k / g.width;
        let c = k % g.width;
        let ws = crate::topo_flow::watershed(&d, g.height, g.width, r, c);
        let mut touched = false;
        for (i, &b) in ws.iter().enumerate() {
            if b && labels[i] == 0.0 {
                labels[i] = id as f32;
                touched = true;
            }
        }
        if touched {
            id += 1;
        }
    }
    let path = dir.join(format!("{}.tif", t(lang, "Бассейны", "Basins")));
    geotiff::write_like(&path, &g, &labels, 0.0)?;
    add_raster(host, &path)?;
    let lines = vec![
        pyfmt::format(
            t(lang, "Бассейнов: %d.", "Basins: %d."),
            &[Arg::from(f64::from(id - 1))],
        ),
        pyfmt::format(
            t(
                lang,
                "Порог аккумуляции: %.4g ячеек.",
                "Accumulation threshold: %.4g cells.",
            ),
            &[Arg::from(threshold)],
        ),
    ];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.14 Снятие ступеней: команда `terrace-smooth`.
pub fn terrace_smooth(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(
                lang,
                "Не выбрана входная ЦМР.",
                "No input DEM is chosen.",
            )
            .to_string()
        })?;
    let interval = params["interval"].as_f64().unwrap_or(0.0);
    if interval <= 0.0 {
        return Err(t(
            lang,
            "Задайте сечение рельефа больше нуля.",
            "Set a contour interval greater than zero.",
        )
        .to_string());
    }
    let iters = params["iters"].as_u64().unwrap_or(50).min(500) as usize;
    let limit = params["limit"].as_f64().unwrap_or(0.5).clamp(0.0, 1.0);
    let dir = run_dir("terrace-smooth")?;
    let g = export_raster(host, layer, &dir)?;
    let nodata = g.nodata.unwrap_or(f64::NAN);
    let out = crate::topo_prep::smooth_clamped_terrace(
        &g.z,
        g.height,
        g.width,
        nodata,
        interval,
        iters,
        limit,
    );
    let out32: Vec<f32> = out
        .iter()
        .zip(&g.z)
        .map(|(&v, &orig)| {
            if orig.is_finite() && orig != nodata {
                v as f32
            } else {
                NODATA as f32
            }
        })
        .collect();
    let path = dir.join(format!(
        "{}.tif",
        t(lang, "Рельеф без ступеней", "Terrain without steps")
    ));
    geotiff::write_like(&path, &g, &out32, NODATA)?;
    add_raster(host, &path)?;
    let lines = vec![
        pyfmt::format(
            t(
                lang,
                "Сечение %.4g, допустимый сдвиг %.4g, итераций %d.",
                "Interval %.4g, allowed shift %.4g, iterations %d.",
            ),
            &[
                Arg::from(interval),
                Arg::from(limit * interval),
                Arg::from(iters as f64),
            ],
        ),
    ];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// Части линии из геометрии GeoJSON: LineString или MultiLineString.
fn line_parts(geometry: &J) -> Vec<Vec<(f64, f64)>> {
    let one = |coords: &J| -> Vec<(f64, f64)> {
        coords
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let a = p.as_array()?;
                Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
            })
            .collect()
    };
    match geometry["type"].as_str().unwrap_or("") {
        "LineString" => vec![one(&geometry["coordinates"])],
        "MultiLineString" => geometry["coordinates"]
            .as_array()
            .into_iter()
            .flatten()
            .map(one)
            .filter(|p| p.len() >= 2)
            .collect(),
        _ => Vec::new(),
    }
}

/// Обойти координаты GeoJSON вглубь и собрать охват.
fn collect_bbox(v: &J, a: &mut f64, b: &mut f64, c: &mut f64, d: &mut f64) {
    let Some(arr) = v.as_array() else { return };
    if arr.len() >= 2 {
        if let (Some(x), Some(y)) = (arr[0].as_f64(), arr[1].as_f64()) {
            *a = a.min(x);
            *b = b.min(y);
            *c = c.max(x);
            *d = d.max(y);
            return;
        }
    }
    for item in arr {
        collect_bbox(item, a, b, c, d);
    }
}

/// Медиана ряда, как `np.median`.
fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// 2.12 Невязка горизонталей против ЦМР: команда `contour-residual`.
pub fn contour_residual(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let contours = params["contours"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(
                lang,
                "Не выбран слой горизонталей.",
                "No contour layer is chosen.",
            )
            .to_string()
        })?;
    let dem = params["dem"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(lang, "Не выбран растр ЦМР.", "No DEM raster is chosen.").to_string()
        })?;
    let field = params["field"].as_str().unwrap_or("");
    if field.is_empty() {
        return Err(t(
            lang,
            "Не выбрано поле отметки горизонталей.",
            "No contour elevation field is chosen.",
        )
        .to_string());
    }
    let step = params["step"].as_f64().unwrap_or(0.0).max(0.0);
    let bilinear = params["sampling"].as_str() != Some("nearest");
    let interval = params["interval"].as_f64().unwrap_or(0.0).max(0.0);

    let dir = run_dir("contour-residual")?;
    let g = export_raster(host, dem, &dir)?;
    let fc = host.call(
        "host.features",
        json!({"layer": contours, "crs": "layer"}),
    )?;
    let (x0, y0) = g.origin();
    let gt = [x0, g.cell.0, 0.0, y0, 0.0, -g.cell.1];

    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let mut elevs = Vec::new();
    let mut holds = Vec::new();
    let mut has_hold = false;
    for (idx, f) in fc["features"].as_array().into_iter().flatten().enumerate() {
        let elev = f["properties"][field]
            .as_f64()
            .or_else(|| f["properties"][field].as_str().and_then(|s| s.parse().ok()));
        let Some(elev) = elev else { continue };
        let hold = f["properties"]["hold"].as_i64();
        if hold.is_some() {
            has_hold = true;
        }
        for part in line_parts(&f["geometry"]) {
            let pts: Vec<(f64, f64)> = if step > 0.0 {
                crate::section::stations(&part, step)
                    .into_iter()
                    .map(|(x, y, _)| (x, y))
                    .collect()
            } else {
                part
            };
            for (x, y) in pts {
                xs.push(x);
                ys.push(y);
                elevs.push(elev);
                holds.push(hold.unwrap_or(-1));
            }
        }
    }
    if xs.is_empty() {
        return Err(t(
            lang,
            "В слое горизонталей нет линий с числовой отметкой.",
            "The contour layer holds no lines with a numeric elevation.",
        )
        .to_string());
    }
    let zs = crate::section::sample_grid_points(
        &g.z, g.height, g.width, &gt, &xs, &ys, bilinear,
    );
    let mut features = Vec::new();
    let mut resid = Vec::new();
    let mut resid_hold = Vec::new();
    for i in 0..xs.len() {
        if !zs[i].is_finite() {
            continue;
        }
        let r = elevs[i] - zs[i];
        resid.push(r);
        if holds[i] >= 0 {
            resid_hold.push((holds[i], r));
        }
        features.push(json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [xs[i], ys[i]]},
            "properties": {
                "fid_src": i as i64,
                "elev": elevs[i],
                "z_dem": zs[i],
                "resid": r,
                "abs_resid": r.abs(),
                "hold": holds[i]
            }
        }));
    }
    if resid.is_empty() {
        return Err(t(
            lang,
            "Ни одна точка горизонталей не попала на растр.",
            "No contour point fell on the raster.",
        )
        .to_string());
    }
    let stats = |r: &[f64]| -> (f64, f64, f64, f64, f64) {
        let n = r.len() as f64;
        let mean = r.iter().sum::<f64>() / n;
        let std = (r.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
        let rmse = (r.iter().map(|v| v * v).sum::<f64>() / n).sqrt();
        let mut abs: Vec<f64> = r.iter().map(|v| v.abs()).collect();
        let med = median(&mut abs);
        let max = abs.iter().cloned().fold(0.0f64, f64::max);
        (mean, std, rmse, med, max)
    };
    let (mean, std, rmse, med, max) = stats(&resid);
    let mut lines = vec![pyfmt::format(
        t(
            lang,
            "Невязки: %d точек, смещение %.4g, СКО %.4g, RMSE %.4g, медиана |r| %.4g, максимум %.4g.",
            "Residuals: %d points, mean %.4g, std %.4g, RMSE %.4g, median |r| %.4g, max %.4g.",
        ),
        &[
            Arg::from(resid.len() as f64),
            Arg::from(mean),
            Arg::from(std),
            Arg::from(rmse),
            Arg::from(med),
            Arg::from(max),
        ],
    )];
    if interval > 0.0 {
        let beyond = resid.iter().filter(|r| r.abs() > interval / 2.0).count();
        lines.push(pyfmt::format(
            t(
                lang,
                "Промах больше половины сечения: %d из %d (%.1f%%).",
                "Beyond half the interval: %d of %d (%.1f%%).",
            ),
            &[
                Arg::from(beyond as f64),
                Arg::from(resid.len() as f64),
                Arg::from(100.0 * beyond as f64 / resid.len() as f64),
            ],
        ));
    }
    if has_hold {
        for (label_ru, label_en, want) in [
            ("построение", "building", 0i64),
            ("проверка", "validation", 1i64),
        ] {
            let sub: Vec<f64> = resid_hold
                .iter()
                .filter(|(h, _)| *h == want)
                .map(|(_, r)| *r)
                .collect();
            if sub.is_empty() {
                continue;
            }
            let (m, s, rm, md, mx) = stats(&sub);
            lines.push(pyfmt::format(
                t(
                    lang,
                    "%s: %d точек, смещение %.4g, СКО %.4g, RMSE %.4g, медиана |r| %.4g, максимум %.4g.",
                    "%s: %d points, mean %.4g, std %.4g, RMSE %.4g, median |r| %.4g, max %.4g.",
                ),
                &[
                    Arg::from(match lang {
                        Lang::Ru => label_ru,
                        Lang::En => label_en,
                    }),
                    Arg::from(sub.len() as f64),
                    Arg::from(m),
                    Arg::from(s),
                    Arg::from(rm),
                    Arg::from(md),
                    Arg::from(mx),
                ],
            ));
        }
    }
    let name = t(lang, "Невязки горизонталей", "Contour residuals");
    host.call(
        "host.addLayer",
        json!({"name": name, "geojson": {"type": "FeatureCollection", "features": features}}),
    )?;
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.13 Диагностика террасинга: команда `terracing-check`.
pub fn terracing_check(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(lang, "Не выбран растр ЦМР.", "No DEM raster is chosen.").to_string()
        })?;
    let interval = params["interval"].as_f64().unwrap_or(0.0);
    if interval <= 0.0 {
        return Err(t(
            lang,
            "Задайте сечение рельефа больше нуля.",
            "Set a contour interval greater than zero.",
        )
        .to_string());
    }
    let base = params["base"].as_f64().unwrap_or(0.0);
    let band = params["band"].as_f64().unwrap_or(0.1).clamp(0.01, 0.45);
    let min_drop = params["min_drop"].as_f64().unwrap_or(0.01).clamp(0.0, 1.0);
    let dir = run_dir("terracing-check")?;
    let g = export_raster(host, layer, &dir)?;
    let kv = crate::topo_break::profile_curvature(&g.z, g.height, g.width, g.cell.0);
    let kv32: Vec<f32> = kv
        .iter()
        .map(|&v| if v.is_finite() { v as f32 } else { NODATA as f32 })
        .collect();
    let path = dir.join(format!(
        "{}.tif",
        t(lang, "Вертикальная кривизна", "Profile curvature")
    ));
    geotiff::write_like(&path, &g, &kv32, NODATA)?;
    add_raster(host, &path)?;
    let s = crate::topo_break::terracing_stats(
        &g.z, g.height, g.width, g.cell.0, interval, base, band, min_drop,
    );
    let verdict = if s.attract_ratio >= 2.0 {
        t(lang, "террасинг есть", "terraced")
    } else if s.attract_ratio >= 1.5 {
        t(lang, "похоже на террасинг", "suspect")
    } else {
        t(lang, "террасинга нет", "clean")
    };
    let lines = vec![
        pyfmt::format(
            t(
                lang,
                "Кривизна: %d ячеек, средняя |k| %.4g, p95 %.4g, максимум %.4g.",
                "Curvature: %d cells, mean |k| %.4g, p95 %.4g, max %.4g.",
            ),
            &[
                Arg::from(s.n_curv as f64),
                Arg::from(s.curv_mean_abs),
                Arg::from(s.curv_p95_abs),
                Arg::from(s.curv_max_abs),
            ],
        ),
        pyfmt::format(
            t(
                lang,
                "Притяжение к уровням: %.3g (ожидалось %.3g), отношение %.3g; плоских исключено %.1f%%.",
                "Attraction to levels: %.3g (expected %.3g), ratio %.3g; flat skipped %.1f%%.",
            ),
            &[
                Arg::from(s.attract_share),
                Arg::from(s.attract_expect),
                Arg::from(s.attract_ratio),
                Arg::from(100.0 * s.flat_skipped),
            ],
        ),
        pyfmt::format(
            t(lang, "Вердикт: %s.", "Verdict: %s."),
            &[Arg::from(verdict)],
        ),
    ];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.15 Отчёт по створу: команда `gauge-report`.
/// Точки створов притягиваются к ячейке с наибольшей аккумуляцией в
/// радиусе, от неё строится водосбор и считается морфометрия.
pub fn gauge_report(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(lang, "Не выбран растр ЦМР.", "No DEM raster is chosen.").to_string()
        })?;
    let gauges = params["gauges"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(
                lang,
                "Не выбран слой створов.",
                "No gauge layer is chosen.",
            )
            .to_string()
        })?;
    let snap = params["snap"].as_f64().unwrap_or(100.0).max(0.0);
    let fill = params["fill"].as_bool().unwrap_or(true);
    let dir = run_dir("gauge-report")?;
    let g = export_raster(host, layer, &dir)?;
    let nodata = g.nodata.unwrap_or(f64::NAN);
    let mut z = g.z.clone();
    if fill {
        crate::topo_prep::fill_depressions(&mut z, g.height, g.width, nodata);
    }
    let d = crate::topo_flow::d8_flow(&z, g.height, g.width, nodata);
    let acc = crate::topo_flow::flow_accumulation(&d, None, g.height, g.width, nodata);
    let fc = host.call("host.features", json!({"layer": gauges, "crs": "layer"}))?;
    let (x0, y0) = g.origin();
    let (cx, cy) = g.cell;
    let sr = if snap > 0.0 {
        (snap / cx.max(cy)).ceil() as isize
    } else {
        0
    };
    let mut features = Vec::new();
    let mut lines = Vec::new();
    for (i, f) in fc["features"].as_array().into_iter().flatten().enumerate() {
        let Some(p) = f["geometry"]["coordinates"].as_array() else {
            continue;
        };
        let (Some(x), Some(y)) = (p.first().and_then(J::as_f64), p.get(1).and_then(J::as_f64))
        else {
            continue;
        };
        let col = ((x - x0) / cx).floor() as isize;
        let row = ((y0 - y) / cy).floor() as isize;
        if row < 0 || row >= g.height as isize || col < 0 || col >= g.width as isize {
            continue;
        }
        // Притяжка к ячейке с наибольшей аккумуляцией в радиусе.
        let mut best = (row as usize, col as usize);
        let mut best_acc = f64::NEG_INFINITY;
        for dr in -sr..=sr {
            for dc in -sr..=sr {
                let r = row + dr;
                let c = col + dc;
                if r < 0 || r >= g.height as isize || c < 0 || c >= g.width as isize {
                    continue;
                }
                let a = acc[r as usize * g.width + c as usize];
                if a.is_finite() && a > best_acc {
                    best_acc = a;
                    best = (r as usize, c as usize);
                }
            }
        }
        let (orow, ocol) = best;
        let basin = crate::topo_flow::watershed(&d, g.height, g.width, orow, ocol);
        let rep = crate::topo_gauge::gauge_report(
            &z, &d, &basin, g.height, g.width, orow, ocol, cx, nodata,
        );
        let ox = x0 + (ocol as f64 + 0.5) * cx;
        let oy = y0 - (orow as f64 + 0.5) * cy;
        features.push(json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [ox, oy]},
            "properties": {
                "gauge": i as i64,
                "area_m2": rep.area,
                "flow_len_m": rep.flow_length,
                "slope_mean": rep.slope_mean,
                "slope_1085": rep.slope_1085,
                "zmax": rep.zmax,
                "zmin": rep.zmin,
                "zoutlet": rep.zoutlet,
                "cells": rep.cells as i64,
                "perimeter_m": rep.perimeter,
                "relief_ratio": rep.relief_ratio,
                "tc_kirpich_s": rep.tc_kirpich,
                "elongation": rep.elongation
            }
        }));
        lines.push(pyfmt::format(
            t(
                lang,
                "Створ %d: площадь %.4g км², водоток %.4g км, уклон %.4g, высоты %.4g–%.4g.",
                "Gauge %d: area %.4g km², stream %.4g km, slope %.4g, elevations %.4g–%.4g.",
            ),
            &[
                Arg::from(i as f64 + 1.0),
                Arg::from(rep.area / 1e6),
                Arg::from(rep.flow_length / 1e3),
                Arg::from(rep.slope_mean),
                Arg::from(rep.zmin),
                Arg::from(rep.zmax),
            ],
        ));
    }
    if features.is_empty() {
        return Err(t(
            lang,
            "Ни один створ не попал на растр.",
            "No gauge fell on the raster.",
        )
        .to_string());
    }
    let name = t(lang, "Створы", "Gauges");
    host.call(
        "host.addLayer",
        json!({"name": name, "geojson": {"type": "FeatureCollection", "features": features}}),
    )?;
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.16 Водосбор линии: команда `ditch-catchment`.
pub fn ditch_catchment(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(lang, "Не выбран растр ЦМР.", "No DEM raster is chosen.").to_string()
        })?;
    let lines_layer = params["lines"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            t(lang, "Не выбран слой линий.", "No line layer is chosen.").to_string()
        })?;
    let fill = params["fill"].as_bool().unwrap_or(true);
    let dir = run_dir("ditch-catchment")?;
    let g = export_raster(host, layer, &dir)?;
    let nodata = g.nodata.unwrap_or(f64::NAN);
    let mut z = g.z.clone();
    if fill {
        crate::topo_prep::fill_depressions(&mut z, g.height, g.width, nodata);
    }
    let d = crate::topo_flow::d8_flow(&z, g.height, g.width, nodata);
    let fc = host.call("host.features", json!({"layer": lines_layer, "crs": "layer"}))?;
    let (x0, y0) = g.origin();
    let gt = [x0, g.cell.0, 0.0, y0, 0.0, -g.cell.1];
    let (cx, cy) = g.cell;
    let mut features = Vec::new();
    let mut lines = Vec::new();
    for (i, f) in fc["features"].as_array().into_iter().flatten().enumerate() {
        let parts = line_parts(&f["geometry"]);
        if parts.is_empty() {
            continue;
        }
        let mut basin = vec![false; g.width * g.height];
        let mut first = (0.0f64, 0.0f64);
        for (pi, part) in parts.iter().enumerate() {
            if pi == 0 && !part.is_empty() {
                first = part[0];
            }
            let ws = crate::topo_gauge::line_catchment(
                &d, g.height, g.width, part, &gt,
            );
            for k in 0..basin.len() {
                basin[k] |= ws[k];
            }
        }
        let cells = basin.iter().filter(|&&b| b).count();
        let area = cells as f64 * cx * cy;
        let mut zsum = 0.0;
        let mut zmin = f64::INFINITY;
        let mut zmax = f64::NEG_INFINITY;
        for k in 0..basin.len() {
            if basin[k] && z[k].is_finite() && z[k] != nodata {
                zsum += z[k];
                zmin = zmin.min(z[k]);
                zmax = zmax.max(z[k]);
            }
        }
        let zmean = if cells > 0 { zsum / cells as f64 } else { f64::NAN };
        features.push(json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [first.0, first.1]},
            "properties": {
                "ditch": i as i64,
                "area_km2": area / 1e6,
                "cells": cells as i64,
                "z_mean": zmean,
                "z_min": if zmin.is_finite() { zmin } else { f64::NAN },
                "z_max": if zmax.is_finite() { zmax } else { f64::NAN }
            }
        }));
        lines.push(pyfmt::format(
            t(
                lang,
                "Линия %d: водосбор %.4g км², ячеек %d, высоты %.4g–%.4g.",
                "Line %d: catchment %.4g km², %d cells, elevations %.4g–%.4g.",
            ),
            &[
                Arg::from(i as f64 + 1.0),
                Arg::from(area / 1e6),
                Arg::from(cells as f64),
                Arg::from(if zmin.is_finite() { zmin } else { f64::NAN }),
                Arg::from(if zmax.is_finite() { zmax } else { f64::NAN }),
            ],
        ));
    }
    if features.is_empty() {
        return Err(t(
            lang,
            "В слое линий нет подходящих объектов.",
            "The line layer holds no suitable features.",
        )
        .to_string());
    }
    let name = t(lang, "Водосборы линий", "Line catchments");
    host.call(
        "host.addLayer",
        json!({"name": name, "geojson": {"type": "FeatureCollection", "features": features}}),
    )?;
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.18 Насыпи и выемки: команда `cut-fill`.
pub fn cut_fill_cmd(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let ground = params["ground"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбрана поверхность «было».", "No ground surface is chosen.").to_string())?;
    let plan = params["plan"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбрана поверхность «стало».", "No design surface is chosen.").to_string())?;
    let dir = run_dir("cut-fill")?;
    let g = export_raster(host, ground, &dir)?;
    let p = export_raster(host, plan, &dir)?;
    if g.width != p.width || g.height != p.height {
        return Err(t(lang, "Поверхности разного размера.", "The surfaces have different sizes.").to_string());
    }
    let nodata = g.nodata.unwrap_or(f64::NAN);
    let rep = crate::volumes::cut_fill(&g.z, &p.z, g.cell.0 * g.cell.1, nodata);
    let diff: Vec<f32> = g
        .z
        .iter()
        .zip(&p.z)
        .map(|(&gv, &pv)| {
            if gv.is_finite() && pv.is_finite() && gv != nodata && pv != nodata {
                (gv - pv) as f32
            } else {
                NODATA as f32
            }
        })
        .collect();
    let path = dir.join(format!("{}.tif", t(lang, "Разница поверхностей", "Surface difference")));
    geotiff::write_like(&path, &g, &diff, NODATA)?;
    add_raster(host, &path)?;
    let lines = vec![
        pyfmt::format(
            t(lang, "Выемка %.4g м³, насыпь %.4g м³, нетто %.4g м³.", "Cut %.4g m³, fill %.4g m³, net %.4g m³."),
            &[Arg::from(rep.cut), Arg::from(rep.fill), Arg::from(rep.net)],
        ),
        pyfmt::format(
            t(lang, "Ячеек: выемка %d, насыпь %d, без изменений %d.", "Cells: cut %d, fill %d, unchanged %d."),
            &[Arg::from(rep.cut_cells as f64), Arg::from(rep.fill_cells as f64), Arg::from(rep.flat_cells as f64)],
        ),
    ];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.21 Демо-карьер: команда `topo-demo-pit`.
pub fn topo_demo_pit(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let rows = params["rows"].as_u64().unwrap_or(200).max(10) as usize;
    let cols = params["cols"].as_u64().unwrap_or(200).max(10) as usize;
    let cell = params["cell"].as_f64().unwrap_or(1.0).max(1e-6);
    let dir = run_dir("topo-demo-pit")?;
    let z = crate::topo_form::demo_quarry(rows, cols);
    let grid = geotiff::Grid::from_geotransform(cols, rows, 0.0, 0.0, cell);
    let z32: Vec<f32> = z.iter().map(|&v| v as f32).collect();
    let path = dir.join(format!("{}.tif", t(lang, "Демо-карьер", "Demo quarry")));
    geotiff::write_like(&path, &grid, &z32, NODATA)?;
    add_raster(host, &path)?;
    let lines = vec![pyfmt::format(
        t(lang, "Демо-карьер %d на %d ячеек.", "Demo quarry %d by %d cells."),
        &[Arg::from(cols as f64), Arg::from(rows as f64)],
    )];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.24 Снятие мостов: команда `bridge-removal`.
pub fn bridge_removal(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран растр рельефа.", "No elevation raster is chosen.").to_string())?;
    let threshold = params["threshold"].as_f64().unwrap_or(1.0).max(0.0);
    let dir = run_dir("bridge-removal")?;
    let g = export_raster(host, layer, &dir)?;
    let nodata = g.nodata.unwrap_or(f64::NAN);
    let mut z = g.z.clone();
    let replaced = crate::topo_ext::remove_bridges(&mut z, g.height, g.width, nodata, threshold);
    let z32: Vec<f32> = z.iter().map(|&v| if v.is_finite() { v as f32 } else { NODATA as f32 }).collect();
    let path = dir.join(format!("{}.tif", t(lang, "Без мостов", "Without bridges")));
    geotiff::write_like(&path, &g, &z32, NODATA)?;
    add_raster(host, &path)?;
    let lines = vec![pyfmt::format(
        t(lang, "Заменено ячеек: %d.", "Cells replaced: %d."),
        &[Arg::from(replaced as f64)],
    )];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.19 Кандидаты бровок и подошв: команда `breakline-candidates`.
pub fn breakline_candidates(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран растр рельефа.", "No elevation raster is chosen.").to_string())?;
    let threshold = params["threshold"].as_f64().unwrap_or(10.0).max(0.0);
    let dir = run_dir("breakline-candidates")?;
    let g = export_raster(host, layer, &dir)?;
    let nodata = g.nodata.unwrap_or(f64::NAN);
    let cand = crate::topo_form::brow_sole_candidates(
        &g.z, g.height, g.width, nodata, g.cell.0, threshold,
    );
    let (x0, y0) = g.origin();
    let (cx, cy) = g.cell;
    let mut features = Vec::new();
    let (mut n_brow, mut n_sole) = (0usize, 0usize);
    for c in &cand {
        let kind = match c.kind {
            crate::topo_form::BrowSoleKind::Brow => {
                n_brow += 1;
                "brow"
            }
            crate::topo_form::BrowSoleKind::Sole => {
                n_sole += 1;
                "sole"
            }
        };
        features.push(json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [x0 + (c.col as f64 + 0.5) * cx, y0 - (c.row as f64 + 0.5) * cy]},
            "properties": {"kind": kind, "z": c.z, "contrast": c.contrast}
        }));
    }
    let name = t(lang, "Кандидаты бровок и подошв", "Breakline candidates");
    host.call("host.addLayer", json!({"name": name, "geojson": {"type": "FeatureCollection", "features": features}}))?;
    let lines = vec![pyfmt::format(
        t(lang, "Бровок: %d, подошв: %d.", "Brows: %d, soles: %d."),
        &[Arg::from(n_brow as f64), Arg::from(n_sole as f64)],
    )];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.22 Снятие высот с ЦМР: команда `snap-elevations`.
pub fn snap_elevations(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let points = params["points"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран слой точек.", "No point layer is chosen.").to_string())?;
    let dem = params["dem"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран растр ЦМР.", "No DEM raster is chosen.").to_string())?;
    let bilinear = params["sampling"].as_str() != Some("nearest");
    let dir = run_dir("snap-elevations")?;
    let g = export_raster(host, dem, &dir)?;
    let fc = host.call("host.features", json!({"layer": points, "crs": "layer"}))?;
    let (x0, y0) = g.origin();
    let gt = [x0, g.cell.0, 0.0, y0, 0.0, -g.cell.1];
    let mut features = Vec::new();
    let mut got = 0usize;
    for f in fc["features"].as_array().into_iter().flatten() {
        let Some(c) = f["geometry"]["coordinates"].as_array() else { continue };
        let (Some(x), Some(y)) = (c.first().and_then(J::as_f64), c.get(1).and_then(J::as_f64)) else { continue };
        let z = crate::section::sample_grid_points(&g.z, g.height, g.width, &gt, &[x], &[y], bilinear)[0];
        let mut props = f["properties"].clone();
        if let Some(o) = props.as_object_mut() {
            o.insert("z".into(), if z.is_finite() { json!(z) } else { J::Null });
        }
        if z.is_finite() {
            got += 1;
        }
        features.push(json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [x, y]},
            "properties": props
        }));
    }
    let name = t(lang, "Точки с высотами", "Points with elevations");
    host.call("host.addLayer", json!({"name": name, "geojson": {"type": "FeatureCollection", "features": features}}))?;
    let lines = vec![pyfmt::format(
        t(lang, "Высота снята в %d точках из %d.", "Elevation read at %d of %d points."),
        &[Arg::from(got as f64), Arg::from(features.len() as f64)],
    )];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.23 Линии стока от точек: команда `downhill-trace`.
pub fn downhill_trace(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let starts = params["starts"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран слой точек.", "No point layer is chosen.").to_string())?;
    let dem = params["dem"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран растр ЦМР.", "No DEM raster is chosen.").to_string())?;
    let fill = params["fill"].as_bool().unwrap_or(true);
    let dir = run_dir("downhill-trace")?;
    let g = export_raster(host, dem, &dir)?;
    let nodata = g.nodata.unwrap_or(f64::NAN);
    let mut z = g.z.clone();
    if fill {
        crate::topo_prep::fill_depressions(&mut z, g.height, g.width, nodata);
    }
    let d = crate::topo_flow::d8_flow(&z, g.height, g.width, nodata);
    let fc = host.call("host.features", json!({"layer": starts, "crs": "layer"}))?;
    let (x0, y0) = g.origin();
    let (cx, cy) = g.cell;
    const OFF: [(isize, isize); 8] = [(-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0)];
    let mut features = Vec::new();
    for f in fc["features"].as_array().into_iter().flatten() {
        let Some(c) = f["geometry"]["coordinates"].as_array() else { continue };
        let (Some(x), Some(y)) = (c.first().and_then(J::as_f64), c.get(1).and_then(J::as_f64)) else { continue };
        let col = ((x - x0) / cx).floor() as isize;
        let row = ((y0 - y) / cy).floor() as isize;
        if row < 0 || row >= g.height as isize || col < 0 || col >= g.width as isize {
            continue;
        }
        let mut cells = vec![(row as usize, col as usize)];
        let mut seen = vec![false; g.width * g.height];
        let mut r = row as usize;
        let mut cc = col as usize;
        seen[r * g.width + cc] = true;
        while d[r * g.width + cc] >= 0 {
            let (dr, dc) = OFF[d[r * g.width + cc] as usize];
            let nr = r as isize + dr;
            let nc = cc as isize + dc;
            if nr < 0 || nr >= g.height as isize || nc < 0 || nc >= g.width as isize {
                break;
            }
            let (nr, nc) = (nr as usize, nc as usize);
            if seen[nr * g.width + nc] {
                break;
            }
            seen[nr * g.width + nc] = true;
            cells.push((nr, nc));
            r = nr;
            cc = nc;
        }
        if cells.len() < 2 {
            continue;
        }
        let coords: Vec<J> = cells.iter().map(|&(r, c)| json!([x0 + (c as f64 + 0.5) * cx, y0 - (r as f64 + 0.5) * cy])).collect();
        features.push(json!({"type": "Feature", "geometry": {"type": "LineString", "coordinates": coords}, "properties": {"cells": cells.len() as i64}}));
    }
    let name = t(lang, "Линии стока", "Downhill traces");
    host.call("host.addLayer", json!({"name": name, "geojson": {"type": "FeatureCollection", "features": features}}))?;
    let lines = vec![pyfmt::format(t(lang, "Линий: %d.", "Traces: %d."), &[Arg::from(features.len() as f64)])];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.17 Отчёт по водосбору: команда `catchment-stats`.
pub fn catchment_stats(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let basins = params["basins"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран растр бассейнов.", "No basin raster is chosen.").to_string())?;
    let dem = params["dem"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран растр ЦМР.", "No DEM raster is chosen.").to_string())?;
    let dir = run_dir("catchment-stats")?;
    let gb = export_raster(host, basins, &dir)?;
    let gd = export_raster(host, dem, &dir)?;
    if gb.width != gd.width || gb.height != gd.height {
        return Err(t(lang, "Растры разного размера.", "The rasters have different sizes.").to_string());
    }
    let (x0, y0) = gb.origin();
    let (cx, cy) = gb.cell;
    let mut by: std::collections::BTreeMap<i64, (usize, f64, f64, f64, f64, f64, f64)> = std::collections::BTreeMap::new();
    for k in 0..gb.z.len() {
        let id = gb.z[k];
        if !id.is_finite() || id <= 0.0 {
            continue;
        }
        let id = id.round() as i64;
        let e = by.entry(id).or_insert((0, 0.0, f64::INFINITY, f64::NEG_INFINITY, 0.0, 0.0, 0.0));
        e.0 += 1;
        let z = gd.z[k];
        if z.is_finite() {
            e.1 += z;
            e.2 = e.2.min(z);
            e.3 = e.3.max(z);
        }
        let r = k / gb.width;
        let c = k % gb.width;
        e.4 += x0 + (c as f64 + 0.5) * cx;
        e.5 += y0 - (r as f64 + 0.5) * cy;
    }
    let mut features = Vec::new();
    for (id, (n, zsum, zmin, zmax, xs, ys, _)) in &by {
        let area = *n as f64 * cx * cy;
        features.push(json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [xs / *n as f64, ys / *n as f64]},
            "properties": {
                "basin": id,
                "area_km2": area / 1e6,
                "cells": *n as i64,
                "z_mean": zsum / *n as f64,
                "z_min": if zmin.is_finite() { *zmin } else { f64::NAN },
                "z_max": if zmax.is_finite() { *zmax } else { f64::NAN }
            }
        }));
    }
    let name = t(lang, "Отчёт по водосборам", "Catchment report");
    host.call("host.addLayer", json!({"name": name, "geojson": {"type": "FeatureCollection", "features": features}}))?;
    let lines = vec![pyfmt::format(t(lang, "Бассейнов: %d.", "Basins: %d."), &[Arg::from(features.len() as f64)])];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// 2.20 Бровки и подошвы в работу: команда `breakline-pairs`.
/// Пары собираются спуском по D8 от пробных вершин бровки до подошвы.
pub fn breakline_pairs(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let layer = params["input"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран слой кандидатов.", "No candidate layer is chosen.").to_string())?;
    let dem = params["dem"].as_str().filter(|s| !s.is_empty());
    let kind_field = params["kind_field"].as_str().filter(|s| !s.is_empty()).unwrap_or("kind");
    let max_dist = params["max_dist"].as_f64().unwrap_or(50.0).max(1.0);
    let min_share = params["min_share"].as_f64().unwrap_or(0.4).clamp(0.1, 1.0);
    let dir = run_dir("breakline-pairs")?;
    let g = match dem {
        Some(d) => Some(export_raster(host, d, &dir)?),
        None => None,
    };
    let d8 = g.as_ref().map(|gr| {
        let nodata = gr.nodata.unwrap_or(f64::NAN);
        crate::topo_flow::d8_flow(&gr.z, gr.height, gr.width, nodata)
    });
    let fc = host.call("host.features", json!({"layer": layer, "crs": "layer"}))?;
    let (x0, y0) = g.as_ref().map(|gr| gr.origin()).unwrap_or((0.0, 0.0));
    let (cx, cy) = g.as_ref().map(|gr| gr.cell).unwrap_or((1.0, 1.0));
    let to_cell = |x: f64, y: f64| -> Option<(usize, usize)> {
        let gr = g.as_ref()?;
        let c = ((x - x0) / cx).floor() as isize;
        let r = ((y0 - y) / cy).floor() as isize;
        if r >= 0 && r < gr.height as isize && c >= 0 && c < gr.width as isize {
            Some((r as usize, c as usize))
        } else {
            None
        }
    };
    struct Line {
        parts: Vec<Vec<(f64, f64)>>,
        brow: bool,
    }
    let mut lines: Vec<Line> = Vec::new();
    for f in fc["features"].as_array().into_iter().flatten() {
        let kind = f["properties"][kind_field]
            .as_str()
            .unwrap_or("")
            .to_lowercase();
        let brow = kind.contains("brow") || kind.contains("бров");
        let toe = kind.contains("toe") || kind.contains("sole") || kind.contains("подош");
        if !brow && !toe {
            continue;
        }
        let parts = line_parts(&f["geometry"]);
        if parts.is_empty() {
            continue;
        }
        lines.push(Line { parts, brow });
    }
    // Ячейки подошв: ячейка -> номера линий.
    let mut toe_cells: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for (li, l) in lines.iter().enumerate() {
        if l.brow {
            continue;
        }
        for part in &l.parts {
            for w in part.windows(2) {
                let steps = 4;
                for s in 0..=steps {
                    let t = s as f64 / steps as f64;
                    let x = w[0].0 + (w[1].0 - w[0].0) * t;
                    let y = w[0].1 + (w[1].1 - w[0].1) * t;
                    if let Some((r, c)) = to_cell(x, y) {
                        if let Some(gr) = &g {
                            toe_cells.entry(r * gr.width + c).or_default().push(li);
                        }
                    }
                }
            }
        }
    }
    const OFF: [(isize, isize); 8] = [(-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0)];
    let mut pair_of: Vec<Option<usize>> = vec![None; lines.len()];
    let mut paired_toes: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (bi, l) in lines.iter().enumerate() {
        if !l.brow {
            continue;
        }
        let Some(gr) = &g else { continue };
        let Some(d) = &d8 else { continue };
        let mut votes: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        let mut total = 0usize;
        for part in &l.parts {
            for w in part.windows(2) {
                let steps = 3;
                for s in 0..=steps {
                    let t = s as f64 / steps as f64;
                    let x = w[0].0 + (w[1].0 - w[0].0) * t;
                    let y = w[0].1 + (w[1].1 - w[0].1) * t;
                    let Some((mut r, mut c)) = to_cell(x, y) else { continue };
                    let max_cells = (max_dist / cx.max(cy)).ceil() as usize;
                    let mut hit = None;
                    for _ in 0..max_cells {
                        if let Some(ids) = toe_cells.get(&(r * gr.width + c)) {
                            if let Some(&id) = ids.iter().find(|&&id| id != bi) {
                                hit = Some(id);
                                break;
                            }
                        }
                        let dir = d[r * gr.width + c];
                        if dir < 0 {
                            break;
                        }
                        let (dr, dc) = OFF[dir as usize];
                        let nr = r as isize + dr;
                        let nc = c as isize + dc;
                        if nr < 0 || nr >= gr.height as isize || nc < 0 || nc >= gr.width as isize {
                            break;
                        }
                        r = nr as usize;
                        c = nc as usize;
                    }
                    total += 1;
                    if let Some(id) = hit {
                        *votes.entry(id).or_insert(0) += 1;
                    }
                }
            }
        }
        if total == 0 {
            continue;
        }
        if let Some((&toe, &n)) = votes.iter().max_by_key(|&(_, &n)| n) {
            if n as f64 / total as f64 >= min_share {
                pair_of[bi] = Some(toe);
                paired_toes.insert(toe);
            }
        }
    }
    let mut top = Vec::new();
    let mut bottom = Vec::new();
    let mut orphans = Vec::new();
    let mut links = 0i64;
    for (li, l) in lines.iter().enumerate() {
        let coords: Vec<J> = l
            .parts
            .iter()
            .map(|p| json!(p.iter().map(|&(x, y)| json!([x, y])).collect::<Vec<_>>()))
            .collect();
        let geom = if coords.len() == 1 {
            json!({"type": "LineString", "coordinates": coords[0]})
        } else {
            json!({"type": "MultiLineString", "coordinates": coords})
        };
        let link = pair_of[li]
            .map(|t| (t + 1) as i64)
            .unwrap_or(-1);
        let feat = json!({
            "type": "Feature",
            "geometry": geom,
            "properties": {"kind": if l.brow { "brow" } else { "toe" }, "link": link}
        });
        if l.brow {
            if pair_of[li].is_some() {
                links += 1;
                top.push(feat);
            } else {
                orphans.push(feat);
            }
        } else {
            if paired_toes.contains(&li) {
                bottom.push(feat);
            } else {
                orphans.push(feat);
            }
        }
    }
    let n_top = top.len();
    let n_bottom = bottom.len();
    host.call("host.addLayer", json!({"name": t(lang, "Верх (бровки)", "Top (brows)"), "geojson": {"type": "FeatureCollection", "features": top}}))?;
    host.call("host.addLayer", json!({"name": t(lang, "Низ (подошвы)", "Bottom (toes)"), "geojson": {"type": "FeatureCollection", "features": bottom}}))?;
    if !orphans.is_empty() {
        host.call("host.addLayer", json!({"name": t(lang, "Непарные линии", "Orphan lines"), "geojson": {"type": "FeatureCollection", "features": orphans}}))?;
    }
    let lines_report = vec![
        pyfmt::format(t(lang, "Пар: %d, бровок: %d, подошв: %d.", "Pairs: %d, brows: %d, toes: %d."), &[Arg::from(links as f64), Arg::from(n_top as f64), Arg::from(n_bottom as f64)]),
        pyfmt::format(t(lang, "Непарных: %d.", "Orphans: %d."), &[Arg::from(orphans.len() as f64)]),
    ];
    Ok(json!({"report": lines_report.join("\n"), "messages": lines_report}))
}

/// 2.03 Topo2Raster: команда `topo2raster`.
pub fn topo2raster_cmd(host: &mut dyn Call, params: &J, lang: Lang) -> Result<J, String> {
    let contours = params["contours"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| t(lang, "Не выбран слой горизонталей.", "No contour layer is chosen.").to_string())?;
    let field = params["field"].as_str().unwrap_or("");
    if field.is_empty() {
        return Err(t(lang, "Не выбрано поле отметки.", "No elevation field is chosen.").to_string());
    }
    let cell = params["cell"].as_f64().unwrap_or(0.0);
    let iterations = params["iterations"].as_u64().unwrap_or(0).max(1) as usize;
    let min_drop = params["min_drop"].as_f64().unwrap_or(0.0);
    let depth_tol = params["depth_tol"].as_f64().unwrap_or(0.0);
    let dir = run_dir("topo2raster")?;
    let fc = host.call("host.features", json!({"layer": contours, "crs": "layer"}))?;
    let mut points: Vec<(f64, f64, f64, f64)> = Vec::new();
    let (mut xmin, mut ymin, mut xmax, mut ymax) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for f in fc["features"].as_array().into_iter().flatten() {
        let z = f["properties"][field]
            .as_f64()
            .or_else(|| f["properties"][field].as_str().and_then(|s| s.parse().ok()));
        let Some(z) = z else { continue };
        for part in line_parts(&f["geometry"]) {
            for (x, y) in part {
                points.push((x, y, z, 0.0));
                xmin = xmin.min(x);
                xmax = xmax.max(x);
                ymin = ymin.min(y);
                ymax = ymax.max(y);
            }
        }
    }
    if points.is_empty() {
        return Err(t(lang, "В горизонталях нет линий с числовой отметкой.", "The contours hold no lines with a numeric elevation.").to_string());
    }
    // Охват: по слою охвата, если задан, иначе по горизонталям.
    let mut extent = (xmin, ymin, xmax, ymax);
    if let Some(ext) = params["extent"].as_str().filter(|s| !s.is_empty()) {
        let efc = host.call("host.features", json!({"layer": ext, "crs": "layer"}))?;
        let (mut a, mut b, mut c, mut d) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for f in efc["features"].as_array().into_iter().flatten() {
            collect_bbox(&f["geometry"]["coordinates"], &mut a, &mut b, &mut c, &mut d);
        }
        if c > a && d > b {
            extent = (a, b, c, d);
        }
    }
    let diag = ((extent.2 - extent.0).powi(2) + (extent.3 - extent.1).powi(2)).sqrt();
    let cell = if cell > 0.0 { cell } else { (diag / 500.0).max(1e-6) };
    let streams = read_lines(host, params["streams"].as_str())?;
    let breaklines = read_lines(host, params["breaklines"].as_str())?;
    let lakes = read_polygons(host, params["lakes"].as_str())?;
    let (z, rows, cols, x0, y_top) = crate::topo_t2r::topo2raster(
        &points,
        &streams,
        &breaklines,
        &lakes,
        extent,
        cell,
        Some(iterations),
        Some(min_drop),
        Some(depth_tol),
    )?;
    let grid = geotiff::Grid::from_geotransform(cols, rows, x0, y_top - rows as f64 * cell, cell);
    let z32: Vec<f32> = z.iter().map(|&v| v as f32).collect();
    let path = dir.join(format!("{}.tif", t(lang, "Рельеф из горизонталей", "Relief from contours")));
    geotiff::write_like(&path, &grid, &z32, NODATA)?;
    add_raster(host, &path)?;
    let lines = vec![
        pyfmt::format(t(lang, "Горизонталей: %d вершин, сетка %d на %d, ячейка %.4g.", "Contours: %d vertices, grid %d by %d, cell %.4g."), &[Arg::from(points.len() as f64), Arg::from(cols as f64), Arg::from(rows as f64), Arg::from(cell)]),
    ];
    Ok(json!({"report": lines.join("\n"), "messages": lines}))
}

/// Прочитать линии слоя (пустой слой - пустой список).
fn read_lines(host: &mut dyn Call, layer: Option<&str>) -> Result<Vec<Vec<(f64, f64)>>, String> {
    let Some(name) = layer.filter(|s| !s.is_empty()) else { return Ok(Vec::new()) };
    let fc = host.call("host.features", json!({"layer": name, "crs": "layer"}))?;
    let mut out = Vec::new();
    for f in fc["features"].as_array().into_iter().flatten() {
        for part in line_parts(&f["geometry"]) {
            if part.len() >= 2 {
                out.push(part);
            }
        }
    }
    Ok(out)
}

/// Прочитать полигоны слоя как озёра.
fn read_polygons(host: &mut dyn Call, layer: Option<&str>) -> Result<Vec<crate::topo_t2r::Lake>, String> {
    let Some(name) = layer.filter(|s| !s.is_empty()) else { return Ok(Vec::new()) };
    let fc = host.call("host.features", json!({"layer": name, "crs": "layer"}))?;
    let ring = |coords: &J| -> Vec<(f64, f64)> {
        coords
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let a = p.as_array()?;
                Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
            })
            .collect()
    };
    let mut out = Vec::new();
    for f in fc["features"].as_array().into_iter().flatten() {
        let g = &f["geometry"];
        match g["type"].as_str().unwrap_or("") {
            "Polygon" => {
                let rings: Vec<Vec<(f64, f64)>> = g["coordinates"].as_array().into_iter().flatten().map(ring).filter(|r| r.len() >= 3).collect();
                if !rings.is_empty() {
                    out.push(crate::topo_t2r::Lake::new(rings));
                }
            }
            "MultiPolygon" => {
                for poly in g["coordinates"].as_array().into_iter().flatten() {
                    let rings: Vec<Vec<(f64, f64)>> = poly.as_array().into_iter().flatten().map(ring).filter(|r| r.len() >= 3).collect();
                    if !rings.is_empty() {
                        out.push(crate::topo_t2r::Lake::new(rings));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_like_numpy() {
        assert_eq!(median_f32(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median_f32(&mut [4.0, 1.0, 2.0, 3.0]), 2.5);
    }

    #[test]
    fn normal_cdf_is_symmetric() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-6);
        assert!((normal_cdf(1.96) - 0.975).abs() < 1e-3);
        assert!((normal_cdf(-1.96) - 0.025).abs() < 1e-3);
    }
}
