// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Расчёты инструментов по значениям прогона модуля, как
//! `grid_isolines/tools/` модуля QGIS. Один файл на инструмент, имя -
//! имя Processing.

use rustgis_kit::Lang;
use rustgis_kit::catalog::Tool;
use serde_json::Value as J;

use crate::report::{Report, ToolError};

pub mod bed_grades;
pub mod crossval;
pub mod cv;
pub mod declustering;
pub mod geophys_profiles;
pub mod line_dimension;
pub mod method_cv;
pub mod minkowski;
pub mod variogram;

/// Инструменты расширения в порядке панели: имя Processing.
pub const OFFERED: &[&str] = &[
    "declustering",
    "geophysprofiles",
    "bed_grades_at_collars",
    "line_dimension",
    "experimental_variogram",
    "crossvalidation",
    "method_crossvalidation",
];

/// Значения прогона с умолчаниями из описания инструмента: поле, которое
/// RustGIS не прислал, берёт `default` из `tools.json`, как окно QGIS.
pub struct Values<'a> {
    pub tool: &'a Tool,
    pub values: &'a J,
    /// Язык показа: им пишутся данные со словами, например подпись пикета.
    pub lang: Lang,
}

impl<'a> Values<'a> {
    pub fn new(tool: &'a Tool, values: &'a J, lang: Lang) -> Self {
        Self { tool, values, lang }
    }

    /// Значение поля, а без него - умолчание описания.
    pub fn get(&self, key: &str) -> J {
        match &self.values[key] {
            J::Null => self
                .tool
                .param(key)
                .and_then(|p| p.default.clone())
                .unwrap_or(J::Null),
            v => v.clone(),
        }
    }

    pub fn number(&self, key: &str) -> f64 {
        crate::points::num(&self.get(key)).unwrap_or(f64::NAN)
    }

    /// Целое: дробное отбрасывается к нулю, как `int()` Python, пусто - 0.
    pub fn integer(&self, key: &str) -> i64 {
        let v = self.get(key);
        v.as_i64()
            .or_else(|| {
                crate::points::num(&v)
                    .filter(|x| x.is_finite())
                    .map(|x| x.trunc() as i64)
            })
            .unwrap_or(0)
    }

    /// Флажок: `bool()` Python от значения.
    pub fn flag(&self, key: &str) -> bool {
        match self.get(key) {
            J::Bool(b) => b,
            J::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
            J::String(s) => !s.is_empty(),
            J::Array(a) => !a.is_empty(),
            J::Object(o) => !o.is_empty(),
            J::Null => false,
        }
    }

    pub fn text(&self, key: &str) -> String {
        self.get(key).as_str().unwrap_or("").to_string()
    }

    /// Объекты слоя (`[{"id", "xyz" | "parts", "attrs"}]`).
    pub fn items(&self, key: &str) -> &'a [J] {
        self.values[key].as_array().map_or(&[], Vec::as_slice)
    }

    /// Сведения о слое `@КЛЮЧ`.
    pub fn layer(&self, key: &str) -> &'a J {
        &self.values[format!("@{key}").as_str()]
    }

    /// Поля всех слоёв на входе: модуль их не округляет на выходе.
    pub fn input_fields(&self) -> Vec<String> {
        self.values
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(k, _)| k.starts_with('@'))
            .flat_map(|(_, l)| l["fields"].as_array().cloned().unwrap_or_default())
            .filter_map(|f| f.as_str().map(str::to_string))
            .collect()
    }
}

/// Расчёт инструмента по имени: выходы по ключам. `None` - такого
/// инструмента у расширения нет.
pub fn run(
    tool: &Tool,
    values: &J,
    lang: Lang,
    report: &mut Report,
) -> Option<Result<J, ToolError>> {
    let v = Values::new(tool, values, lang);
    match tool.name.as_str() {
        "declustering" => Some(declustering::run(&v, report).map(|(out, _)| out)),
        "geophysprofiles" => Some(geophys_profiles::run(&v, report)),
        "bed_grades_at_collars" => Some(bed_grades::run(&v, report)),
        "line_dimension" => Some(line_dimension::run(&v, report)),
        "experimental_variogram" => Some(variogram::run(&v, report)),
        "crossvalidation" => Some(crossval::run(&v, report)),
        "method_crossvalidation" => Some(method_cv::run(&v, report)),
        "minkowski_dimension" => Some(minkowski::run(&v, report)),
        _ => None,
    }
}
