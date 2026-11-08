//! Exploratory cargo criteria. Numerical forces require an explicit calibration.
use crate::{
    experiments::Config,
    grid::{Grid, Vector2},
    ship::WallKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
pub const MAX_TIERS: usize = 10; // Valeur par défaut des anciennes sauvegardes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Safety {
    pub max_tiers: usize,
    pub wind_kmh: f64,
    pub friction: f64,
    pub drag_coefficient: f64,
    pub span_m: f64,
    pub wind_estimate_enabled: bool,
    pub mass_kg: f64,
    pub base_m: f64,
    pub height_m: f64,
    /// Zero means uncalibrated, never interpreted as zero aerodynamic force.
    pub newtons_per_unit: f64,
}
impl Default for Safety {
    fn default() -> Self {
        Self {
            max_tiers: MAX_TIERS,
            wind_kmh: 130.0,
            friction: 0.3,
            drag_coefficient: 1.2,
            span_m: 2.438,
            wind_estimate_enabled: true,
            mass_kg: 15000.0,
            base_m: 6.058,
            height_m: 2.591,
            newtons_per_unit: 0.0,
        }
    }
}
impl Safety {
    pub fn valid(&self) -> bool {
        [
            self.mass_kg,
            self.base_m,
            self.height_m,
            self.span_m,
            self.drag_coefficient,
        ]
        .iter()
        .all(|v| v.is_finite() && *v > 0.0)
            && self.friction.is_finite()
            && self.friction > 0.0
            && (1..=100).contains(&self.max_tiers)
            && self.wind_kmh.is_finite()
            && self.wind_kmh >= 0.0
            && self.newtons_per_unit.is_finite()
            && self.newtons_per_unit >= 0.0
    }
    /// Force horizontale estimée, surface frontale, rho_air = 1,225 kg/m³.
    /// Cd est une hypothèse réglable, pas une calibration du solveur.
    pub fn wind_force(&self, speed_mps: f64) -> f64 {
        0.5 * 1.225 * self.drag_coefficient * self.span_m * self.height_m * speed_mps.powi(2)
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
/// Rapports de glissement et de perte de contact, à chaque interface d'une
/// pile de boîtes alignées et jointives (tolérance de discrétisation : 1 cellule).
/// Forces horizontales supposées de même sens : enveloppe conservative.
/// Masses centrées, pas de twistlocks, pas de roulis, aucune prédiction d'angle.
pub fn stack_ratios(
    c: &Config,
    boxes: &BTreeMap<usize, Bounds>,
    forces: &BTreeMap<usize, f64>,
) -> (f64, f64) {
    let (mut slip, mut rocking) = (0.0_f64, 0.0_f64);
    for (&id, base) in boxes {
        let mut current = (id, base);
        let (mut count, mut shear, mut moment) = (0usize, 0.0, 0.0);
        loop {
            let force = forces.get(&current.0).copied().unwrap_or(0.0);
            if !force.is_finite() {
                return (f64::NAN, f64::NAN);
            }
            shear += force.abs();
            moment += force.abs() * (count as f64 + 0.5) * c.safety.height_m;
            count += 1;
            let next = boxes.iter().find(|(_, b)| {
                b.left == current.1.left
                    && b.right == current.1.right
                    && b.bottom < current.1.top
                    && current.1.top - b.bottom <= 2
            });
            match next {
                Some((&id, b)) => current = (id, b),
                None => break,
            }
        }
        let weight = count as f64 * c.safety.mass_kg * 9.81;
        slip = slip.max(shear / (c.safety.friction * weight));
        rocking = rocking.max(moment / (weight * c.safety.base_m / 2.0));
    }
    (slip, rocking)
}
/// Force par boîte estimée à partir de |u_x| maximal sur la couronne fluide
/// voisine, rapporté à la vitesse numérique d'entrée. Modèle exploratoire :
/// Cd et conversion du vent sont des hypothèses, pas une calibration CFD.
pub fn wind_estimate(
    c: &Config,
    g: &Grid,
    boxes: &BTreeMap<usize, Bounds>,
) -> Option<(f64, f64, f64)> {
    if !c.safety.wind_estimate_enabled || c.speed <= 0.0 || boxes.is_empty() {
        return None;
    }
    let mut forces = BTreeMap::new();
    for (&id, b) in boxes {
        let mut peak = 0.0_f64;
        let left = b.left.saturating_sub(1);
        let right = (b.right + 1).min(c.n + 1);
        let top = b.top.saturating_sub(1);
        let bottom = (b.bottom + 1).min(c.n + 1);
        let mut inspect = |x: usize, y: usize| {
            if y as f32 >= c.waterline {
                return;
            }
            let idx = g.to_index(x, y);
            if g.cells[idx].wall {
                return;
            }
            let speed = g.get_velocity_at_center(x, y).x as f64;
            if !speed.is_finite() {
                peak = f64::NAN;
                return;
            }
            if peak.is_nan() {
                return;
            }
            let physical_speed = speed.abs() / c.speed as f64 * c.safety.wind_kmh / 3.6;
            peak = peak.max(c.safety.wind_force(physical_speed));
        };
        for y in top..=bottom {
            inspect(left, y);
            inspect(right, y);
        }
        for x in left..=right {
            inspect(x, top);
            inspect(x, bottom);
        }
        if !peak.is_finite() {
            return Some((f64::NAN, f64::NAN, f64::NAN));
        }
        forces.insert(id, peak);
    }
    let (slip, rocking) = stack_ratios(c, boxes, &forces);
    Some((forces.values().copied().fold(0.0, f64::max), slip, rocking))
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
    fn wind_and_stack_thresholds() {
        let mut c = config();
        assert!((c.safety.wind_force(130.0 / 3.6) - 6054.386701).abs() < 0.01);
        assert!((c.safety.friction * c.safety.mass_kg * 9.81 - 44145.0).abs() < 0.01);
        for id in 0..10 {
            add(&mut c, id, 20, 20 + 10 * id);
        }
        let b = bounds(&c);
        let f: BTreeMap<_, _> = b
            .keys()
            .map(|&id| (id, c.safety.wind_force(130.0 / 3.6)))
            .collect();
        let (slip, rocking) = stack_ratios(&c, &b, &f);
        assert!(slip < 1.0 && rocking < 1.0);
        assert!((rocking - f[&0] * 10.0 / c.safety.centre_force_limit()).abs() < 1e-12);
        // Vue transversale : même masse, surface latérale plus grande et base plus courte.
        c.safety.base_m = 2.438;
        c.safety.span_m = 6.058;
        let f: BTreeMap<_, _> = b
            .keys()
            .map(|&id| (id, c.safety.wind_force(130.0 / 3.6)))
            .collect();
        let (slip, rocking) = stack_ratios(&c, &b, &f);
        assert!(slip < 1.0 && rocking > 1.0);
        c.safety.friction = 0.05;
        assert!(stack_ratios(&c, &b, &f).0 > 1.0);
        c.safety.max_tiers = 9;
        assert!(
            crate::experiments::Recorder::new(
                c.clone(),
                std::path::Path::new("unused-invalid-test")
            )
            .is_err()
        );
        c.safety.max_tiers = 12;
        assert!(c.validate().is_ok());
        let root = std::env::temp_dir().join(format!(
            "tipe-configurable-height-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut r = crate::experiments::Recorder::new(c, &root).unwrap();
        r.finish("interrupted").unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn excessive_wind_excludes_boat_from_ranking() {
        let mut c = config();
        add(&mut c, 0, 20, 20);
        c.speed = 1.0;
        c.safety.friction = 0.001;
        let (mut g, labels) = c.grid().unwrap();
        for cell in &mut g.cells {
            cell.velocity_x = 1.0;
        }
        let root = std::env::temp_dir().join(format!(
            "tipe-wind-reject-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut r = crate::experiments::Recorder::new(c, &root).unwrap();
        assert!(r.sample(&mut g, &labels, 0.0).is_err());
        let summary: serde_json::Value =
            serde_json::from_slice(&std::fs::read(r.folder.join("summary.json")).unwrap()).unwrap();
        assert_eq!(summary["status"], "invalid_cargo_wind");
        assert!(crate::experiments::entries(&root).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn force_limit_is_moment_balance() {
        let s = Safety::default();
        assert!((s.centre_force_limit() * s.height_m - s.mass_kg * 9.81 * s.base_m).abs() < 1e-8);
    }
}
