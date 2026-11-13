// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Учебный набор плотности 3.08, порт `density.demo_dataset` модуля:
//! 10 точек с разными сигмами (масса 500), 2 линии (масса 200, у одной
//! вырезка интервала), 2 полигона (масса 300, один под дазиметрию).
//! Итого 1000. Положение точек - генератором NumPy, при зерне больше нуля
//! совпадает с модулем.

use crate::npy_rng::Rng;

/// Точка: X, Y, масса, сигма.
pub type DemoPoint = (f64, f64, f64, f64);

/// Линия с массой, полушириной коридора и необязательной вырезкой.
#[derive(Debug, Clone, PartialEq)]
pub struct DemoLine {
    pub verts: Vec<[f64; 2]>,
    pub mass: f64,
    pub half: f64,
    pub from_m: Option<f64>,
    pub to_m: Option<f64>,
}

/// Полигон с массой и признаком дазиметрии.
#[derive(Debug, Clone, PartialEq)]
pub struct DemoPolygon {
    pub rings: Vec<Vec<[f64; 2]>>,
    pub mass: f64,
    pub dasy: bool,
}

/// Учебный набор целиком.
#[derive(Debug, Clone, PartialEq)]
pub struct Demo {
    pub points: Vec<DemoPoint>,
    pub lines: Vec<DemoLine>,
    pub polygons: Vec<DemoPolygon>,
}

/// `demo_dataset(xmin, ymin, xmax, ymax, seed)`.
pub fn demo_dataset(xmin: f64, ymin: f64, xmax: f64, ymax: f64, rng: &mut Rng) -> Demo {
    let w = xmax - xmin;
    let h = ymax - ymin;
    let d = w.min(h);
    let points = (0..10)
        .map(|i| {
            let x = xmin + rng.uniform(0.15, 0.85) * w;
            let y = ymin + rng.uniform(0.15, 0.85) * h;
            let sigma = (0.004 + 0.06 * (f64::from(i) / 9.0)) * d;
            (x, y, 50.0, sigma)
        })
        .collect();
    let p = |fx: f64, fy: f64| [xmin + fx * w, ymin + fy * h];
    let l1 = vec![p(0.10, 0.30), p(0.50, 0.36), p(0.90, 0.30)];
    let l2 = vec![p(0.20, 0.70), p(0.80, 0.74)];
    let len2 = (l2[1][0] - l2[0][0]).hypot(l2[1][1] - l2[0][1]);
    let lines = vec![
        DemoLine {
            verts: l1,
            mass: 100.0,
            half: 0.03 * d,
            from_m: None,
            to_m: None,
        },
        DemoLine {
            verts: l2,
            mass: 100.0,
            half: 0.03 * d,
            from_m: Some(0.25 * len2),
            to_m: Some(0.75 * len2),
        },
    ];
    let b = |x0: f64, x1: f64, y0: f64, y1: f64| {
        vec![vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1), p(x0, y0)]]
    };
    let polygons = vec![
        DemoPolygon {
            rings: b(0.12, 0.40, 0.10, 0.42),
            mass: 150.0,
            dasy: false,
        },
        DemoPolygon {
            rings: b(0.58, 0.90, 0.55, 0.88),
            mass: 150.0,
            dasy: true,
        },
    ];
    Demo {
        points,
        lines,
        polygons,
    }
}
