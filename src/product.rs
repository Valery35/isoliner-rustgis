// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Isoliner как продукт крейта `rustgis-kit`. Протокол RustGIS, окно
//! инструмента, перевод координат и итоги слоями - у крейта. Здесь
//! расчёт инструментов и свои команды.

use rustgis_kit::catalog::Tool;
use rustgis_kit::runner::no_command;
use rustgis_kit::{Call, Catalog, Failure, Lang, Options, Product, Store};
use serde_json::{Value as J, json};

use crate::report::{Report, render};
use crate::tools;

/// Описания инструментов, выгрузка модуля (`tools/export_tools.py`).
pub const TOOLS_JSON: &str = include_str!("../fixtures/tools.json");

/// Основа `plugin.json` без раздела `tools`.
pub const MANIFEST: &str = include_str!("../plugin.json");

pub const OPTIONS: Options = Options {
    toolbox: "Isoliner",
    offered: tools::OFFERED,
    xy_tables: &[],
};

/// Папка данных расширения во временной папке системы.
pub fn home() -> std::path::PathBuf {
    rustgis_kit::store::temp_home("isoliner")
}

/// Поля, которых нет в окне RustGIS: RustGIS принимает у инструмента не
/// больше 32 полей, а у 1.09 их 37. Скрыты дополнительные настройки
/// решателя минимальной кривизны, расчёт берёт для них умолчания модуля.
pub const HIDDEN: &[(&str, &[&str])] = &[(
    "method_crossvalidation",
    &[
        "MAX_RESIDUAL",
        "MAX_ITER",
        "BOUNDARY_TENSION",
        "RELAX",
        "ANISO",
    ],
)];

/// Описания инструментов для окна RustGIS: `tools.json` без [`HIDDEN`].
pub fn catalog() -> Result<Catalog, String> {
    let mut c = Catalog::parse(TOOLS_JSON)?;
    for t in &mut c.tools {
        if let Some((_, keys)) = HIDDEN.iter().find(|(n, _)| *n == t.name) {
            t.params.retain(|p| !keys.contains(&p.key.as_str()));
        }
    }
    Ok(c)
}

pub struct Isoliner {
    pub catalog: Catalog,
}

impl Isoliner {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            catalog: catalog()?,
        })
    }
}

impl Product for Isoliner {
    fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    fn options(&self) -> &Options {
        &OPTIONS
    }

    fn store(&self) -> Store {
        Store::new(home().join("results"))
    }

    fn compute(&self, tool: &Tool, values: &J, lang: Lang) -> Result<(J, Vec<String>), Failure> {
        let mut report = Report::default();
        let result = tools::run(tool, values, lang, &mut report)
            .ok_or_else(|| Failure::Run(tool.name.clone()))?;
        let messages = report.lines.iter().map(|l| l.show(tool, lang)).collect();
        match result {
            Ok(out) => Ok((out, messages)),
            Err(e) => Err(Failure::Run(render(tool, &e.template, &e.args, lang))),
        }
    }

    fn command(&self, _host: &mut dyn Call, req: &J, lang: Lang) -> J {
        match req["id"].as_str().unwrap_or("") {
            "about" => json!({"ok": {
                "title": "Isoliner",
                "version": env!("CARGO_PKG_VERSION"),
                "module": self.catalog.version,
                "tools": tools::OFFERED.len(),
            }}),
            id => no_command(id, lang),
        }
    }
}

/// `plugin.json` с разделом `tools` и переводами его строк.
pub fn manifest(catalog: &Catalog, base: &str) -> Result<String, String> {
    let (tools, ru) = rustgis_kit::manifest::tools_section(catalog, &OPTIONS);
    rustgis_kit::manifest::splice(base, &tools, &ru)
}
