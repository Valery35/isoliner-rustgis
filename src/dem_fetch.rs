// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 2.01–2.02 Загрузка ЦМР по рамке: GLO-30, GEDTM30, Mapzen Terrain Tiles.
//! Порт `dem_glo30.py` модуля QGIS.
//!
//! Реализована сетевая часть и подготовка плиток; перепроецирование
//! оставлено на сторону RustGIS/QGIS при добавлении слоя.

use std::fs;
use std::path::{Path, PathBuf};

const BUCKET_URL: &str = "https://copernicus-dem-30m.s3.amazonaws.com";
#[allow(dead_code)]
const DEFAULT_CELL: f64 = 30.0;
#[allow(dead_code)]
const DEFAULT_MAX_TILES: usize = 25;

pub const SOURCE_GLO30: &str = "glo30";
pub const SOURCE_GEDTM30: &str = "gedtm30";
pub const SOURCE_MAPZEN: &str = "mapzen";

const MAPZEN_URL: &str = "https://s3.amazonaws.com/elevation-tiles-prod/geotiff/{z}/{x}/{y}.tif";
const MAPZEN_MAX_ZOOM: i32 = 14;
const MAPZEN_TILE_PX: i32 = 512;
#[allow(dead_code)]
const MAPZEN_MAX_XYZ: usize = 256;
const EARTH_CIRC: f64 = 2.0 * std::f64::consts::PI * 6378137.0;

/// Ошибка загрузки ЦМР.
#[derive(Debug, Clone)]
pub struct DemError(pub String);

impl std::fmt::Display for DemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DemError {}

fn err(s: impl Into<String>) -> DemError {
    DemError(s.into())
}

/// Имя плитки GLO-30 по юго-западному углу градусной ячейки.
pub fn glo30_tile_name(lat: f64, lon: f64) -> String {
    let f_lat = lat.floor() as i32;
    let f_lon = lon.floor() as i32;
    let ns = if f_lat >= 0 {
        format!("N{:02}", f_lat)
    } else {
        format!("S{:02}", -f_lat)
    };
    let ew = if f_lon >= 0 {
        format!("E{:03}", f_lon)
    } else {
        format!("W{:03}", -f_lon)
    };
    format!("Copernicus_DSM_COG_10_{}_00_{}_00_DEM", ns, ew)
}

/// URL плитки GLO-30.
pub fn glo30_url(name: &str) -> String {
    format!("{}/{}/{}.tif", BUCKET_URL, name, name)
}

/// Размер пикселя плитки Mapzen уровня z на широте lat, м.
pub fn mapzen_ground_pixel(z: i32, lat: f64) -> f64 {
    let n = 1i64 << z.max(0) as u32;
    EARTH_CIRC / (MAPZEN_TILE_PX as f64 * n as f64) * lat.to_radians().cos().max(1e-6)
}

/// Уровень Mapzen под заданную ячейку.
pub fn mapzen_zoom_for(cell: f64, lat: f64) -> i32 {
    let cell = cell.max(1e-6);
    let coslat = lat.to_radians().cos().max(1e-6);
    let z = ((EARTH_CIRC * coslat) / (MAPZEN_TILE_PX as f64 * cell))
        .log2()
        .ceil();
    (z as i32).clamp(0, MAPZEN_MAX_ZOOM)
}

/// Номер плитки Mapzen z/x/y (XYZ, y вниз от севера).
pub fn mapzen_xy(lon: f64, lat: f64, z: i32) -> (i32, i32) {
    let n = 1i64 << z.max(0) as u32;
    let lat = lat.clamp(-85.05112878, 85.05112878);
    let x = ((lon + 180.0) / 360.0 * n as f64).floor() as i32;
    let r = lat.to_radians();
    let y = ((1.0 - r.sin().asinh() / std::f64::consts::PI) / 2.0 * n as f64).floor() as i32;
    (x.clamp(0, n as i32 - 1), y.clamp(0, n as i32 - 1))
}

