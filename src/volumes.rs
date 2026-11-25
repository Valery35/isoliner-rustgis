// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 2.18 Насыпи и выемки: объёмы между двумя поверхностями.
//! Порт `volumes.py` модуля QGIS.
//!
//! Считает объём между «было» (ground) и «стало» (plan):
//! положительное — выемка (снять), отрицательное — насыпь (добавить).

/// Отчёт по объёмам.
#[derive(Debug, Clone, Default)]
pub struct VolumeReport {
    /// Общая выемка (м³).
    pub cut: f64,
    /// Общая насыпь (м³).
    pub fill: f64,
    /// Разница: выемка минус насыпь.
    pub net: f64,
    /// Ячеек с выемкой.
    pub cut_cells: usize,
    /// Ячеек с насыпью.
    pub fill_cells: usize,
    /// Ячеек без изменения.
    pub flat_cells: usize,
}

/// Объёмы между двумя поверхностями.
/// ground — «было», plan — «стало», cell_area — площадь ячейки (м²).
pub fn cut_fill(
    ground: &[f64],
    plan: &[f64],
    cell_area: f64,
    nodata: f64,
) -> VolumeReport {
    let mut rep = VolumeReport::default();
    for k in 0..ground.len() {
        let g = ground[k];
        let p = plan[k];
        if g == nodata || p == nodata || !g.is_finite() || !p.is_finite() {
            continue;
        }
        let diff = g - p; // положительное — снимать (выемка).
        if diff > 0.0 {
            rep.cut += diff * cell_area;
            rep.cut_cells += 1;
        } else if diff < 0.0 {
            rep.fill += (-diff) * cell_area;
            rep.fill_cells += 1;
        } else {
            rep.flat_cells += 1;
        }
    }
    rep.net = rep.cut - rep.fill;
    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Плоскость 10 против плоскости 8: выемка 2 м по всей площади.
    #[test]
    fn uniform_cut() {
        let ground = vec![10.0f64; 100];
        let plan = vec![8.0f64; 100];
        let rep = cut_fill(&ground, &plan, 1.0, -9999.0);
        assert!((rep.cut - 200.0).abs() < 1e-10, "cut={}", rep.cut);
        assert!(rep.fill == 0.0);
        assert_eq!(rep.cut_cells, 100);
    }

    /// В половине выемка, в половине насыпь.
    #[test]
    fn half_cut_half_fill() {
        let ground = vec![10.0f64; 10];
        let mut plan = vec![10.0f64; 10];
        // Половина: снять 5.
        for p in plan.iter_mut().take(5) {
            *p = 5.0;
        }
        // Половина: насыпить 3.
        for p in plan.iter_mut().skip(5).take(5) {
            *p = 13.0;
        }
        let rep = cut_fill(&ground, &plan, 2.0, -9999.0);
        assert!((rep.cut - 50.0).abs() < 1e-10, "cut={}", rep.cut);
        assert!((rep.fill - 30.0).abs() < 1e-10, "fill={}", rep.fill);
        assert!((rep.net - 20.0).abs() < 1e-10, "net={}", rep.net);
    }
}
