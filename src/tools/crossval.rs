// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 1.08 Кросс-валидация вариограммы, порт `grid_isolines/tools/crossval.py`.
//!
//! Каждая точка по очереди исключается и оценивается кригингом по
//! остальным. Вариограмма - из таблицы моделей VG_TABLE или из полей
//! окна. Выход OUTPUT - точки остатков, метрики и советы - в журнал.

use serde_json::{Value as J, json};

use crate::args;
use crate::field_format::{Spec, plan_for, row};
use crate::krige::{self, PolyTrend, Search};
use crate::numpy;
use crate::points::read_points;
use crate::report::{Report, ToolError};
use crate::tools::Values;
use crate::tools::cv::{self, Metrics};
use crate::tools::declustering::{san, value_field};
use crate::tools::variogram::{outliers, warn_data};

/// Точность полей без подписи с единицами (`FIELD_DECIMALS` класса).
pub const FIELD_DECIMALS: &[(&str, Spec)] = &[
    ("z_est", Spec::Significant(5)),
    ("error", Spec::Significant(5)),
    ("abs_error", Spec::Significant(5)),
    ("std_resid", Spec::Decimals(3)),
];

/// Метрики и советы в журнал - общий текст 1.08 и 1.09.
pub fn report_metrics(v: &Values<'_>, report: &mut Report, m: &Metrics, nvalid: usize, n: usize) {
    report.info("== Кросс-валидация (leave-one-out) ==", args![]);
    report.info("Точек оценено: %d из %d", args![nvalid, n]);
    report.info("ME (смещение):   %+.4g   (ближе к 0 - лучше)", args![m.me]);
    report.info("MAE:             %.4g", args![m.mae]);
    report.info("RMSE:            %.4g   (меньше - лучше)", args![m.rmse]);
    report.info("MSDR:            %.3f   (ближе к 1 - лучше)", args![m.msdr]);
    report.info("R (оценка/факт): %.3f", args![m.r]);
    for a in cv::cv_advice(v, m) {
        report.info("%s", args![format!("• {a}")]);
    }
}

fn or(x: f64, d: f64) -> f64 {
    if x.is_nan() || x == 0.0 { d } else { x }
}

