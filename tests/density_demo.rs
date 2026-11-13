// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Учебный набор 3.08 против `density.demo_dataset` модуля
//! (`tests/data/density_demo.json`, сценарий `tools/gen_density_ref.py`):
//! при одном зерне совпадает до бита.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use isoliner_rustgis::density::demo_dataset;
use isoliner_rustgis::npy_rng::Rng;
use serde_json::{Value as J, json};

#[test]
fn like_module() {
    let path = format!(
        "{}/tests/data/density_demo.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let d: J = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for c in d["cases"].as_array().unwrap() {
        let e: Vec<f64> = c["ext"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let mut rng = Rng::new(u128::from(c["seed"].as_u64().unwrap()));
        let got = demo_dataset(e[0], e[1], e[2], e[3], &mut rng);
        let pts: Vec<J> = got
            .points
            .iter()
            .map(|p| json!([p.0, p.1, p.2, p.3]))
            .collect();
        assert_eq!(json!(pts), c["points"]);
        for (k, l) in got.lines.iter().enumerate() {
            let w = &c["lines"][k];
            assert_eq!(json!(l.verts), w["verts"]);
            assert_eq!(json!(l.half), w["half"]);
            assert_eq!(json!(l.from_m), w["from_m"]);
            assert_eq!(json!(l.to_m), w["to_m"]);
        }
        for (k, p) in got.polygons.iter().enumerate() {
            assert_eq!(json!(p.rings), c["polygons"][k]["rings"]);
            assert_eq!(json!(p.dasy), c["polygons"][k]["dasy"]);
        }
    }
}
