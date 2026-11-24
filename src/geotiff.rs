// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Чтение и запись одноканального GeoTIFF для растровых команд.
//!
//! RustGIS выгружает растровый слой командой `share.exportRaster` в
//! GeoTIFF float32 (Deflate, предиктор 3), выход добавляется обратно
//! `layer.addData`. Привязка не разбирается, а переносится тегами
//! GeoTIFF как есть: выходной грид совпадает с входным ячейка в ячейку.
//! Из тегов берётся только размер ячейки для расчёта.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use tiff::decoder::{Decoder, DecodingResult};
use tiff::encoder::{Compression, DeflateLevel, TiffEncoder, colortype};
use tiff::tags::Tag;

/// Теги привязки GeoTIFF, переносимые на выход.
const GEO_F64: [Tag; 3] = [
    Tag::ModelPixelScaleTag,
    Tag::ModelTiepointTag,
    Tag::ModelTransformationTag,
];

/// Растр одного канала с привязкой входного файла.
#[derive(Debug, Clone)]
pub struct Grid {
    pub width: usize,
    pub height: usize,
    /// Значения по строкам сверху вниз; nodata и NaN - `None` в [`Grid::mask`].
    pub z: Vec<f64>,
    /// `true` - ячейка без данных.
    pub mask: Vec<bool>,
    /// Размер ячейки по X и по Y, единицы системы растра.
    pub cell: (f64, f64),
    pub nodata: Option<f64>,
    geo_f64: Vec<(Tag, Vec<f64>)>,
    geo_keys: Option<Vec<u16>>,
    geo_doubles: Option<Vec<f64>>,
    geo_ascii: Option<String>,
}

