// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Генератор против NumPy 2.4.6 (`tests/data/npy_rng.json`, сценарий
//! `tools/gen_rng_ref.py`): состояние после засева, равномерные, целые
//! вперемешку с равномерными и 6000 нормальных на зерно. Совпадение
//! точное, до бита.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use isoliner_rustgis::npy_rng::Rng;
use serde_json::Value as J;

fn data() -> J {
    let path = format!("{}/tests/data/npy_rng.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn floats(v: &J) -> Vec<f64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect()
}

#[test]
fn like_numpy_default_rng() {
    let d = data();
    for c in d["cases"].as_array().unwrap() {
        let seed: u128 = c["seed"].as_str().unwrap().parse().unwrap();
        let mut r = Rng::new(seed);
        let (state, inc) = r.state();
        assert_eq!(
            state.to_string(),
            c["state"].as_str().unwrap(),
            "seed {seed}"
        );
        assert_eq!(inc.to_string(), c["inc"].as_str().unwrap(), "seed {seed}");
        let rand: Vec<f64> = (0..5).map(|_| r.random()).collect();
        assert_eq!(rand, floats(&c["random"]), "seed {seed}");
        let u = [
            r.uniform(0.35, 0.65),
            r.uniform(0.0, 2.0 * std::f64::consts::PI),
        ];
        assert_eq!(u.to_vec(), floats(&c["uniform"]), "seed {seed}");
        let want = c["mixed"].as_array().unwrap();
        for (k, hi) in [3i64, 5, 1 << 31, (1 << 32) + 1, 7, 1 << 40, 4]
            .iter()
            .enumerate()
        {
            assert_eq!(
                r.integers(1, *hi),
                want[2 * k].as_i64().unwrap(),
                "seed {seed} hi {hi}"
            );
            assert_eq!(
                r.random(),
                want[2 * k + 1].as_f64().unwrap(),
                "seed {seed} hi {hi}"
            );
        }
        let n = r.normals(0.0, 1.0, 6000);
        let wn = floats(&c["normal"]);
        for (i, (a, b)) in n.iter().zip(&wn).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "seed {seed} normal {i}: {a} {b}");
        }
        assert_eq!(
            r.normals(5.0, 3.0, 50),
            floats(&c["normal_scaled"]),
            "seed {seed}"
        );
        let want = c["choice"].as_array().unwrap();
        for (k, (n, m)) in [(50, 10), (20000, 100), (20000, 1000), (7, 7)]
            .iter()
            .enumerate()
        {
            let got: Vec<u64> = r
                .choice(*n, *m)
                .unwrap()
                .into_iter()
                .map(|v| v as u64)
                .collect();
            let w: Vec<u64> = want[k]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect();
            assert_eq!(got, w, "seed {seed} choice({n}, {m})");
        }
    }
}
