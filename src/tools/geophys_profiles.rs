// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 1.11 Пример геофизических профилей (демо), порт
//! `grid_isolines/tools/geophys_profiles.py`.
//!
//! Охват EXTENT приходит объектами слоя, область - их общий охват. Выход
//! OUTPUT - точки профилей. Генератор совместим с NumPy, поэтому при
//! зерне больше нуля данные совпадают с модулем число в число.

use rustgis_kit::Lang;
use serde_json::{Value as J, json};

use crate::args;
use crate::field_format::{plan_for, row};
use crate::geodemo::{self, Electro, Frame, Subsidence};
use crate::npy_rng::Rng;
use crate::report::{Report, ToolError};
use crate::tools::Values;

/// Подписи полей выхода (`FIELD_LABELS` класса), по ним - точность.
pub const FIELD_LABELS: &[(&str, &str)] = &[
    ("profile", "Профиль"),
    ("picket_m", "Пикет, м"),
    ("pk", "Пикет (ПК)"),
    ("z", "Отметка z, м"),
    ("rho_k", "ρк, Ом·м"),
    ("rho_true", "ρк без шума, Ом·м"),
    ("sp", "ЕП, мВ"),
    ("vp", "ВП, мВ/В"),
    ("tour", "Тур"),
    ("settle", "Оседание, мм"),
    ("settle_true", "Оседание без шума, мм"),
];

/// Общий охват объектов (части и точки): x0, y0, x1, y1.
pub fn bbox(items: &[J]) -> Option<(f64, f64, f64, f64)> {
    let mut pts: Vec<(f64, f64)> = Vec::new();
    let xy = |p: &J| {
        (
            p[0].as_f64().unwrap_or(f64::NAN),
            p[1].as_f64().unwrap_or(f64::NAN),
        )
    };
    for f in items {
        for part in f["parts"].as_array().into_iter().flatten() {
            pts.extend(part.as_array().into_iter().flatten().map(xy));
        }
        if f["xyz"].is_array() {
            pts.push(xy(&f["xyz"]));
        }
    }
    let pts: Vec<(f64, f64)> = pts
        .into_iter()
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .collect();
    if pts.is_empty() {
        return None;
    }
    let fold = |f: fn(f64, f64) -> f64, init: f64, k: usize| {
        pts.iter()
            .map(|p| if k == 0 { p.0 } else { p.1 })
            .fold(init, f)
    };
    Some((
        fold(f64::min, f64::INFINITY, 0),
        fold(f64::min, f64::INFINITY, 1),
        fold(f64::max, f64::NEG_INFINITY, 0),
        fold(f64::max, f64::NEG_INFINITY, 1),
    ))
}

/// Подпись пикета на языке показа: «ПК5+20», «PK5+20».
pub fn pk_label(m: f64, lang: Lang) -> String {
    let (n, plus) = geodemo::pk_parts(m);
    let prefix = match lang {
        Lang::Ru => "ПК",
        Lang::En => "PK",
    };
    format!("{prefix}{n}+{plus:02}")
}

fn min_max(a: &[f64]) -> (f64, f64) {
    (
        a.iter().copied().fold(f64::INFINITY, f64::min),
        a.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    )
}

fn distinct(a: &[i64]) -> usize {
    let mut v = a.to_vec();
    v.sort_unstable();
    v.dedup();
    v.len()
}

