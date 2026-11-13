// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 1.09 Кросс-валидация метода (LOO), порт `grid_isolines/tools/method_cv.py`.
//!
//! Метод - кригинг или минимальная кривизна. Охваты окна QGIS (сетка
//! минимальной кривизны и область проверки) приходят слоями, охват - их
//! общий охват. Когда проверяемых точек меньше, чем прошло фильтры,
//! выборка случайная, генератором NumPy с зерном SEED.

use serde_json::{Value as J, json};

use crate::args;
use crate::field_format::{plan_for, row};
use crate::krige;
use crate::mincurv::{self, Frame, Params};
use crate::npy_rng::Rng;
use crate::points::{num, read_points};
use crate::report::{Report, ToolError};
use crate::tools::Values;
use crate::tools::crossval::FIELD_DECIMALS;
use crate::tools::cv;
use crate::tools::declustering::{san, value_field};
use crate::tools::geophys_profiles::bbox;
use crate::tools::variogram::outliers;

/// Охват точек слоя (`_points_bbox`).
fn points_bbox(items: &[J]) -> Option<(f64, f64, f64, f64)> {
    let pts: Vec<(f64, f64)> = items
        .iter()
        .filter(|it| it["xyz"].is_array())
        .map(|it| {
            (
                it["xyz"][0].as_f64().unwrap_or(f64::NAN),
                it["xyz"][1].as_f64().unwrap_or(f64::NAN),
            )
        })
        .collect();
    if pts.is_empty() {
        return None;
    }
    let f = |g: fn(f64, f64) -> f64, init: f64, k: usize| {
        pts.iter()
            .map(|p| if k == 0 { p.0 } else { p.1 })
            .fold(init, g)
    };
    Some((
        f(f64::min, f64::INFINITY, 0),
        f(f64::min, f64::INFINITY, 1),
        f(f64::max, f64::NEG_INFINITY, 0),
        f(f64::max, f64::NEG_INFINITY, 1),
    ))
}

/// Необязательное число: пусто - `None`.
fn opt(v: &Values<'_>, key: &str) -> Option<f64> {
    match v.get(key) {
        J::Null => None,
        J::String(s) if s.is_empty() => None,
        x => num(&x),
    }
}

/// `float(values.get(key) or default)`.
fn or(v: &Values<'_>, key: &str, d: f64) -> f64 {
    let x = v.number(key);
    if x.is_nan() || x == 0.0 { d } else { x }
}

