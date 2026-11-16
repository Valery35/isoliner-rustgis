// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Морфометрия поверхности, порт `grid_isolines/topo_surface.py`:
//! уклон и экспозиция по Horn (1981, ядро 3x3, как в gdaldem).

/// Уклон (градусы) и экспозиция (градусы от севера по часовой).
///
/// Horn 3x3, край продлевается крайним значением (`np.pad` mode edge).
/// Экспозиция плоских ячеек (нулевой градиент) равна -1. Ячейки без
/// данных и их соседи получают NaN в обоих выходах: ядро 3x3 через
/// пропуски не считается.
pub fn slope_aspect(
    z: &[f64],
    mask: &[bool],
    width: usize,
    height: usize,
    cell: f64,
) -> (Vec<f64>, Vec<f64>) {
    let n = width * height;
    let mut slope = vec![f64::NAN; n];
    let mut aspect = vec![f64::NAN; n];
    if width == 0 || height == 0 {
        return (slope, aspect);
    }
    // значение соседа с продлением края; NaN у ячейки без данных
    let at = |r: isize, c: isize| -> f64 {
        let r = r.clamp(0, height as isize - 1) as usize;
        let c = c.clamp(0, width as isize - 1) as usize;
        let i = r * width + c;
        if mask[i] { f64::NAN } else { z[i] }
    };
    for r in 0..height {
        for c in 0..width {
            let (ri, ci) = (r as isize, c as isize);
            let nw = at(ri - 1, ci - 1);
            let n_ = at(ri - 1, ci);
            let ne = at(ri - 1, ci + 1);
            let w_ = at(ri, ci - 1);
            let e_ = at(ri, ci + 1);
            let sw = at(ri + 1, ci - 1);
            let s_ = at(ri + 1, ci);
            let se = at(ri + 1, ci + 1);
            let dzdx = ((ne + 2.0 * e_ + se) - (nw + 2.0 * w_ + sw)) / (8.0 * cell);
            // строка 0 северная: положительный dzdy - рост на север
            let dzdy = ((nw + 2.0 * n_ + ne) - (sw + 2.0 * s_ + se)) / (8.0 * cell);
            let i = r * width + c;
            let s = dzdx.hypot(dzdy).atan().to_degrees();
            if !s.is_finite() || mask[i] {
                continue;
            }
            // экспозиция - азимут спуска: вектор (-dzdx, -dzdy) в осях (восток, север)
            let mut a = (-dzdx).atan2(-dzdy).to_degrees();
            if a < 0.0 {
                a += 360.0;
            }
            if dzdx == 0.0 && dzdy == 0.0 {
                a = -1.0;
            }
            slope[i] = s;
            aspect[i] = a;
        }
    }
    (slope, aspect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_rising_north_faces_south() {
        // плоскость z = 2 * y (на север растёт), ячейка 1: уклон atan(2),
        // спуск на юг - экспозиция 180
        let (w, h) = (4, 3);
        let z: Vec<f64> = (0..h)
            .flat_map(|r| (0..w).map(move |_| 2.0 * (h - 1 - r) as f64))
            .collect();
        let mask = vec![false; w * h];
        let (s, a) = slope_aspect(&z, &mask, w, h, 1.0);
        let i = w + 1;
        assert!((s[i] - 2f64.atan().to_degrees()).abs() < 1e-12);
        assert!((a[i] - 180.0).abs() < 1e-12);
    }

    #[test]
    fn flat_and_holes() {
        let (w, h) = (3, 3);
        let z = vec![5.0; 9];
        let mut mask = vec![false; 9];
        let (s, a) = slope_aspect(&z, &mask, w, h, 2.0);
        assert_eq!(s[4], 0.0);
        assert_eq!(a[4], -1.0);
        mask[0] = true;
        let (s, a) = slope_aspect(&z, &mask, w, h, 2.0);
        // сосед пропуска тоже без данных, дальняя ячейка считается
        assert!(s[0].is_nan() && s[1].is_nan() && s[4].is_nan() && a[4].is_nan());
        assert_eq!(s[8], 0.0);
    }
}