/// URL плитки Mapzen.
pub fn mapzen_url(z: i32, x: i32, y: i32) -> String {
    MAPZEN_URL
        .replace("{z}", &z.to_string())
        .replace("{x}", &x.to_string())
        .replace("{y}", &y.to_string())
}

/// EPSG зоны UTM по точке.
pub fn utm_epsg_for(lon: f64, lat: f64) -> i32 {
    let zone = ((lon + 180.0) / 6.0).floor() as i32 + 1;
    let zone = zone.clamp(1, 60);
    if lat >= 0.0 { 32600 + zone } else { 32700 + zone }
}

/// Плитки GLO-30, покрывающие рамку в EPSG:4326.
pub fn tiles_for_bbox(
    lon_min: f64,
    lat_min: f64,
    lon_max: f64,
    lat_max: f64,
    max_tiles: usize,
) -> Result<Vec<String>, DemError> {
    if lon_max <= lon_min || lat_max <= lat_min {
        return Err(err("Пустая рамка: проверьте координаты."));
    }
    if lon_min < -180.0 || lon_max > 180.0 || lat_min < -90.0 || lat_max > 90.0 {
        return Err(err(
            "Рамка выходит за пределы EPSG:4326. Рамка через антимеридиан не поддерживается.",
        ));
    }
    let eps = 1e-9;
    let lon_a = (lon_min).floor() as i32;
    let lon_b = (lon_max - eps).floor() as i32;
    let lat_a = (lat_min).floor() as i32;
    let lat_b = (lat_max - eps).floor() as i32;
    let n_tiles = ((lon_b - lon_a + 1) * (lat_b - lat_a + 1)) as usize;
    if n_tiles > max_tiles {
        return Err(err(format!(
            "Рамка требует {} плиток при пределе {}. Уменьшите рамку или поднимите предел.",
            n_tiles, max_tiles
        )));
    }
    let mut names = Vec::with_capacity(n_tiles);
    for la in lat_a..=lat_b {
        for lo in lon_a..=lon_b {
            names.push(glo30_tile_name(la as f64, lo as f64));
        }
    }
    Ok(names)
}

/// Плитки Mapzen, покрывающие рамку.
pub fn mapzen_tiles(
    lon_min: f64,
    lat_min: f64,
    lon_max: f64,
    lat_max: f64,
    z: i32,
) -> Vec<(i32, i32, i32)> {
    let (x0, y0) = mapzen_xy(lon_min, lat_max, z);
    let (x1, y1) = mapzen_xy(lon_max, lat_min, z);
    let mut out = Vec::with_capacity(((y1 - y0 + 1) * (x1 - x0 + 1)) as usize);
    for y in y0..=y1 {
        for x in x0..=x1 {
            out.push((z, x, y));
        }
    }
    out
}

/// Уровень и плитки Mapzen под рамку и ячейку.
pub fn mapzen_plan(
    lon_min: f64,
    lat_min: f64,
    lon_max: f64,
    lat_max: f64,
    cell: f64,
    max_xyz: usize,
) -> (i32, Vec<(i32, i32, i32)>, i32) {
    let lat_c = (lat_min + lat_max) / 2.0;
    let z_want = mapzen_zoom_for(cell, lat_c);
    let mut z = z_want;
    let mut tiles = mapzen_tiles(lon_min, lat_min, lon_max, lat_max, z);
    while tiles.len() > max_xyz && z > 0 {
        z -= 1;
        tiles = mapzen_tiles(lon_min, lat_min, lon_max, lat_max, z);
    }
    (z, tiles, z_want)
}

/// Скачать URL в файл через системный curl. При ошибке возвращает DemError.
pub fn fetch_url_to_file(url: &str, path: &Path) -> Result<(), DemError> {
    let parent = path.parent().ok_or_else(|| err("Некорректный путь для сохранения."))?;
    fs::create_dir_all(parent).map_err(|e| err(format!("Не удалось создать папку: {}", e)))?;
    let out = std::process::Command::new("curl")
        .args(["-sSL", "-f", "-o", path.to_str().unwrap_or(""), url])
        .output()
        .map_err(|e| err(format!("Не удалось запустить curl: {}", e)))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(err(format!(
            "curl завершился с ошибкой для {}: {}",
            url, stderr
        )));
    }
    Ok(())
}