pub fn run(v: &Values<'_>, report: &mut Report) -> Result<J, ToolError> {
    let items = v.items("INPUT");
    let layer = v.layer("INPUT");
    let zfield = v.text("ZFIELD");
    let idfield = v.text("IDFIELD");
    let id_opt = (!idfield.is_empty()).then_some(idfield.as_str());
    let kriging = v.integer("METHOD") == 0;
    let clip = outliers(v);
    let p = read_points(items, &zfield, layer, report, clip, id_opt)?;
    let n = p.xs.len();
    let wfield = v.text("WEIGHT_FIELD");
    let mut mcw = None;
    if !wfield.is_empty() {
        let pw = read_points(items, &zfield, layer, report, clip, Some(&wfield))?;
        mcw = cv::read_weights(report, &pw.ids, n);
        if mcw.is_some() {
            report.info(
                "Метрики взвешены декластеризацией (поле «%s»).",
                args![wfield.as_str()],
            );
        }
    }

    // отбор проверяемых точек: область и значение
    let frect = bbox(v.items("FILTER_EXTENT")).filter(|r| r.2 > r.0 && r.3 > r.1);
    let (zmin, zmax) = (opt(v, "ZMIN"), opt(v, "ZMAX"));
    let cand: Vec<usize> = (0..n)
        .filter(|&i| {
            frect.is_none_or(|r| {
                p.xs[i] >= r.0 && p.xs[i] <= r.2 && p.ys[i] >= r.1 && p.ys[i] <= r.3
            }) && zmin.is_none_or(|z| p.vs[i] >= z)
                && zmax.is_none_or(|z| p.vs[i] <= z)
        })
        .collect();
    if cand.len() < 2 {
        return Err(ToolError::new(
            "После фильтров осталось меньше двух проверяемых точек.",
            args![],
        ));
    }
    let mut nreq = v.integer("N_VALIDATE");
    if nreq <= 0 {
        nreq = cand.len().min(100) as i64;
    }
    let nreq = (nreq as usize).min(cand.len());
    let mut rng = Rng::from_module_seed(v.integer("SEED"));
    let val_idx: Vec<usize> = if nreq < cand.len() {
        let mut s: Vec<usize> = rng
            .choice(cand.len(), nreq)
            .unwrap_or_default()
            .into_iter()
            .map(|k| cand[k])
            .collect();
        s.sort_unstable();
        s
    } else {
        cand
    };
    let excl = (or(v, "EXCL_X", 0.0), or(v, "EXCL_Y", 0.0));
    let nodata = -9999.0;
    report.info(
        "Метод: %s. Проверяем %d из %d точек.",
        args![
            if kriging {
                "кригинг"
            } else {
                "минимальная кривизна"
            },
            val_idx.len(),
            n
        ],
    );

    let mut est = vec![f64::NAN; val_idx.len()];
    let mut var: Option<Vec<f64>> = None;
    let span = |a: &[f64]| {
        a.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - a.iter().copied().fold(f64::INFINITY, f64::min)
    };
    if kriging {
        let ktype = if v.integer("KTYPE") == 0 { 1 } else { 0 };
        let skmean = or(v, "SKMEAN", 0.0);
        let nugget = or(v, "NUGGET", 0.0);
        let mut radius = or(v, "RADIUS", 0.0);
        let ndmin = match v.integer("MIN_POINTS") {
            0 => 1,
            k => k.max(0) as usize,
        };
        let ndmax = match v.integer("MAX_POINTS") {
            0 => 24,
            k => k.max(0) as usize,
        };
        let (w, h) = (span(&p.xs), span(&p.ys));
        let auto_range = {
            let a = w.max(h) / 3.0;
            if a == 0.0 { 1.0 } else { a }
        };
        if radius <= 0.0 {
            let d = w.hypot(h);
            radius = if d == 0.0 { 1e12 } else { d };
        }
        let rad2 = radius * radius;
        let vg = cv::build_variogram(v, report, nugget, auto_range)?;
        let mut vv = vec![f64::NAN; val_idx.len()];
        for (k, &i) in val_idx.iter().enumerate() {
            let keep: Vec<usize> = (0..n)
                .filter(|&q| {
                    let drop = q == i
                        || ((excl.0 > 0.0 || excl.1 > 0.0)
                            && (p.xs[q] - p.xs[i]).abs() <= excl.0
                            && (p.ys[q] - p.ys[i]).abs() <= excl.1);
                    !drop
                })
                .collect();
            if keep.len() < ndmin.max(1) {
                continue;
            }
            let pick = |a: &[f64]| keep.iter().map(|&q| a[q]).collect::<Vec<f64>>();
            let (e, s) = krige::solve_point(
                p.xs[i],
                p.ys[i],
                &pick(&p.xs),
                &pick(&p.ys),
                &pick(&p.vs),
                &vg,
                ktype,
                skmean,
                ndmin,
                ndmax,
                rad2,
                nodata,
            );
            if e != nodata {
                est[k] = e;
                vv[k] = s;
            }
        }
        var = Some(vv);
    } else {
        let rect = bbox(v.items("EXTENT"))
            .filter(|r| r.2 > r.0 && r.3 > r.1)
            .or_else(|| points_bbox(items))
            .unwrap_or((0.0, 0.0, 1.0, 1.0));
        let (xmin, ymin, xmax, ymax) = rect;
        let (width, height) = (xmax - xmin, ymax - ymin);
        let mut cell = or(v, "CELL_SIZE", 0.0);
        if cell <= 0.0 {
            let c = width.min(height) / 50.0;
            cell = if c == 0.0 { 1.0 } else { c };
        }
        let nx = ((width / cell).ceil() as i64).max(2) as usize;
        let ny = ((height / cell).ceil() as i64).max(2) as usize;
        let max_iter = match v.integer("MAX_ITER") {
            0 => 100_000,
            k => k.max(1) as usize,
        };
        let max_res = or(v, "MAX_RESIDUAL", 0.0);
        let zr = {
            let r = span(&p.vs);
            if r == 0.0 { 1.0 } else { r }
        };
        let params = Params {
            tension: or(v, "TENSION", 0.0),
            boundary_tension: or(v, "BOUNDARY_TENSION", 0.0),
            relax: or(v, "RELAX", 1.85),
            aniso: or(v, "ANISO", 1.0),
            tol: if max_res > 0.0 {
                max_res
            } else {
                (1e-4 * zr).max(1e-6)
            },
        };
        report.info("Сетка %d x %d, ячейка %.4g.", args![nx, ny, cell]);
        let frame = Frame {
            xmin,
            ymin,
            cell,
            nx,
            ny,
        };
        est = mincurv::loo_estimates(
            &p.xs,
            &p.ys,
            &p.vs,
            &frame,
            &val_idx,
            &params,
            max_iter,
            (max_iter / 20).max(2000),
            excl,
        );
    }

    let fact: Vec<f64> = val_idx.iter().map(|&i| p.vs[i]).collect();
    let ok: Vec<usize> = (0..val_idx.len()).filter(|&k| est[k].is_finite()).collect();
    if ok.len() < 2 {
        return Err(ToolError::new("Слишком мало оценённых точек.", args![]));
    }
    let fo: Vec<f64> = ok.iter().map(|&k| fact[k]).collect();
    let eo: Vec<f64> = ok.iter().map(|&k| est[k]).collect();
    let err: Vec<f64> = eo.iter().zip(&fo).map(|(a, b)| a - b).collect();
    let vo: Vec<f64> = var
        .as_ref()
        .map_or_else(Vec::new, |vv| ok.iter().map(|&k| vv[k]).collect());
    let wo: Option<Vec<f64>> = mcw.map(|w| ok.iter().map(|&k| w[val_idx[k]]).collect());
    let m = cv::weighted_cv_metrics(&fo, &eo, &err, &vo, wo.as_deref());
    report.info("== Кросс-валидация метода (LOO) ==", args![]);
    report.info("Точек оценено: %d из %d", args![ok.len(), val_idx.len()]);
    report.info("ME (смещение):   %+.4g", args![m.me]);
    report.info("MAE:             %.4g", args![m.mae]);
    report.info("RMSE:            %.4g", args![m.rmse]);
    if var.is_some() {
        report.info("MSDR:            %.3f", args![m.msdr]);
    }
    report.info("R (оценка/факт): %.3f", args![m.r]);
    for a in cv::cv_advice(v, &m) {
        report.info("%s", args![format!("• {a}")]);
    }

    let idname = id_opt.map(|f| {
        let s = san(f);
        if s.is_empty() {
            "well_id".to_string()
        } else {
            s
        }
    });
    let mut names: Vec<String> = Vec::new();
    if let Some(id) = &idname {
        names.push(id.clone());
    }
    names.extend(
        [value_field(&zfield).as_str(), "z_est", "error", "abs_error"]
            .iter()
            .map(|s| s.to_string()),
    );
    if var.is_some() {
        names.push("std_resid".into());
    }
    let fields: Vec<J> = names
        .iter()
        .enumerate()
        .map(|(k, n)| {
            let t = if k == 0 && idname.is_some() {
                "string"
            } else {
                "double"
            };
            json!({"name": n, "type": t})
        })
        .collect();
    let nrefs: Vec<&str> = names.iter().map(String::as_str).collect();
    let plan = plan_for(&nrefs, &[], FIELD_DECIMALS, &v.input_fields());
    let mut feats = Vec::new();
    for &k in &ok {
        let i = val_idx[k];
        let e = est[k] - fact[k];
        let mut raw = Vec::new();
        if idname.is_some() {
            raw.push(match &p.ids[i] {
                J::Null => J::Null,
                x => json!(crate::drillhole::py_str(x)),
            });
        }
        raw.extend([json!(fact[k]), json!(est[k]), json!(e), json!(e.abs())]);
        if let Some(vv) = &var {
            let s = vv[k].max(0.0).sqrt();
            raw.push(if s > 0.0 { json!(e / s) } else { J::Null });
        }
        let (values, raw) = row(&nrefs, raw, &plan);
        feats.push(json!({
            "geometry": {"xyz": [p.xs[i], p.ys[i], null]},
            "values": values,
            "raw": raw,
        }));
    }
    Ok(json!({"OUTPUT": {"geometry": "point", "fields": fields, "features": feats}}))
}
