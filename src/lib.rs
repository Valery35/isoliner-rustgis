// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Isoliner для RustGIS - расширение-процесс RustGIS на крейте
//! `rustgis-kit`. Инструменты модуля QGIS Isoliner считаются здесь
//! по тем же описаниям (`fixtures/tools.json`) и сверяются с прогонами
//! модуля (`fixtures/tool_runs.json`).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod declus;
pub mod mba;
pub mod demo_fields;
pub mod variogram_map;
pub mod density;
pub mod drillhole;
pub mod field_format;
pub mod fractal;
pub mod geodemo;
pub mod geotiff;
pub mod hydro;
pub mod kb2d;
pub mod krige;
pub mod mincurv;
pub mod npy_rng;
pub mod numpy;
pub mod points;
pub mod product;
pub mod pyfmt;
pub mod raster_cmd;
pub mod report;
pub mod subsidence;
pub mod tools;
pub mod topo_surface;
pub mod validate;
pub mod vgtable;
// константы NumPy как есть, со всеми знаками исходника
#[allow(clippy::excessive_precision, clippy::unreadable_literal)]
pub mod ziggurat;
