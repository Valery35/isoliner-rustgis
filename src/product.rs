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

/// Своя вкладка ленты Isoliner: растровые команды сгруппированы по
/// разделам модуля QGIS. Инструменты остаются в панели «Инструменты».
pub mod tab {
    use rustgis_kit::manifest::{Group, Item, Label, Layout, Menu, Tab};

    const fn l(en: &'static str, ru: &'static str) -> Label {
        Label { en, ru }
    }

    static GRID: Menu = Menu {
        label: l("Grid and isolines", "Грид и изолинии"),
        icon: Some("catalog"),
        help: Some(l("Grids, kriging and isolines", "Гриды, кригинг и изолинии")),
        layout: None,
        items: &[
            Item::Command("kriging-2d"),
            Item::Command("minimum-curvature"),
            Item::Command("mba-grid"),
            Item::Command("raster-to-isolines"),
            Item::Command("variogram-map"),
        ],
    };

    static TERRAIN: Menu = Menu {
        label: l("Terrain", "Рельеф"),
        icon: Some("relief"),
        help: Some(l("Relief from a DEM", "Рельеф по ЦМР")),
        layout: None,
        items: &[
            Item::Command("topo2raster"),
            Item::Command("fill-depressions"),
            Item::Command("slope-aspect"),
            Item::Command("peaks-pits"),
        ],
    };

    static HYDRO: Menu = Menu {
        label: l("Hydrology", "Гидрология"),
        icon: Some("chart"),
        help: Some(l("Flow, networks and catchments", "Сток, сети и водосборы")),
        layout: None,
        items: &[
            Item::Command("flow-d8"),
            Item::Command("river-network"),
            Item::Command("basins"),
            Item::Command("gauge-report"),
            Item::Command("ditch-catchment"),
            Item::Command("catchment-stats"),
            Item::Command("downhill-trace"),
        ],
    };

    static PREP: Menu = Menu {
        label: l("Diagnostics and cleanup", "Диагностика и правка"),
        icon: Some("table"),
        help: Some(l("Contours, breaklines and DEM cleanup", "Горизонтали, бровки и чистка ЦМР")),
        layout: None,
        items: &[
            Item::Command("bridge-removal"),
            Item::Command("snap-elevations"),
            Item::Command("terrace-smooth"),
            Item::Command("contour-residual"),
            Item::Command("terracing-check"),
            Item::Command("breakline-candidates"),
            Item::Command("breakline-pairs"),
            Item::Command("cut-fill"),
            Item::Command("topo-demo-pit"),
        ],
    };

    static EXTRA: Menu = Menu {
        label: l("Additional analysis", "Дополнительные"),
        icon: Some("chart"),
        help: Some(l("Probability and gradient", "Вероятность и градиент")),
        layout: None,
        items: &[Item::Command("probability-threshold"), Item::Command("flow-gradient")],
    };

    static SUBS: Menu = Menu {
        label: l("Subsidence", "Оседания"),
        icon: Some("measure"),
        help: Some(l("Tilt, curvature and strain", "Наклоны, кривизна и деформации")),
        layout: None,
        items: &[
            Item::Command("subsidence-tilt-curvature"),
            Item::Command("subsidence-horizontal"),
        ],
    };