/// Скачать плитки Mapzen Terrain Tiles в папку.
/// Возвращает пути к сохранённым файлам.
pub fn fetch_dem_mapzen(
    lon_min: f64,
    lat_min: f64,
    lon_max: f64,
    lat_max: f64,
    cell: f64,
    out_dir: &Path,
    max_xyz: usize,
) -> Result<Vec<PathBuf>, DemError> {
    let (_, tiles, _) = mapzen_plan(lon_min, lat_min, lon_max, lat_max, cell, max_xyz);
    if tiles.is_empty() {
        return Err(err("Рамка не даёт ни одной плитки Mapzen."));
    }
    let mut paths = Vec::with_capacity(tiles.len());
    let n_tiles = tiles.len();
    for (idx, &(z, x, y)) in tiles.iter().enumerate() {
        let url = mapzen_url(z, x, y);
        let path = out_dir.join(format!("mapzen_{}_{}_{}.tif", z, x, y));
        match fetch_url_to_file(&url, &path) {
            Ok(()) => paths.push(path),
            Err(e) => {
                // Mapzen плитки могут отсутствовать за полярным кругом; пропускаем.
                if paths.is_empty() && idx == n_tiles - 1 {
                    return Err(e);
                }
            }
        }
    }
    if paths.is_empty() {
        return Err(err("Не удалось скачать ни одной плитки Mapzen."));
    }
    Ok(paths)
}

/// Скачать плитки GLO-30 в папку. Океанские плитки отсутствуют — пропускаем.
pub fn fetch_dem_glo30(
    lon_min: f64,
    lat_min: f64,
    lon_max: f64,
    lat_max: f64,
    out_dir: &Path,
    max_tiles: usize,
) -> Result<Vec<PathBuf>, DemError> {
    let names = tiles_for_bbox(lon_min, lat_min, lon_max, lat_max, max_tiles)?;
    let mut paths = Vec::with_capacity(names.len());
    for name in names {
        let url = glo30_url(&name);
        let path = out_dir.join(format!("{}.tif", name));
        match fetch_url_to_file(&url, &path) {
            Ok(()) => paths.push(path),
            Err(_) => {
                // Пропускаем отсутствующие плитки (океан).
            }
        }
    }
    if paths.is_empty() {
        return Err(err(
            "Не удалось скачать ни одной плитки GLO-30. Проверьте соединение или рамка в океане.",
        ));
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glo30_tile_name_examples() {
        assert_eq!(glo30_tile_name(45.2, 37.9), "Copernicus_DSM_COG_10_N45_00_E037_00_DEM");
        assert_eq!(glo30_tile_name(-1.5, -72.3), "Copernicus_DSM_COG_10_S02_00_W073_00_DEM");
    }

    #[test]
    fn mapzen_zoom_matches_cell() {
        // На экваторе при ячейке 30 м нужен уровень с пикселем <= 30 м.
        let z = mapzen_zoom_for(30.0, 0.0);
        assert!(mapzen_ground_pixel(z, 0.0) <= 30.0);
        assert!(z <= MAPZEN_MAX_ZOOM);
    }

    #[test]
    fn mapzen_xy_monotonic() {
        let (x0, y0) = mapzen_xy(0.0, 80.0, 5);
        let (x1, y1) = mapzen_xy(10.0, 70.0, 5);
        assert!(x1 >= x0);
        assert!(y1 >= y0);
    }

    #[test]
    fn tiles_for_small_bbox() {
        let names = tiles_for_bbox(37.0, 45.0, 37.5, 45.5, 25).unwrap();
        assert_eq!(names.len(), 1);
        assert!(names[0].contains("N45_00_E037_00"));
    }

    #[test]
    fn mapzen_tiles_count() {
        let t = mapzen_tiles(37.0, 55.0, 37.5, 55.5, 10);
        assert!(!t.is_empty());
    }

    #[test]
    fn utm_epsg_northern() {
        assert_eq!(utm_epsg_for(37.5, 55.0), 32637);
    }
}
