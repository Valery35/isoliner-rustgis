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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_like_numpy() {
        assert_eq!(median_f32(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median_f32(&mut [4.0, 1.0, 2.0, 3.0]), 2.5);
    }
}