    pub static TAB: Tab = Tab {
        label: l("Isoliner", "Isoliner"),
        groups: &[
            Group {
                label: l("1. Grids and kriging", "1. Гриды и кригинг"),
                layout: Layout::Large,
                items: &[Item::Menu(&GRID)],
            },
            Group {
                label: l("2. Topography", "2. Топография"),
                layout: Layout::Large,
                items: &[Item::Menu(&TERRAIN)],
            },
            Group {
                label: l("2. Topography: diagnostics", "2. Топография: диагностика"),
                layout: Layout::Large,
                items: &[Item::Menu(&PREP)],
            },
            Group {
                label: l("3. Additional tools", "3. Дополнительные инструменты"),
                layout: Layout::Large,
                items: &[Item::Menu(&EXTRA)],
            },
            Group {
                label: l("6. Hydrology", "6. Гидрология"),
                layout: Layout::Large,
                items: &[Item::Menu(&HYDRO)],
            },
            Group {
                label: l("9. Subsidence", "9. Оседания"),
                layout: Layout::Large,
                items: &[Item::Menu(&SUBS)],
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

/// Описания инструментов для окна RustGIS. RustGIS 0.8.0 принимает до
/// 64 полей на инструмент, поэтому все поля 1.09 идут в окно.
pub fn catalog() -> Result<Catalog, String> {
    Catalog::parse(TOOLS_JSON)
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
            "contour-residual" => {
                match crate::raster_cmd::contour_residual(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "terracing-check" => {
                match crate::raster_cmd::terracing_check(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "gauge-report" => {
                match crate::raster_cmd::gauge_report(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "ditch-catchment" => {
                match crate::raster_cmd::ditch_catchment(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "catchment-stats" => {
                match crate::raster_cmd::catchment_stats(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "cut-fill" => match crate::raster_cmd::cut_fill_cmd(host, &req["params"], lang) {
                Ok(v) => json!({"ok": v}),
                Err(e) => json!({"error": e}),
            },
            "breakline-candidates" => {
                match crate::raster_cmd::breakline_candidates(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "topo-demo-pit" => {
                match crate::raster_cmd::topo_demo_pit(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "snap-elevations" => {
                match crate::raster_cmd::snap_elevations(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "downhill-trace" => {
                match crate::raster_cmd::downhill_trace(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "bridge-removal" => {
                match crate::raster_cmd::bridge_removal(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "breakline-pairs" => {
                match crate::raster_cmd::breakline_pairs(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "topo2raster" => {
                match crate::raster_cmd::topo2raster_cmd(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "mba-grid" => match crate::raster_cmd::mba_grid(host, &req["params"], lang) {
                Ok(v) => json!({"ok": v}),
                Err(e) => json!({"error": e}),
            },
            "raster-to-isolines" => {
                match crate::raster_cmd::raster_to_isolines(host, &req["params"], lang) {
                    Ok(v) => json!({"ok": v}),
                    Err(e) => json!({"error": e}),
                }
            }
            "variogram-map" => {
                match crate::raster_cmd::variogram_map_cmd(host, &req["params"], lang) {
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
            "about" => {
                let text = format!(
                    "Isoliner {} (module {}), {} tools",
                    env!("CARGO_PKG_VERSION"),
                    self.catalog.version,
                    tools::OFFERED.len()
                );
                let _ = host.call("host.notify", json!({"text": &text})).ok();
                json!({"ok": {"text": text}})
            }
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
        (1, _) => ("1. Grids and kriging", "1. Гриды и кригинг"),
        (2, n) if (8..=99).contains(&n) => (
            "2. Topography: diagnostics",
            "2. Топография: диагностика",
        ),
        (2, _) => ("2. Topography", "2. Топография"),
        (3, _) => ("3. Additional tools", "3. Дополнительные инструменты"),
        (4, _) => ("4. Section", "4. Разрез"),
        (5, _) => ("5. Geological model", "5. Геологическая модель"),
        (6, _) => ("6. Hydrology", "6. Гидрология"),
        (7, _) => ("7. Fractal analysis", "7. Фрактальный анализ"),
        (8, _) => ("8. Data exchange", "8. Обмен данными"),
        (9, _) => ("9. Subsidence", "9. Оседания"),
        _ => return None,
    })
}

/// Раскладывает инструменты по подгруппам QGIS (`toolbox`) и добавляет
/// переводы названий групп в `i18n`. Затем сортирует инструменты по
/// группе: RustGIS в панели Инструментов склеивает только соседние
/// одинаковые toolbox, поэтому иначе одна группа повторяется несколько раз.
pub fn apply_toolbox_groups(tools: &mut [J], ru: &mut Map<String, J>) {
    let tools_json: J = serde_json::from_str(TOOLS_JSON).unwrap_or_default();
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
        if let Some(id) = tool.get("id").and_then(|v| v.as_str())
            && let Some(&code) = code_by_id.get(id)
            && let Some((en, rus)) = qgis_group(code)
        {
            tool["toolbox"] = J::String(en.to_string());
            ru.insert(en.to_string(), J::String(rus.to_string()));
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
