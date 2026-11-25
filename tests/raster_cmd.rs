// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Растровая команда 2.05 «Уклон и экспозиция» против модуля.
//!
//! RustGIS подменён: `share.exportRaster` кладёт вырезку ЦМР
//! `tests/data/dem_small.tif` по указанному пути, `layer.addData`
//! запоминает добавленные файлы. Выходы сверяются бит в бит с
//! `topo_surface.slope_aspect` модуля (`tools/gen_slope_ref.py`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use isoliner_rustgis::{geotiff, raster_cmd};
use rustgis_kit::{Call, Lang};
use serde_json::{Value as J, json};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

#[derive(Default)]
struct Fake {
    added: Vec<PathBuf>,
    exported: Vec<String>,
}

impl Call for Fake {
    fn call(&mut self, op: &str, params: J) -> Result<J, String> {
        match op {
            "share.exportRaster" => {
                self.exported
                    .push(params["layer"].as_str().unwrap().to_string());
                let to = params["path"].as_str().unwrap();
                std::fs::copy(data("dem_small.tif"), to).map_err(|e| e.to_string())?;
                Ok(json!({"path": to}))
            }
            "host.features" => {
                let layer = params["layer"].as_str().unwrap();
                match layer {
                    "wells" => Ok(plane_points_geojson()),
                    "bounds" => Ok(bounds_geojson()),
                    _ => Err(format!("unexpected layer {layer}")),
                }
            }
            "layer.addData" => {
                self.added
                    .push(PathBuf::from(params["path"].as_str().unwrap()));
                Ok(json!({"layer": self.added.len() + 1}))
            }
            _ => Err(format!("unexpected {op}")),
        }
    }
}

fn plane_points_geojson() -> J {
    let mut feats = Vec::new();
    for k in 0..9 {
        let x = 5.0 + f64::from(k % 3) * 50.0;
        let y = 5.0 + f64::from(k / 3) * 40.0;
        let z = 2.0 * x - y + 7.0;
        feats.push(json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [x, y]},
            "properties": {"z": z}
        }));
    }
    json!({"type": "FeatureCollection", "features": feats})
}

fn bounds_geojson() -> J {
    json!({
        "type": "FeatureCollection",
        "features": [{
            "type": "Feature",
            "geometry": {
                "type": "Polygon",
                "coordinates": [[[0.0, 0.0], [120.0, 0.0], [120.0, 100.0], [0.0, 100.0], [0.0, 0.0]]]
            },
            "properties": {}
        }]
    })
}

fn f32s(name: &str) -> Vec<f32> {
    std::fs::read(data(name))
        .unwrap()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

#[test]
fn slope_aspect_matches_module_bit_for_bit() {
    let mut host = Fake::default();
    let out = raster_cmd::slope_aspect(&mut host, &json!({"input": "dem"}), Lang::En).unwrap();
    assert_eq!(host.exported, ["dem"]);
    assert_eq!(host.added.len(), 2);
    let input = geotiff::read(&data("dem_small.tif")).unwrap();
    for (path, refname) in host
        .added
        .iter()
        .zip(["dem_small_slope.f32", "dem_small_aspect.f32"])
    {
        let g = geotiff::read(path).unwrap();
        assert_eq!((g.width, g.height), (input.width, input.height));
        assert_eq!(g.cell, input.cell);
        assert_eq!(g.nodata, Some(-9999.0));
        let want = f32s(refname);
        assert_eq!(g.z.len(), want.len());
        for (i, (a, b)) in g.z.iter().zip(&want).enumerate() {
            assert_eq!(a.to_bits(), f64::from(*b).to_bits(), "{refname}[{i}]");
        }
    }
    // пропуски входа и их соседи без данных, как в модуле
    let nd = f32s("dem_small_slope.f32")
        .iter()
        .filter(|&&v| v == -9999.0)
        .count();
    assert!(nd > 406, "{nd}");
    let report = out["report"].as_str().unwrap();
    assert!(report.starts_with("Slope: from "), "{report}");
    assert!(host.added[0].ends_with("Slope, degrees.tif"));
}

#[test]
fn minimum_curvature_reproduces_plane() {
    let mut host = Fake::default();
    let out = raster_cmd::minimum_curvature(
        &mut host,
        &json!({
            "input": "wells",
            "zfield": "z",
            "extent": "bounds",
            "cell_size": 10.0,
            "max_residual": 1e-9
        }),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.added.len(), 1);
    let g = geotiff::read(&host.added[0]).unwrap();
    assert_eq!((g.width, g.height), (12, 10));
    let frame = isoliner_rustgis::mincurv::Frame {
        xmin: 0.0,
        ymin: 0.0,
        cell: 10.0,
        nx: 12,
        ny: 10,
    };
    let grid = isoliner_rustgis::mincurv::Grid {
        nx: g.width,
        ny: g.height,
        z: g.z.clone(),
    };
    let v = isoliner_rustgis::mincurv::sample_bilinear(&grid, &frame, 65.0, 35.0);
    assert!((v - 102.0).abs() < 1e-3, "{v}");
    let report = out["report"].as_str().unwrap();
    assert!(report.starts_with("Grid "), "{report}");
    assert!(host.added[0].to_string_lossy().contains("Minimum curvature"));
}

#[test]
fn kriging_2d_reproduces_plane() {
    let mut host = Fake::default();
    let out = raster_cmd::kriging_2d(
        &mut host,
        &json!({
            "input": "wells",
            "zfield": "z",
            "extent": "bounds",
            "cell_size": 10.0,
            "ktype": "Ordinary (OK)",
            "s1_model": "Spherical",
            "s1_sill": 1.0,
            "s1_range": 1000.0,
            "detrend": true,
            "detrend_deg": "1 (plane)"
        }),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.added.len(), 1);
    let g = geotiff::read(&host.added[0]).unwrap();
    assert_eq!((g.width, g.height), (12, 10));
    let frame = isoliner_rustgis::mincurv::Frame {
        xmin: 0.0,
        ymin: 0.0,
        cell: 10.0,
        nx: 12,
        ny: 10,
    };
    let grid = isoliner_rustgis::mincurv::Grid {
        nx: g.width,
        ny: g.height,
        z: g.z.clone(),
    };
    let v = isoliner_rustgis::mincurv::sample_bilinear(&grid, &frame, 65.0, 35.0);
    assert!((v - 102.0).abs() < 1e-2, "{v}");
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Grid "), "{report}");
    assert!(host.added[0].to_string_lossy().contains("2D Kriging"));
}

#[test]
fn slope_aspect_needs_a_layer() {
    let mut host = Fake::default();
    let e = raster_cmd::slope_aspect(&mut host, &json!({}), Lang::Ru).unwrap_err();
    assert_eq!(e, "Не выбрана входная ЦМР.");
    assert!(host.exported.is_empty());
}
