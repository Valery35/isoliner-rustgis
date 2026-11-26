// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Isoliner как продукт крейта `rustgis-kit`. Протокол RustGIS, окно
//! инструмента, перевод координат и итоги слоями - у крейта. Здесь
//! расчёт инструментов и свои команды.

use rustgis_kit::catalog::Tool;
use rustgis_kit::runner::no_command;
use rustgis_kit::{Call, Catalog, Failure, Lang, Options, Product, Store};
use serde_json::{Map, Value as J, json};

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

/// Своя вкладка ленты Isoliner: только растровые команды, сгруппированные
/// по номерам разделов модуля QGIS. Инструменты остаются в панели
/// Инструментов, чтобы не дублировать их на ленте.
pub mod tab {
    use rustgis_kit::manifest::{Group, Item, Label, Layout, Tab};

    const fn l(en: &'static str, ru: &'static str) -> Label {
        Label { en, ru }
    }

    pub static TAB: Tab = Tab {
        label: l("Isoliner", "Isoliner"),
        groups: &[
            Group {
                label: l("1. Grid and isolines", "1. Грид и изолинии"),
                layout: Layout::Large,
                items: &[Item::Command("kriging-2d"), Item::Command("minimum-curvature")],
            },
            Group {
                label: l("2. Topography", "2. Топография"),
                layout: Layout::Large,
                items: &[
                    Item::Command("slope-aspect"),
                    Item::Command("fill-depressions"),
                    Item::Command("flow-d8"),
                    Item::Command("river-network"),
                    Item::Command("basins"),
                    Item::Command("peaks-pits"),
                ],
            },
            Group {
                label: l("3. Topography diagnostics", "3. Топография: диагностика"),
                layout: Layout::Large,
                items: &[Item::Command("terrace-smooth")],
            },
            Group {
                label: l("4. Additional analysis", "4. Анализ"),
                layout: Layout::Large,
                items: &[Item::Command("probability-threshold"), Item::Command("flow-gradient")],
            },
            Group {
                label: l("10. Subsidence", "10. Сдвигения"),
                layout: Layout::Large,
                items: &[
                    Item::Command("subsidence-tilt-curvature"),
                    Item::Command("subsidence-horizontal"),
                ],
            },
            Group {
                label: l("Help", "Справка"),
                layout: Layout::Large,
                items: &[Item::Command("about")],
            },
        ],
    };
}

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

    fn command(&self, host: &mut dyn Call, req: &J, lang: Lang) -> J {
        match req["id"].as_str().unwrap_or("") {
            "slope-aspect" => match crate::raster_cmd::slope_aspect(host, &req["params"], lang) {
                Ok(v) => json!({"ok": v}),
                Err(e) => json!({"error": e}),
            },
            "minimum-curvature" => {
                match crate::raster_cmd::minimum_curvature(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "kriging-2d" => match crate::raster_cmd::kriging_2d(host, &req["params"], lang) {
                Ok(v) => json!({"ok": v}),
                Err(e) => json!({"error": e}),
            }
            "probability-threshold" => {
                match crate::raster_cmd::probability_threshold(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "flow-gradient" => match crate::raster_cmd::flow_gradient(host, &req["params"], lang) {
                Ok(v) => json!({"ok": v}),
                Err(e) => json!({"error": e}),
            }
            "fill-depressions" => {
                match crate::raster_cmd::fill_depressions(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "flow-d8" => match crate::raster_cmd::flow_d8(host, &req["params"], lang) {
                Ok(v) => json!({"ok": v}),
                Err(e) => json!({"error": e}),
            },
            "river-network" => {
                match crate::raster_cmd::river_network(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "peaks-pits" => match crate::raster_cmd::peaks_pits(host, &req["params"], lang) {
                Ok(v) => json!({"ok": v}),
                Err(e) => json!({"error": e}),
            },
            "basins" => match crate::raster_cmd::basins(host, &req["params"], lang) {
                Ok(v) => json!({"ok": v}),
                Err(e) => json!({"error": e}),
            },
            "terrace-smooth" => {
                match crate::raster_cmd::terrace_smooth(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "subsidence-tilt-curvature" => {
                match crate::raster_cmd::subsidence_tilt_curvature(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "subsidence-horizontal" => {
                match crate::raster_cmd::subsidence_horizontal(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
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

/// Группа инструмента по его коду, как в дереве инструментов QGIS.
fn qgis_group(code: &str) -> Option<(&'static str, &'static str)> {
    let mut parts = code.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u32>().unwrap_or(0);
    Some(match (major, minor) {
        (1, _) => ("1. Grid and isolines", "1. Грид и изолинии"),
        (2, n) if n <= 10 => ("2. Topography", "2. Топография"),
        (2, _) => ("3. Topography: diagnostics and cleanup", "3. Топография: диагностика и правка"),
        (3, _) => ("4. Additional analysis tools", "4. Дополнительные инструменты анализа"),
        (4, _) => ("5. Section", "5. Разрез"),
        (5, _) => ("6. Geological model", "6. Геологическая модель"),
        (6, _) => ("7. River hydrology", "7. Гидрология рек"),
        (7, _) => ("8. Fractal analysis", "8. Фрактальный анализ"),
        (8, _) => ("9. Data exchange", "9. Обмен данными"),
        (9, _) => ("10. Subsidence", "10. Сдвигения"),
        _ => return None,
    })
}

/// Раскладывает инструменты по подгруппам QGIS (`toolbox`) и добавляет
/// переводы названий групп в `i18n`. Затем сортирует инструменты по
/// группе: RustGIS в панели Инструментов склеивает только соседние
/// одинаковые toolbox, поэтому иначе одна группа повторяется несколько раз.
pub fn apply_toolbox_groups(tools: &mut Vec<J>, ru: &mut Map<String, J>) {
    let tools_json: J = serde_json::from_str(TOOLS_JSON).expect("tools.json is valid");
    let mut code_by_id = std::collections::HashMap::new();
    if let Some(arr) = tools_json["tools"].as_array() {
        for t in arr {
            if let (Some(name), Some(code)) = (t["name"].as_str(), t["code"].as_str()) {
                // Имя в tools.json с подчёркиваниями, в plugin.json id заменяется на дефисы.
                code_by_id.insert(name.replace('_', "-"), code);
                code_by_id.insert(name.to_string(), code);
            }
        }
    }
    for tool in tools.iter_mut() {
        if let Some(id) = tool.get("id").and_then(|v| v.as_str()) {
            if let Some(&code) = code_by_id.get(id) {
                if let Some((en, rus)) = qgis_group(code) {
                    tool["toolbox"] = J::String(en.to_string());
                    ru.insert(en.to_string(), J::String(rus.to_string()));
                }
            }
        }
    }
    // Устойчивая сортировка: инструменты одной группы идут подряд и в
    // порядке кода, группы - в порядке QGIS (числовой префикс).
    tools.sort_by(|a, b| {
        let key = |t: &J| t.get("toolbox").and_then(|v| v.as_str()).unwrap_or("").to_string();
        key(a).cmp(&key(b))
    });
}

/// `plugin.json` с разделами `tools` и `tab` и переводами их строк.
pub fn manifest(catalog: &Catalog, base: &str) -> Result<String, String> {
    let (mut tools, mut ru) = rustgis_kit::manifest::tools_section(catalog, &OPTIONS);
    apply_toolbox_groups(&mut tools, &mut ru);
    let base_val: J = serde_json::from_str(base).map_err(|e| e.to_string())?;
    let empty = Vec::new();
    let commands: Vec<&str> = base_val["commands"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .map(|c| c["id"].as_str().unwrap_or(""))
        .collect();
    let (tab, tab_ru) =
        rustgis_kit::manifest::tab_section(&tab::TAB, catalog, &OPTIONS, &commands)
            .map_err(|e| e.to_string())?;
    ru.extend(tab_ru);
    rustgis_kit::manifest::splice_all(base, &tools, Some(&tab), &ru)
}