pub fn run(v: &Values<'_>, report: &mut Report) -> Result<J, ToolError> {
    let ext = bbox(v.items("EXTENT"))
        .filter(|e| e.2 > e.0 && e.3 > e.1)
        .ok_or_else(|| ToolError::new("Не задан охват.", args![]))?;
    let subsidence = v.integer("MODE") == 1;
    let frame = Frame {
        ext,
        n_profiles: v.integer("N_PROFILES"),
        picket_step: v.number("PICKET_STEP"),
        z_base: v.number("Z_BASE"),
        z_amp: v.number("Z_AMP"),
    };
    let mut rng = Rng::from_module_seed(v.integer("SEED"));
    let mut names = vec!["profile", "picket_m", "pk"];
    let d = if subsidence {
        let subs = v.number("SUBS_MAX");
        names.extend(["tour", "z", "settle", "settle_true"]);
        geodemo::gen_subsidence(
            &frame,
            &Subsidence {
                subs_max: subs,
                n_tours: v.integer("N_TOURS"),
                noise: (subs * 0.01).max(1.0),
                positive: v.integer("SUBS_SIGN") == 1,
            },
            &mut rng,
        )
    } else {
        names.extend(["z", "rho_k", "rho_true", "sp", "vp"]);
        geodemo::gen_profiles(
            &frame,
            &Electro {
                rho_bg: v.number("RHO_BG"),
                rho_min: v.number("RHO_MIN"),
                sp_amp: v.number("SP_AMP"),
                vp_bg: 5.0,
                vp_amp: v.number("VP_AMP"),
                noise: v.number("NOISE"),
            },
            &mut rng,
        )
    }
    .ok_or_else(|| ToolError::new("Не задан охват.", args![]))?;
    let plan = plan_for(&names, FIELD_LABELS, &[], &v.input_fields());
    let n = d.profile.len();
    let features: Vec<J> = (0..n)
        .map(|i| {
            let raw: Vec<J> = names
                .iter()
                .map(|name| match *name {
                    "profile" => json!(d.profile[i]),
                    "tour" => json!(d.tour[i]),
                    "pk" => json!(pk_label(d.picket_m[i], v.lang)),
                    "picket_m" => json!(d.picket_m[i]),
                    "z" => json!(d.z[i]),
                    "rho_k" => json!(d.rho_k[i]),
                    "rho_true" => json!(d.rho_true[i]),
                    "sp" => json!(d.sp[i]),
                    "vp" => json!(d.vp[i]),
                    "settle" => json!(d.settle[i]),
                    _ => json!(d.settle_true[i]),
                })
                .collect();
            let (values, raw) = row(&names, raw, &plan);
            json!({"geometry": {"xyz": [d.x[i], d.y[i], null]}, "values": values, "raw": raw})
        })
        .collect();
    let types = |name: &str| match name {
        "profile" | "tour" => "int",
        "pk" => "string",
        _ => "double",
    };
    let fields: Vec<J> = names
        .iter()
        .map(|n| json!({"name": n, "type": types(n)}))
        .collect();
    let n_prof = distinct(&d.profile);
    if subsidence {
        report.info(
            "Оседания: профилей %d, туров %d, точек %d. Мульда до %.1f мм. \
             Разность settle между турами по одним пикетам даёт скорость оседания.",
            args![n_prof, distinct(&d.tour), n, min_max(&d.settle).0],
        );
    } else {
        let (rlo, rhi) = min_max(&d.rho_k);
        let (slo, shi) = min_max(&d.sp);
        report.info(
            "Электроразведка: профилей %d, точек %d. ρк %.4g..%.4g Ом·м, ЕП %.1f..%.1f мВ.",
            args![n_prof, n, rlo, rhi, slo, shi],
        );
        report.info(
            "Интерполируйте rho_k (2D Kriging или минимальная кривизна) и постройте \
             изолинии - аномалия-пятно оконтурится. Поле rho_true - эталон без шума \
             для проверки точности.",
            args![],
        );
    }
    Ok(json!({"OUTPUT": {"geometry": "point", "fields": fields, "features": features}}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_of_parts_and_points() {
        let items = vec![
            json!({"parts": [[[1.0, 5.0, null], [3.0, 2.0, null]]]}),
            json!({"xyz": [7.0, 4.0, null]}),
            json!({"xyz": [null, 9.0, null]}),
        ];
        assert_eq!(bbox(&items), Some((1.0, 2.0, 7.0, 5.0)));
        assert_eq!(bbox(&[]), None);
    }

    #[test]
    fn pk_in_both_languages() {
        assert_eq!(pk_label(520.0, Lang::Ru), "ПК5+20");
        assert_eq!(pk_label(5.0, Lang::En), "PK0+05");
    }
}
