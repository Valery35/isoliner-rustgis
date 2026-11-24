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

use crate::geotiff;
use crate::mincurv::{self, Frame, Params};
use crate::points;
use crate::pyfmt::{self, Arg};
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_like_numpy() {
        assert_eq!(median_f32(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median_f32(&mut [4.0, 1.0, 2.0, 3.0]), 2.5);
    }
}
