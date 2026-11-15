// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 7.05 Пример для фракталов (демо), порт `grid_isolines/tools/fractal_demo.py`.
//!
//! Речная сеть рекурсивным ветвлением со срединными смещениями, изрезанный
//! водосбор вокруг неё и береговая линия. Генератор NumPy с зерном SEED
//! (ноль - тоже зерно ноль), порядок обращений тот же: реки, водосбор, берег.

use std::f64::consts::PI;

use serde_json::{Value as J, json};

use crate::args;
use crate::npy_rng::Rng;
use crate::report::{Report, ToolError};
use crate::tools::Values;
use crate::tools::density_demo::{layer, xyz};
use crate::tools::geophys_profiles::bbox;

/// Фрактальная ломаная срединных смещений между двумя точками (`midpoint`).
fn midpoint(p0: [f64; 2], p1: [f64; 2], rough: f64, depth: usize, rng: &mut Rng) -> Vec<[f64; 2]> {
    let mut pts = vec![p0, p1];
    for _ in 0..depth {
        let mut out = vec![pts[0]];
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            let d = [b[0] - a[0], b[1] - a[1]];
            let n = [-d[1], d[0]];
            let l = d[0].hypot(d[1]);
            let z = rng.normal(0.0, rough * l);
            let k = l + 1e-12;
            out.push([m[0] + n[0] / k * z, m[1] + n[1] / k * z]);
            out.push(b);
        }
        pts = out;
    }
    pts
}

struct Net<'a> {
    rng: &'a mut Rng,
    min_wh: f64,
    rivers: Vec<(i64, Vec<[f64; 2]>)>,
}

impl Net<'_> {
    fn branch(&mut self, p: [f64; 2], ang: f64, l: f64, order: i64, depth: i32) {
        if depth <= 0 || l < 0.02 * self.min_wh {
            return;
        }
        let q = [p[0] + l * ang.cos(), p[1] + l * ang.sin()];
        let pl = midpoint(p, q, 0.10, 3, self.rng);
        self.rivers.push((order, pl.clone()));
        let k = self.rng.integers(2, 4);
        for i in 0..k {
            let tt = 0.3 + 0.6 * self.rng.random();
            let bp = pl[(tt * (pl.len() - 1) as f64) as usize];
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            let da = (25.0 + 25.0 * self.rng.random()) * (PI / 180.0) * sign;
            let nl = l * (0.45 + 0.15 * self.rng.random());
            self.branch(bp, ang + da, nl, order + 1, depth - 1);
        }
    }
}

pub fn run(v: &Values<'_>, report: &mut Report) -> Result<J, ToolError> {
    let (x0, y0, x1, y1) = bbox(v.items("EXTENT"))
        .filter(|e| e.2 > e.0 && e.3 > e.1)
        .ok_or_else(|| ToolError::new("Не задан охват.", args![]))?;
    let (w, h) = (x1 - x0, y1 - y0);
    let seed = v.integer("SEED").max(0);
    let mut rng = Rng::new(u128::from(seed.unsigned_abs()));

    let mut net = Net {
        rng: &mut rng,
        min_wh: w.min(h),
        rivers: Vec::new(),
    };
    net.branch([x0 + 0.5 * w, y0 + 0.06 * h], PI / 2.0, 0.8 * h, 1, 4);
    let rivers = std::mem::take(&mut net.rivers);
    let river_feats: Vec<J> = rivers
        .iter()
        .map(|(o, pl)| json!({"geometry": {"points": xyz(pl)}, "values": [o], "raw": [o]}))
        .collect();

    // водосбор: изрезанный овал вокруг сети
    let (cx, cy) = (x0 + 0.5 * w, y0 + 0.5 * h);
    let nang = 24;
    let base: Vec<[f64; 2]> = (0..nang)
        .map(|i| {
            let a = 2.0 * PI * f64::from(i) / f64::from(nang);
            let r = 0.42 + 0.06 * rng.random();
            [cx + r * w * a.cos(), cy + r * h * a.sin()]
        })
        .collect();
    let mut ring = Vec::new();
    for k in 0..base.len() {
        let seg = midpoint(base[k], base[(k + 1) % base.len()], 0.16, 3, &mut rng);
        ring.extend_from_slice(&seg[..seg.len() - 1]);
    }
    ring.push(ring[0]);
    let basin_name = v.tool.translate("Водосбор (демо)", v.lang).to_string();
    let basin =
        json!({"geometry": {"rings": [xyz(&ring)]}, "values": [basin_name], "raw": [basin_name]});

    // берег: одна сильно изрезанная линия через охват
    let coast = midpoint(
        [x0 + 0.05 * w, y0 + 0.85 * h],
        [x0 + 0.95 * w, y0 + 0.7 * h],
        0.22,
        8,
        &mut rng,
    );
    let coast_name = v.tool.translate("Берег (демо)", v.lang).to_string();
    let coast =
        json!({"geometry": {"points": xyz(&coast)}, "values": [coast_name], "raw": [coast_name]});

    report.info("Рек сгенерировано: %d.", args![rivers.len()]);
    Ok(json!({
        "OUT_RIVERS": layer("line", &[("order", "int")], river_feats),
        "OUT_BASIN": layer("polygon", &[("name", "string")], vec![basin]),
        "OUT_COAST": layer("line", &[("name", "string")], vec![coast]),
    }))
}
