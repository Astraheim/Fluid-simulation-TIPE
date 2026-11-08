//! Exploratory cargo criteria. Numerical forces require an explicit calibration.
use crate::{
    experiments::Config,
    grid::{Grid, Vector2},
    ship::WallKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
pub const MAX_TIERS: usize = 10;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Safety {
    pub mass_kg: f64,
    pub base_m: f64,
    pub height_m: f64,
    /// Zero means uncalibrated, never interpreted as zero aerodynamic force.
    pub newtons_per_unit: f64,
}
impl Default for Safety {
    fn default() -> Self {
        Self {
            mass_kg: 15000.0,
            base_m: 6.058,
            height_m: 2.591,
            newtons_per_unit: 0.0,
        }
    }
}
impl Safety {
    pub fn valid(&self) -> bool {
        [self.mass_kg, self.base_m, self.height_m]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
            && self.newtons_per_unit.is_finite()
            && self.newtons_per_unit >= 0.0
    }
    pub fn centre_force_limit(&self) -> f64 {
        self.mass_kg * 9.81 * self.base_m / self.height_m
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub left: usize,
    pub right: usize,
    pub top: usize,
    pub bottom: usize,
}
pub fn bounds(c: &Config) -> BTreeMap<usize, Bounds> {
    let mut out = BTreeMap::new();
    for b in &c.bands {
        if let Some(WallKind::Container(id)) = b.kind {
            let r = out.entry(id).or_insert(Bounds {
                left: b.x,
                right: b.x,
                top: b.first,
                bottom: b.last,
            });
            r.left = r.left.min(b.x);
            r.right = r.right.max(b.x);
            r.top = r.top.min(b.first);
            r.bottom = r.bottom.max(b.last);
        }
    }
    out
}
pub fn tiers(c: &Config) -> usize {
    let boxes = bounds(c);
    (0..=c.n + 1)
        .map(|x| {
            boxes
                .values()
                .filter(|b| b.left <= x && x <= b.right)
                .count()
        })
        .max()
        .unwrap_or(0)
}
/// Fraction of boxes adjacent to a clear vertical strip, open from above.
/// A 2D geometric proxy, not a crew-access or crane-clearance certification.
pub fn access(c: &Config) -> f64 {
    let boxes = bounds(c);
    if boxes.is_empty() {
        return 0.0;
    }
    let top = boxes.values().map(|b| b.top).min().unwrap();

    let accessible = boxes
        .values()
        .filter(|b| {
            let gap = ((b.right - b.left + 1) as f64 * 0.1).ceil() as usize;
            let clear = |left: usize, right: usize| {
                !c.bands.iter().any(|wall| {
                    left <= wall.x && wall.x <= right && wall.first <= b.bottom && wall.last >= top
                })
            };
            (b.left >= gap && clear(b.left - gap, b.left - 1))
                || (b.right + gap <= c.n && clear(b.right + 1, b.right + gap))
        })
        .count();
    accessible as f64 / boxes.len() as f64
}
/// Returns mean absolute resultant force and maximum overturning/lift ratio.
/// Gravity at box centre; forces integrated about both bottom edges. No lashing,
/// friction, ship motion, or transfer of loads between boxes is modelled.
pub fn loads(
    c: &Config,
    g: &Grid,
    labels: &HashMap<usize, WallKind>,
    forces: &[Vector2],
    boxes: &BTreeMap<usize, Bounds>,
) -> (f64, Option<f64>) {
    let mut sums: BTreeMap<usize, [f64; 4]> = BTreeMap::new();
    let mut indices: Vec<_> = labels.keys().copied().collect();
    indices.sort_unstable();
    for idx in indices {
        if let WallKind::Container(id) = labels[&idx] {
            let (x, y) = g.decode_index(idx);
            if y as f32 >= c.waterline {
                continue;
            }
            let b = boxes[&id];
            let f = forces[idx];
            let a = sums.entry(id).or_insert([0.0; 4]);
            let dx = c.safety.base_m / (b.right - b.left + 1) as f64;
            let dy = c.safety.height_m / (b.bottom - b.top + 1) as f64;
            let ry = (y as f64 - b.bottom as f64 - 0.5) * dy;
            a[0] += f.x as f64;
            a[1] += f.y as f64;
            a[2] += (x as f64 - b.left as f64 + 0.5) * dx * f.y as f64 - ry * f.x as f64;
            a[3] += (x as f64 - b.right as f64 - 0.5) * dx * f.y as f64 - ry * f.x as f64;
        }
    }
    let mean = sums.values().map(|a| a[0].hypot(a[1])).sum::<f64>() / boxes.len().max(1) as f64;
    let ratio = if c.safety.newtons_per_unit > 0.0 {
        let weight = c.safety.mass_kg * 9.81;
        let restoring = weight * c.safety.base_m / 2.0;
        Some(
            sums.values()
                .map(|a| (-a[2]).max(a[3]).max(0.0) * c.safety.newtons_per_unit / restoring)
                .chain(
                    sums.values()
                        .map(|a| (-a[1]).max(0.0) * c.safety.newtons_per_unit / weight),
                )
                .fold(0.0, f64::max),
        )
    } else {
        None
    };
    (mean, ratio)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::experiments::Band;
    fn config() -> Config {
        let mut c = Config::capture(&Grid::new(), &HashMap::new());
        c.bands.clear();
        c.waterline = 400.0;
        c
    }
    fn add(c: &mut Config, id: usize, x: usize, y: usize) {
        for x in x..x + 10 {
            c.bands.push(Band {
                x,
                first: y,
                last: y + 9,
                kind: Some(WallKind::Container(id)),
            });
        }
    }
    #[test]
    fn tiers_and_access_use_geometry() {
        let mut c = config();
        for i in 0..11 {
            add(&mut c, i, 20, 20 + i * 10);
        }
        assert_eq!(tiers(&c), 11);
        let mut packed = config();
        for i in 0..5 {
            add(&mut packed, i, 20 + i * 10, 20);
        }
        let mut spaced = config();
        for i in 0..5 {
            add(&mut spaced, i, 20 + i * 14, 20);
        }
        assert!(access(&spaced) > access(&packed));
    }
    #[test]
    fn overturning_both_directions_and_lift() {
        let mut c = config();
        add(&mut c, 0, 20, 20);
        let (g, l) = c.grid().unwrap();
        let b = bounds(&c);
        let mut f = vec![Vector2::default(); g.cells.len()];
        let idx = g.to_index(25, 25);
        f[idx].x = 1e6;
        assert_eq!(loads(&c, &g, &l, &f, &b).1, None);
        c.safety.newtons_per_unit = 1.0;
        assert!(loads(&c, &g, &l, &f, &b).1.unwrap() > 1.0);
        f[idx].x = -1e6;
        assert!(loads(&c, &g, &l, &f, &b).1.unwrap() > 1.0);
        f[idx].x = 0.0;
        f[idx].y = -200000.0;
        assert!(loads(&c, &g, &l, &f, &b).1.unwrap() > 1.0);
        f[idx].y = 0.0;
        assert_eq!(loads(&c, &g, &l, &f, &b).1, Some(0.0));
    }
    #[test]
    fn too_tall_cannot_start_an_experiment() {
        let mut c = config();
        for i in 0..11 {
            add(&mut c, i, 20, 20 + i * 10);
        }
        assert!(
            crate::experiments::Recorder::new(c, std::path::Path::new("unused-invalid-test"))
                .is_err()
        );
    }
    #[test]
    fn legacy_config_and_bonus_bound() {
        let mut c = config();
        add(&mut c, 0, 20, 20);
        let mut value = serde_json::to_value(&c).unwrap();
        value.as_object_mut().unwrap().remove("safety");
        let old: Config = serde_json::from_value(value).unwrap();
        assert_eq!(old.safety.newtons_per_unit, 0.0);
        let mut stats = BTreeMap::new();
        stats.insert("Fx".into(), crate::experiments::Stats::default());
        stats.insert(
            "container_load_mean_abs".into(),
            crate::experiments::Stats::default(),
        );
        let mut e = crate::experiments::Entry {
            config: c.clone(),
            folder: String::new(),
            status: "completed".into(),
            samples: 2,
            stats,
            source: String::new(),
        };
        let w = crate::experiments::Weights::default();
        let sheltered = w.score(&e);
        assert!((w.optimistic_score(&c) - sheltered).abs() < 1e-12);
        e.stats.get_mut("container_load_mean_abs").unwrap().mean = 100.0;
        assert!(w.score(&e) < sheltered);
    }
    #[test]
    fn force_limit_is_moment_balance() {
        let s = Safety::default();
        assert!((s.centre_force_limit() * s.height_m - s.mass_kg * 9.81 * s.base_m).abs() < 1e-8);
    }
}
