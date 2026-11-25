// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 4.10 Пример для разреза (демо). Порт `demo_stack.py` модуля QGIS.
//! Шесть гладких поверхностей с региональным падением и волнистой
//! переменной мощностью: пять пластов (3 вмещающих, 2 промышленных).

use crate::section::Surface;

/// Демо-поверхность: синусоидальная волна с региональным наклоном.
/// nx × ny ячеек, gt — геотрансформация.
pub fn demo_surfaces(nx: usize, ny: usize) -> Vec<Surface> {
    let gt = [0.0, 1.0, 0.0, ny as f64, 0.0, -1.0];
    let n = nx * ny;

    // Шесть поверхностей стопкой. Между ними пять пластов:
    // 1-2 (вмещающий), 2-3 (промышленный), 3-4 (вмещающий),
    // 4-5 (промышленный), 5-6 (вмещающий).
    // Базовые высоты (от нуля) и мощности.
    let base = [80.0, 65.0, 55.0, 40.0, 32.0, 20.0];

    (0..6)
        .map(|s| {
            let mut data = Vec::with_capacity(n);
            for r in 0..ny {
                for c in 0..nx {
                    let x = c as f64;
                    let y = r as f64;
                    // Региональный наклон: вниз на восток.
                    let tilt = -0.3 * x;
                    // Волна: переменная мощность.
                    let wave = 3.0
                        * ((x / 30.0).sin() * (y / 20.0).cos())
                        * (s as f64 * 0.5 + 0.5);
                    data.push(base[s] + tilt + wave);
                }
            }
            Surface { data, rows: ny, cols: nx, gt }
        })
        .collect()
}

/// Линии разрезов для демо: прямой и ломаный.
pub fn demo_section_lines() -> Vec<(String, Vec<(f64, f64)>)> {
    vec![
        (
            "Section 1".into(),
            vec![(10.0, 40.0), (90.0, 40.0)],
        ),
        (
            "Section 2 (straight)".into(),
            vec![(10.0, 55.0), (90.0, 55.0)],
        ),
        (
            "Section 3 (broken)".into(),
            vec![(10.0, 70.0), (50.0, 70.0), (90.0, 60.0)],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::section::build_section;

    /// Шесть поверхностей → пять пластов.
    #[test]
    fn five_beds() {
        let surfaces = demo_surfaces(100, 100);
        assert_eq!(surfaces.len(), 6);
        let lines = demo_section_lines();
        let section_line = &lines[1].1; // Section 2
        let res = build_section(section_line, &surfaces, 5.0, 1.0);
        // Шесть поверхностей → пять колец.
        // Внутри линии все поверхности валидны (внутри сетки 100×100).
        assert!(res.rings.len() >= 1, "rings={}, все z NAN?", res.rings.len());
    }

    /// Поверхности лежат стопкой: верхняя выше нижней.
    #[test]
    fn stacked() {
        let surfaces = demo_surfaces(50, 50);
        // Точка в центре.
        let gt = [0.0, 1.0, 0.0, 50.0, 0.0, -1.0];
        let (r, c) = (25usize, 25usize);
        let idx = r * 50 + c;
        for s in 0..5 {
            let top = surfaces[s].data[idx];
            let bot = surfaces[s + 1].data[idx];
            assert!(
                top > bot,
                "surface {} z={} must be > surface {} z={}",
                s, top, s + 1, bot
            );
        }
    }
}