pub fn run(v: &Values<'_>, report: &mut Report) -> Result<J, ToolError> {
    let items = v.items("INPUT");
    let layer = v.layer("INPUT");
    let zfield = v.text("ZFIELD");
    let idfield = v.text("IDFIELD");
    let clip = outliers(v);
    let id_opt = (!idfield.is_empty()).then_some(idfield.as_str());
    let p = read_points(items, &zfield, layer, report, clip, id_opt)?;
    warn_data(report, &p);
    let wfield = v.text("WEIGHT_FIELD");
    let mut cvw = None;
    if !wfield.is_empty() {
        let pw = read_points(items, &zfield, layer, report, clip, Some(&wfield))?;
        cvw = cv::read_weights(report, &pw.ids, p.xs.len());
        if cvw.is_some() {
            report.info(
                "Метрики взвешены декластеризацией (поле «%s»).",
                args![wfield.as_str()],
            );
        }
    }
    let ktype = if v.integer("KTYPE") == 0 { 1 } else { 0 };
    let skmean = or(v.number("SKMEAN"), 0.0);
    let nugget = or(v.number("NUGGET"), 0.0);
    let mut radius = or(v.number("RADIUS"), 0.0);
    let ndmin = match v.integer("MIN_POINTS") {
        0 => 1,
        k => k.max(0) as usize,
    };
    let ndmax = match v.integer("MAX_POINTS") {
        0 => 24,
        k => k.max(0) as usize,
    };
    let span = |a: &[f64]| {
        a.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - a.iter().copied().fold(f64::INFINITY, f64::min)
    };
    let (w, h) = (span(&p.xs), span(&p.ys));
    let auto_range = or(w.max(h) / 3.0, 1.0);
    if radius <= 0.0 {
        radius = or(w.hypot(h), 1e12);
    }
    let vg = cv::build_variogram(v, report, nugget, auto_range)?;
    let nodata = -9999.0;
    let dvar = numpy::var(&p.vs);
    report.info(
        "Дисперсия данных: %.4g. Ориентир: суммарный силл (C0 + вклады C) задавайте \
         близким к ней. Наггет и силл - в абсолютных единицах дисперсии, не 0-1.",
        args![dvar],
    );
    report.info("Кросс-валидация по %d точкам…", args![p.xs.len()]);
    let mut detrend = v.flag("DETREND");
    let mut degree = 1usize;
    if detrend {
        degree = (v.integer("DETREND_DEG").max(0) + 1) as usize;
        let need = PolyTrend::n_terms(degree);
        if p.vs.len() <= need {
            report.warn(
                "Снятие тренда отключено: точек %d, для степени %d нужно больше %d.",
                args![p.vs.len(), degree, need],
            );
            detrend = false;
        } else {
            let var0 = numpy::var(&p.vs);
            let t = PolyTrend::fit(&p.xs, &p.ys, &p.vs, degree);
            let r: Vec<f64> = (0..p.xs.len())
                .map(|i| p.vs[i] - t.at(p.xs[i], p.ys[i]))
                .collect();
            let share = if var0 > 0.0 {
                100.0 * (1.0 - numpy::var(&r) / var0)
            } else {
                0.0
            };
            report.info(
                "Снят тренд степени %d: убрано %.1f%% дисперсии. Тренд переподбирается \
                 на каждом шаге LOO по остальным точкам, вариограмму задавайте по остаткам.",
                args![degree, share],
            );
        }
    }
    let search = Search {
        ktype,
        skmean,
        ndmin,
        ndmax,
        rad2: radius * radius,
        nodata,
    };
    let (est, var) = if detrend {
        krige::cross_validate_detrend(&p.xs, &p.ys, &p.vs, degree, &vg, &search)
    } else {
        krige::cross_validate(&p.xs, &p.ys, &p.vs, &vg, &search)
    };
    let ok: Vec<usize> = (0..est.len()).filter(|&i| est[i] != nodata).collect();
    if ok.len() < 2 {
        return Err(ToolError::new("Слишком мало оценённых точек.", args![]));
    }
    let pick = |a: &[f64]| ok.iter().map(|&i| a[i]).collect::<Vec<f64>>();
    let (fact, eo, vo) = (pick(&p.vs), pick(&est), pick(&var));
    let err: Vec<f64> = eo.iter().zip(&fact).map(|(a, b)| a - b).collect();
    let wo = cvw.as_ref().map(|w| pick(w));
    let m = cv::weighted_cv_metrics(&fact, &eo, &err, &vo, wo.as_deref());
    report_metrics(v, report, &m, ok.len(), p.xs.len());

    let valname = value_field(&zfield);
    let idname = id_opt.map(|f| {
        let s = san(f);
        if s.is_empty() {
            "well_id".to_string()
        } else {
            s
        }
    });
    let mut names: Vec<String> = Vec::new();
    let mut fields = Vec::new();
    if let Some(id) = &idname {
        names.push(id.clone());
        fields.push(json!({"name": id, "type": "string"}));
    }
    for (n, t) in [
        (valname.as_str(), "double"),
        ("z_est", "double"),
        ("error", "double"),
        ("abs_error", "double"),
        ("std_resid", "double"),
    ] {
        names.push(n.to_string());
        fields.push(json!({"name": n, "type": t}));
    }
    let nrefs: Vec<&str> = names.iter().map(String::as_str).collect();
    let plan = plan_for(&nrefs, &[], FIELD_DECIMALS, &v.input_fields());
    let mut feats = Vec::new();
    for &i in &ok {
        let e = est[i] - p.vs[i];
        let s = var[i].max(0.0).sqrt();
        let mut raw = Vec::new();
        if idname.is_some() {
            raw.push(match &p.ids[i] {
                J::Null => J::Null,
                x => json!(crate::drillhole::py_str(x)),
            });
        }
        raw.extend([
            json!(p.vs[i]),
            json!(est[i]),
            json!(e),
            json!(e.abs()),
            if s > 0.0 { json!(e / s) } else { J::Null },
        ]);
        let (values, raw) = row(&nrefs, raw, &plan);
        feats.push(json!({
            "geometry": {"xyz": [p.xs[i], p.ys[i], null]},
            "values": values,
            "raw": raw,
        }));
    }
    Ok(json!({"OUTPUT": {"geometry": "point", "fields": fields, "features": feats}}))
}