impl Grid {
    /// Новый растр с привязкой: левый нижний угол `(xmin, ymin)`,
    /// квадратная ячейка `cell`. Верх севера - `ymin + height*cell`.
    pub fn from_geotransform(width: usize, height: usize, xmin: f64, ymin: f64, cell: f64) -> Self {
        let size = width * height;
        Self {
            width,
            height,
            z: vec![0.0; size],
            mask: vec![false; size],
            cell: (cell, cell),
            nodata: None,
            geo_f64: vec![
                (Tag::ModelPixelScaleTag, vec![cell, cell, 0.0]),
                (
                    Tag::ModelTiepointTag,
                    vec![0.0, 0.0, 0.0, xmin, ymin + height as f64 * cell, 0.0],
                ),
            ],
            geo_keys: None,
            geo_doubles: None,
            geo_ascii: None,
        }
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Прочитать первый канал GeoTIFF.
pub fn read(path: &Path) -> Result<Grid, String> {
    let file = File::open(path).map_err(err)?;
    let mut d = Decoder::new(BufReader::new(file)).map_err(err)?;
    let (w, h) = d.dimensions().map_err(err)?;
    let (w, h) = (w as usize, h as usize);
    let spp = d
        .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)
        .map_err(err)?
        .unwrap_or(1)
        .max(1) as usize;
    let mut geo_f64 = Vec::new();
    for t in GEO_F64 {
        if d.find_tag(t).map_err(err)?.is_some() {
            geo_f64.push((t, d.get_tag_f64_vec(t).map_err(err)?));
        }
    }
    let geo_keys = match d.find_tag(Tag::GeoKeyDirectoryTag).map_err(err)? {
        Some(_) => Some(d.get_tag_u16_vec(Tag::GeoKeyDirectoryTag).map_err(err)?),
        None => None,
    };
    let geo_doubles = match d.find_tag(Tag::GeoDoubleParamsTag).map_err(err)? {
        Some(_) => Some(d.get_tag_f64_vec(Tag::GeoDoubleParamsTag).map_err(err)?),
        None => None,
    };
    let geo_ascii = match d.find_tag(Tag::GeoAsciiParamsTag).map_err(err)? {
        Some(_) => Some(d.get_tag_ascii_string(Tag::GeoAsciiParamsTag).map_err(err)?),
        None => None,
    };
    let nodata = match d.find_tag(Tag::GdalNodata).map_err(err)? {
        Some(_) => d
            .get_tag_ascii_string(Tag::GdalNodata)
            .map_err(err)?
            .trim_matches(char::from(0))
            .trim()
            .parse::<f64>()
            .ok(),
        None => None,
    };
    let all: Vec<f64> = match d.read_image().map_err(err)? {
        DecodingResult::F32(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::F64(v) => v,
        DecodingResult::U8(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::U16(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::U32(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::I8(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::I16(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::I32(v) => v.into_iter().map(f64::from).collect(),
        _ => return Err("unsupported GeoTIFF sample type".into()),
    };
    if all.len() < w * h * spp {
        return Err("GeoTIFF holds fewer pixels than its size".into());
    }
    let z: Vec<f64> = all.iter().step_by(spp).take(w * h).copied().collect();
    // nodata как у GDAL: ячейка равна значению nodata, иначе не число
    let mask = z
        .iter()
        .map(|&v| match nodata {
            Some(nd) => v == nd || (nd.is_nan() && v.is_nan()),
            None => !v.is_finite(),
        })
        .collect();
    let cell = cell_size(&geo_f64).ok_or("GeoTIFF has no pixel size")?;
    Ok(Grid {
        width: w,
        height: h,
        z,
        mask,
        cell,
        nodata,
        geo_f64,
        geo_keys,
        geo_doubles,
        geo_ascii,
    })
}

/// Размер ячейки по ModelPixelScale, иначе по матрице ModelTransformation.
fn cell_size(tags: &[(Tag, Vec<f64>)]) -> Option<(f64, f64)> {
    let get = |t: Tag| tags.iter().find(|(k, _)| *k == t).map(|(_, v)| v);
    if let Some(s) = get(Tag::ModelPixelScaleTag).filter(|s| s.len() >= 2) {
        return Some((s[0].abs(), s[1].abs()));
    }
    let m = get(Tag::ModelTransformationTag).filter(|m| m.len() >= 8)?;
    Some((m[0].hypot(m[4]), m[1].hypot(m[5])))
}

/// Записать массив той же сетки, что у `like`, в GeoTIFF float32 с
/// привязкой `like` и значением nodata `nodata`.
pub fn write_like(path: &Path, like: &Grid, data: &[f32], nodata: f64) -> Result<(), String> {
    if data.len() != like.width * like.height {
        return Err("raster size does not match its grid".into());
    }
    let file = File::create(path).map_err(err)?;
    let mut enc = TiffEncoder::new(BufWriter::new(file))
        .map_err(err)?
        .with_compression(Compression::Deflate(DeflateLevel::Balanced));
    let mut img = enc
        .new_image::<colortype::Gray32Float>(like.width as u32, like.height as u32)
        .map_err(err)?;
    {
        let e = img.encoder();
        for (t, v) in &like.geo_f64 {
            e.write_tag(*t, &v[..]).map_err(err)?;
        }
        if let Some(k) = &like.geo_keys {
            e.write_tag(Tag::GeoKeyDirectoryTag, &k[..]).map_err(err)?;
        }
        if let Some(v) = &like.geo_doubles {
            e.write_tag(Tag::GeoDoubleParamsTag, &v[..]).map_err(err)?;
        }
        if let Some(s) = &like.geo_ascii {
            e.write_tag(Tag::GeoAsciiParamsTag, s.as_str()).map_err(err)?;
        }
        e.write_tag(Tag::GdalNodata, format_nodata(nodata).as_str())
            .map_err(err)?;
    }
    img.write_data(data).map_err(err)
}

/// Значение nodata текстом, как пишет GDAL: целое без точки.
fn format_nodata(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Квадратная ячейка с относительным допуском `tol` (`topo_flow.square_cell`
/// модуля): среднее из размеров по X и Y, иначе оба размера для отказа.
pub fn square_cell(cell: (f64, f64), tol: f64) -> Result<f64, (f64, f64)> {
    let (cx, cy) = cell;
    if cx <= 0.0 || cy <= 0.0 {
        return Err(cell);
    }
    let rel = (cx - cy).abs() / cx.max(cy);
    if rel > tol {
        return Err(cell);
    }
    Ok(0.5 * (cx + cy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_cell_tolerance() {
        assert_eq!(square_cell((15.0, 15.0), 1e-3), Ok(15.0));
        // растр 0.1999974 на 0.2000043 м проходит, как в модуле с 5.13.45
        assert!(square_cell((0.1999974, 0.2000043), 1e-3).is_ok());
        assert!(square_cell((10.0, 15.0), 1e-3).is_err());
        assert_eq!(format_nodata(-9999.0), "-9999");
    }
}
