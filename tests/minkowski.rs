// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Размерность Минковского против `fractal.minkowski_dimension` модуля
//! (`tests/data/minkowski.json`, сценарий `tools/gen_minkowski_ref.py`):
//! покрытия совпадают точно, D и R² до 1e-12.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use isoliner_rustgis::fractal::minkowski_dimension;
use serde_json::Value as J;

#[test]
fn like_module() {
    let path = format!("{}/tests/data/minkowski.json", env!("CARGO_MANIFEST_DIR"));
    let d: J = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for c in d["cases"].as_array().unwrap() {
        let lines: Vec<Vec<[f64; 2]>> = c["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                l.as_array()
                    .unwrap()
                    .iter()
                    .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
                    .collect()
            })
            .collect();
        let kw = &c["kw"];
        let m = minkowski_dimension(
            &lines,
            8,
            kw["offsets"].as_u64().unwrap_or(3) as usize,
            kw["densify"].as_f64().unwrap_or(0.5),
            u128::from(kw["seed"].as_u64().unwrap_or(0)),
        );
        let name = c["name"].as_str().unwrap();
        let counts: Vec<usize> = c["counts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as usize)
            .collect();
        assert_eq!(m.counts, counts, "{name}");
        let want = c["d"].as_f64().unwrap();
        assert!((m.d - want).abs() < 1e-12, "{name}: {} {want}", m.d);
        let r2 = c["r2"].as_f64().unwrap();
        assert!((m.r2 - r2).abs() < 1e-12, "{name}: {} {r2}", m.r2);
    }
}
