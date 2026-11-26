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
    rasters: std::collections::HashMap<String, PathBuf>,
    layers: Vec<(String, J)>,
}

impl Call for Fake {
    fn call(&mut self, op: &str, params: J) -> Result<J, String> {
        match op {
            "share.exportRaster" => {
                let layer = params["layer"].as_str().unwrap().to_string();
                self.exported.push(layer.clone());
                let to = params["path"].as_str().unwrap();
                let src = self.rasters.get(&layer).cloned().unwrap_or_else(|| data("dem_small.tif"));
                std::fs::copy(src, to).map_err(|e| e.to_string())?;
                Ok(json!({"path": to}))
            }
            "host.features" => {
                let layer = params["layer"].as_str().unwrap();
                match layer {
                    "wells" => Ok(plane_points_geojson()),
                    "bounds" => Ok(bounds_geojson()),
                    "contours" => Ok(json!({
                        "type": "FeatureCollection",
                        "features": [{
                            "type": "Feature",
                            "geometry": {
                                "type": "LineString",
                                "coordinates": [[0.5, 1.5], [4.5, 1.5]]
                            },
                            "properties": {"elev": 7.0, "hold": 0}
                        }]
                    })),
                    "gauges" => Ok(json!({
                        "type": "FeatureCollection",
                        "features": [{
                            "type": "Feature",
                            "geometry": {"type": "Point", "coordinates": [5.5, 0.5]},
                            "properties": {}
                        }]
                    })),
                    "ditches" => Ok(json!({
                        "type": "FeatureCollection",
                        "features": [{
                            "type": "Feature",
                            "geometry": {
                                "type": "LineString",
                                "coordinates": [[0.5, 0.5], [5.5, 0.5]]
                            },
                            "properties": {}
                        }]
                    })),
                    "breaks" => Ok(json!({
                        "type": "FeatureCollection",
                        "features": [
                            {"type":"Feature","geometry":{"type":"LineString","coordinates":[[0.5,4.5],[5.5,4.5]]},"properties":{"kind":"brow"}},
                            {"type":"Feature","geometry":{"type":"LineString","coordinates":[[0.5,2.5],[5.5,2.5]]},"properties":{"kind":"toe"}}
                        ]
                    })),
                    "small_bounds" => Ok(json!({
                        "type": "FeatureCollection",
                        "features": [{
                            "type": "Feature",
                            "geometry": {
                                "type": "Polygon",
                                "coordinates": [[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]]
                            },
                            "properties": {}
                        }]
                    })),
                    _ => Err(format!("unexpected layer {layer}")),
                }
            }
            "layer.addData" => {
                self.added
                    .push(PathBuf::from(params["path"].as_str().unwrap()));
                Ok(json!({"layer": self.added.len() + 1}))
            }
            "host.addLayer" => {
                let name = params["name"].as_str().unwrap_or("").to_string();
                self.layers.push((name, params["geojson"].clone()));
                Ok(json!({"layer": self.layers.len()}))
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

fn write_grid(path: &Path, z: &[f32], nx: usize, ny: usize, cell: f64, xmin: f64, ymin: f64) {
    let grid = geotiff::Grid::from_geotransform(nx, ny, xmin, ymin, cell);
    geotiff::write_like(path, &grid, z, -9999.0).unwrap();
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
fn probability_threshold_computes_cdf() {
    let mut host = Fake::default();
    let dir = std::env::temp_dir().join(format!("isoprobtest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let est_path = dir.join("est.tif");
    let se_path = dir.join("se.tif");
    // estimate = 10 всюду, stderr = 1 всюду, threshold = 11
    // above: z = (10-11)/1 = -1 -> P(Z > -1) = 0.8413
    // below: z = (11-10)/1 = 1 -> P(Z < 1) = 0.8413
    write_grid(&est_path, &vec![10.0f32; 12 * 10], 12, 10, 10.0, 0.0, 0.0);
    write_grid(&se_path, &vec![1.0f32; 12 * 10], 12, 10, 10.0, 0.0, 0.0);
    host.rasters.insert("est".to_string(), est_path.clone());
    host.rasters.insert("se".to_string(), se_path.clone());

    let out = raster_cmd::probability_threshold(
        &mut host,
        &json!({
            "estimate": "est",
            "stderr": "se",
            "threshold": 9.0,
            "side": "Above threshold (P(Z > t))"
        }),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.added.len(), 1);
    let g = geotiff::read(&host.added[0]).unwrap();
    assert!((g.z[0] - 0.8413).abs() < 1e-3, "{}", g.z[0]);
    assert!(out["report"].as_str().unwrap().contains("Cells with probability"));
}

#[test]
fn flow_gradient_adds_two_rasters() {
    let mut host = Fake::default();
    let out = raster_cmd::flow_gradient(&mut host, &json!({"input": "dem"}), Lang::En).unwrap();
    assert_eq!(host.exported, ["dem"]);
    assert_eq!(host.added.len(), 2);
    let report = out["report"].as_str().unwrap();
    assert!(report.starts_with("Gradient: from "), "{report}");
    assert!(host.added[0].to_string_lossy().contains("Hydraulic gradient"));
    assert!(host.added[1].to_string_lossy().contains("Flow direction"));
}

#[test]
fn fill_depressions_raises_the_pit() {
    let dir = std::env::temp_dir().join(format!("isoliner-fill-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("pit.tif");
    // 5x5: border 10, interior 12, one pit in the centre.
    let mut z = vec![12.0f32; 25];
    for i in 0..5 {
        z[i] = 10.0;
        z[20 + i] = 10.0;
        z[i * 5] = 10.0;
        z[i * 5 + 4] = 10.0;
    }
    z[12] = 0.0;
    write_grid(&src, &z, 5, 5, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out =
        raster_cmd::fill_depressions(&mut host, &json!({"input": "dem"}), Lang::En).unwrap();
    assert_eq!(host.exported, ["dem"]);
    assert_eq!(host.added.len(), 1);
    let g = geotiff::read(&host.added[0]).unwrap();
    assert!((g.z[12] - 12.0).abs() < 1e-6, "pit = {}", g.z[12]);
    assert!((g.z[0] - 10.0).abs() < 1e-6, "border = {}", g.z[0]);
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Depressions filled: 1"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn flow_d8_drains_a_bowl() {
    let dir = std::env::temp_dir().join(format!("isoliner-d8-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("bowl.tif");
    // 5x5: border 10, inner ring 5, centre 0 - everything flows to the centre.
    let mut z = vec![10.0f32; 25];
    for r in 1..4 {
        for c in 1..4 {
            z[r * 5 + c] = 5.0;
        }
    }
    z[12] = 0.0;
    write_grid(&src, &z, 5, 5, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::flow_d8(
        &mut host,
        &json!({"input": "dem", "fill": false}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.exported, ["dem"]);
    assert_eq!(host.added.len(), 2);
    let d = geotiff::read(&host.added[0]).unwrap();
    let acc = geotiff::read(&host.added[1]).unwrap();
    // The centre has no lower neighbour: no flow, and every cell drains into it.
    assert_eq!(d.z[12], 0.0);
    assert!((acc.z[12] - 25.0).abs() < 1e-6, "acc = {}", acc.z[12]);
    // A border cell drains inward to the ring.
    assert_ne!(d.z[0], 0.0);
    assert_ne!(d.z[0], 255.0);
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Maximum accumulation"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn river_network_adds_lines() {
    let dir = std::env::temp_dir().join(format!("isoliner-rn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("slope.tif");
    // 6x6 plane sloping south-east: every cell drains to the bottom-right corner.
    let mut z = vec![0.0f32; 36];
    for r in 0..6 {
        for c in 0..6 {
            z[r * 6 + c] = 10.0 - (r as f32 + c as f32);
        }
    }
    write_grid(&src, &z, 6, 6, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::river_network(
        &mut host,
        &json!({"input": "dem", "threshold": 2.0, "fill": true}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.layers.len(), 1);
    let (name, fc) = &host.layers[0];
    assert_eq!(name, "River network");
    assert!(!fc["features"].as_array().unwrap().is_empty());
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Links:"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn peaks_pits_adds_two_layers() {
    let dir = std::env::temp_dir().join(format!("isoliner-pp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("extrema.tif");
    let mut z = vec![10.0f32; 25];
    z[12] = 20.0;
    z[6] = 0.0;
    write_grid(&src, &z, 5, 5, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::peaks_pits(
        &mut host,
        &json!({"input": "dem", "window": 3, "min_prominence": 1.0}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.layers.len(), 2);
    assert_eq!(host.layers[0].0, "Peaks");
    assert_eq!(host.layers[1].0, "Pits");
    assert_eq!(host.layers[0].1["features"].as_array().unwrap().len(), 1);
    assert_eq!(host.layers[1].1["features"].as_array().unwrap().len(), 1);
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Peaks: 1, pits: 1"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn basins_labels_the_plane() {
    let dir = std::env::temp_dir().join(format!("isoliner-basins-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("slope.tif");
    let mut z = vec![0.0f32; 36];
    for r in 0..6 {
        for c in 0..6 {
            z[r * 6 + c] = 10.0 - (r as f32 + c as f32);
        }
    }
    write_grid(&src, &z, 6, 6, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::basins(
        &mut host,
        &json!({"input": "dem", "threshold": 1.0, "fill": true}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.added.len(), 1);
    let g = geotiff::read(&host.added[0]).unwrap();
    assert!(g.z.iter().all(|&v| v == 1.0), "labels: {:?}", g.z);
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Basins: 1"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn terrace_smooth_reduces_the_step() {
    let dir = std::env::temp_dir().join(format!("isoliner-ts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("steps.tif");
    let mut z = vec![10.0f32; 25];
    z[12] = 20.0;
    write_grid(&src, &z, 5, 5, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::terrace_smooth(
        &mut host,
        &json!({"input": "dem", "interval": 4.0, "iters": 10, "limit": 0.5}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.added.len(), 1);
    let g = geotiff::read(&host.added[0]).unwrap();
    assert!(g.z[12] < 20.0, "step stayed {}", g.z[12]);
    assert!(g.z[12] >= 18.0 - 1e-6, "step dropped to {}", g.z[12]);
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Interval"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn contour_residual_reads_the_dem() {
    let dir = std::env::temp_dir().join(format!("isoliner-cr-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("dem.tif");
    let mut z = vec![0.0f32; 25];
    for r in 0..5 {
        for c in 0..5 {
            z[r * 5 + c] = 10.0 - r as f32;
        }
    }
    write_grid(&src, &z, 5, 5, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::contour_residual(
        &mut host,
        &json!({
            "contours": "contours",
            "field": "elev",
            "dem": "dem",
            "step": 0.0,
            "sampling": "Bilinear",
            "interval": 2.0
        }),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.layers.len(), 1);
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Residuals:"), "{report}");
    assert!(report.contains("mean 0,"), "{report}");
    // The hold field splits the report into building and validation.
    assert!(report.contains("building:"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn terracing_check_outputs_curvature() {
    let dir = std::env::temp_dir().join(format!("isoliner-tc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("dem.tif");
    let rows = 10;
    let cols = 10;
    let mut z = vec![0.0f32; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            z[r * cols + c] = r as f32 * 0.1;
        }
    }
    write_grid(&src, &z, rows, cols, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::terracing_check(
        &mut host,
        &json!({
            "input": "dem",
            "interval": 2.0,
            "base": 0.0,
            "band": 0.1,
            "min_drop": 0.01
        }),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.added.len(), 1);
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Curvature:"), "{report}");
    assert!(report.contains("Verdict:"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn gauge_report_adds_a_point() {
    let dir = std::env::temp_dir().join(format!("isoliner-gr-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("dem.tif");
    let mut z = vec![0.0f32; 36];
    for r in 0..6 {
        for c in 0..6 {
            z[r * 6 + c] = 10.0 - (r + c) as f32;
        }
    }
    write_grid(&src, &z, 6, 6, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::gauge_report(
        &mut host,
        &json!({"input": "dem", "gauges": "gauges", "snap": 2.0, "fill": true}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.layers.len(), 1);
    let (name, fc) = &host.layers[0];
    assert_eq!(name, "Gauges");
    assert_eq!(fc["features"].as_array().unwrap().len(), 1);
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Gauge 1:"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ditch_catchment_adds_a_point() {
    let dir = std::env::temp_dir().join(format!("isoliner-dc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("dem.tif");
    let mut z = vec![0.0f32; 36];
    for r in 0..6 {
        for c in 0..6 {
            z[r * 6 + c] = 10.0 - (r + c) as f32;
        }
    }
    write_grid(&src, &z, 6, 6, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::ditch_catchment(
        &mut host,
        &json!({"input": "dem", "lines": "ditches", "fill": true}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.layers.len(), 1);
    assert_eq!(host.layers[0].0, "Line catchments");
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Line 1:"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cut_fill_reports_volumes() {
    let dir = std::env::temp_dir().join(format!("isoliner-cf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ground = dir.join("ground.tif");
    let plan = dir.join("plan.tif");
    let gz = vec![10.0f32; 25];
    let mut pz = vec![9.0f32; 25];
    pz[12] = 11.0;
    write_grid(&ground, &gz, 5, 5, 1.0, 0.0, 0.0);
    write_grid(&plan, &pz, 5, 5, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("ground".into(), ground);
    host.rasters.insert("plan".into(), plan);
    let out = raster_cmd::cut_fill_cmd(
        &mut host,
        &json!({"ground": "ground", "plan": "plan"}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.added.len(), 1);
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Cut 24"), "{report}");
    assert!(report.contains("fill 1"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn bridge_removal_replaces_the_spike() {
    let dir = std::env::temp_dir().join(format!("isoliner-br-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("bridge.tif");
    let mut z = vec![10.0f32; 25];
    z[12] = 30.0;
    write_grid(&src, &z, 5, 5, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::bridge_removal(
        &mut host,
        &json!({"input": "dem", "threshold": 5.0}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.added.len(), 1);
    let g = geotiff::read(&host.added[0]).unwrap();
    assert!(g.z[12] < 30.0, "spike stayed {}", g.z[12]);
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Cells replaced:"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn snap_elevations_writes_z() {
    let dir = std::env::temp_dir().join(format!("isoliner-se-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("dem.tif");
    let mut z = vec![0.0f32; 25];
    for r in 0..5 {
        for c in 0..5 {
            z[r * 5 + c] = 10.0 - r as f32;
        }
    }
    write_grid(&src, &z, 5, 5, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::snap_elevations(
        &mut host,
        &json!({"points": "wells", "dem": "dem", "sampling": "Bilinear"}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.layers.len(), 1);
    assert!(out["report"].as_str().unwrap().contains("Elevation read at"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn downhill_trace_adds_lines() {
    let dir = std::env::temp_dir().join(format!("isoliner-dt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("dem.tif");
    let mut z = vec![0.0f32; 36];
    for r in 0..6 {
        for c in 0..6 {
            z[r * 6 + c] = 10.0 - (r + c) as f32;
        }
    }
    write_grid(&src, &z, 6, 6, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::downhill_trace(
        &mut host,
        &json!({"starts": "gauges", "dem": "dem", "fill": true}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.layers.len(), 1);
    assert!(out["report"].as_str().unwrap().contains("Traces:"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn catchment_stats_reports_basins() {
    let dir = std::env::temp_dir().join(format!("isoliner-cs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let basins = dir.join("basins.tif");
    let dem = dir.join("dem.tif");
    let mut bz = vec![1.0f32; 25];
    for k in 0..12 {
        bz[k] = 2.0;
    }
    let dz: Vec<f32> = (0..25).map(|k| 10.0 + k as f32).collect();
    write_grid(&basins, &bz, 5, 5, 1.0, 0.0, 0.0);
    write_grid(&dem, &dz, 5, 5, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("basins".into(), basins);
    host.rasters.insert("dem".into(), dem);
    let out = raster_cmd::catchment_stats(
        &mut host,
        &json!({"basins": "basins", "dem": "dem"}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.layers.len(), 1);
    assert!(out["report"].as_str().unwrap().contains("Basins: 2"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn breakline_candidates_adds_points() {
    let dir = std::env::temp_dir().join(format!("isoliner-bc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("step.tif");
    let mut z = vec![0.0f32; 49];
    for r in 0..7 {
        for c in 0..7 {
            z[r * 7 + c] = if r < 3 { 20.0 } else { 10.0 };
        }
    }
    write_grid(&src, &z, 7, 7, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::breakline_candidates(
        &mut host,
        &json!({"input": "dem", "threshold": 5.0}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.layers.len(), 1);
    assert!(out["report"].as_str().unwrap().contains("Brows:"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn demo_pit_adds_a_raster() {
    let mut host = Fake::default();
    let out = raster_cmd::topo_demo_pit(
        &mut host,
        &json!({"rows": 50, "cols": 50, "cell": 1.0}),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.added.len(), 1);
    assert!(out["report"].as_str().unwrap().contains("Demo quarry"));
}

#[test]
fn breakline_pairs_makes_a_pair() {
    let dir = std::env::temp_dir().join(format!("isoliner-bp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("dem.tif");
    let mut z = vec![0.0f32; 36];
    for r in 0..6 {
        for c in 0..6 {
            z[r * 6 + c] = 10.0 - (r + c) as f32;
        }
    }
    write_grid(&src, &z, 6, 6, 1.0, 0.0, 0.0);
    let mut host = Fake::default();
    host.rasters.insert("dem".into(), src);
    let out = raster_cmd::breakline_pairs(
        &mut host,
        &json!({
            "input": "breaks",
            "dem": "dem",
            "kind_field": "kind",
            "max_dist": 50.0,
            "min_share": 0.2
        }),
        Lang::En,
    )
    .unwrap();
    assert!(host.layers.len() >= 2, "layers: {}", host.layers.len());
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Pairs:"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn topo2raster_builds_a_raster() {
    let mut host = Fake::default();
    let out = raster_cmd::topo2raster_cmd(
        &mut host,
        &json!({
            "contours": "contours",
            "field": "elev",
            "extent": "small_bounds",
            "cell": 1.0,
            "iterations": 5,
            "min_drop": 0.0,
            "depth_tol": 0.0
        }),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.added.len(), 1);
    assert!(out["report"].as_str().unwrap().contains("Contours:"));
}

#[test]
fn subsidence_tilt_curvature_adds_five_rasters() {
    let mut host = Fake::default();
    let out = raster_cmd::subsidence_tilt_curvature(
        &mut host,
        &json!({
            "input": "dem",
            "units": "Millimeters",
            "sign": "Positive down",
            "base": 15.0,
            "reduce": false
        }),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.exported, ["dem"]);
    assert_eq!(host.added.len(), 5);
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Base "), "{report}");
}

#[test]
fn subsidence_horizontal_adds_raster() {
    let mut host = Fake::default();
    let out = raster_cmd::subsidence_horizontal(
        &mut host,
        &json!({
            "curv": "dem",
            "formula": "ε = mₑ·K·L (current guidelines)",
            "half_l": 100.0,
            "depth": 300.0,
            "boundary": "Permanent (δ₀ = 55°)"
        }),
        Lang::En,
    )
    .unwrap();
    assert_eq!(host.exported, ["dem"]);
    assert_eq!(host.added.len(), 1);
    let report = out["report"].as_str().unwrap();
    assert!(report.contains("Half-trough length"), "{report}");
}

#[test]
fn slope_aspect_needs_a_layer() {
    let mut host = Fake::default();
    let e = raster_cmd::slope_aspect(&mut host, &json!({}), Lang::Ru).unwrap_err();
    assert_eq!(e, "Не выбрана входная ЦМР.");
    assert!(host.exported.is_empty());
}
