//! Géométrie et comparaison de dispositions d'un porte-conteneurs (vue de côté).
//! Compatible avec les appels existants de src/app.rs.
//!
//! La ligne d'eau est statique : elle sert de limite à l'écoulement simulé.
//! Le fluide sous la ligne d'eau est bloqué, et n'est pas simulé comme de l'eau.
//! Le dessin détaillé de la partie immergée relève du rendu graphique, pas du solveur.

use crate::conditions::*;
use crate::grid::{Grid, Vector2};
use rand::Rng;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WallKind {
    Hull,
    BowHull,
    SternHull,
    Container(usize),
    BowFairing,
    SternFairing,
    /// Domaine fictif sous la ligne de flottaison (ne pas compter comme coque).
    Water,
}

#[derive(Clone, Debug)]
pub struct Container {
    pub id: usize,
    pub center: (isize, isize),
    pub width: f32,
    pub height: f32,
}

/// Paramètres historiques, préservés pour compatibilité avec app.rs.
#[derive(Clone, Copy, Debug)]
pub struct HullCurveSpec {
    pub curve_bow: bool,
    pub curve_stern: bool,
    pub curvature: f32,
    pub bow_length: f32,
    pub stern_length: f32,
}

/// Réglages supplémentaires de la vue de côté.
/// Le programme principal peut les modifier ultérieurement dans egui.
#[derive(Clone, Copy, Debug)]
pub struct SideProfileSettings {
    /// Fraction de hauteur de coque émergée (0..1).
    pub freeboard_ratio: f32,
    /// Épaisseur de la ligne de séparation air/eau, en cellules.
    pub waterline_thickness: usize,
    /// Inclinaison de l'étrave : 0 verticale, 1 très inclinée.
    pub bow_rake: f32,
    /// Surélévation du gaillard avant, exprimée en fraction du franc-bord.
    pub forecastle_rise: f32,
    /// Fraction de la longueur de proue occupée par la transition du gaillard.
    pub forecastle_length_ratio: f32,
    /// Exposant de la courbure de l'étrave (>0).
    pub bow_exponent: f32,
    /// Inclinaison et arrondi de la poupe, gérés indépendamment.
    pub stern_rake: f32,
    pub stern_exponent: f32,
}

impl Default for SideProfileSettings {
    fn default() -> Self {
        Self {
            freeboard_ratio: 0.50,
            waterline_thickness: 3,
            bow_rake: 0.65,
            forecastle_rise: 0.18,
            forecastle_length_ratio: 0.65,
            bow_exponent: 1.35,
            stern_rake: 0.18,
            stern_exponent: 1.4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FairingSide { Bow, Stern }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FairingProfile {
    Triangular,
    Rounded,
    Parabolic,
    Stepped,
    TopCover,
    Sail,
    Arc,
}

impl FairingProfile {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Triangular => "Triangulaire",
            Self::Rounded => "Arrondi",
            Self::Parabolic => "Parabolique",
            Self::Stepped => "Étagé",
            Self::TopCover => "Capot supérieur",
            Self::Sail => "Voile / rampe",
            Self::Arc => "Arc courbe",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Fairing {
    /// Point de départ du carénage sur le pont (x_start, deck_y).
    pub position: (isize, isize),
    pub length: f32,
    /// Hauteur maximale au-dessus du pont.
    pub half_height: f32,
    pub side: FairingSide,
    pub profile: FairingProfile,
    pub height_scale: f32,
}

#[derive(Clone, Debug)]
pub struct ShipLayout {
    pub name: String,
    pub hull_rects: Vec<((isize, isize), f32, f32)>,
    pub hull_curve: Option<HullCurveSpec>,
    pub containers: Vec<Container>,
    pub fairings: Vec<Fairing>,
    pub flow_angle: f32,
}

#[derive(Clone, Debug)]
pub struct LayoutResult {
    pub layout_name: String,
    pub hull_force: Vector2,
    pub container_forces: HashMap<usize, Vector2>,
    pub total_drag: f32,
    pub total_torque: f32,
    pub score: f32,
}

/// Interpolation à dérivée nulle aux deux extrémités.
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Grid {
    fn fill_vertical(&mut self, x: isize, y0: f32, y1: f32) {
        if x < 0 || x >= N as isize { return; }
        let first = y0.min(y1).ceil().max(0.0) as isize;
        let last = y0.max(y1).floor().min(N - 1.0) as isize;
        for y in first..=last {
            self.wall_init(y as usize, x as usize, true);
        }
    }

    /// Carène basse continue : fond presque plat, sans dents ni bulbe en blocs.
    fn stamp_underwater_midship(&mut self, left: isize, right: isize, waterline_y: f32, draft: f32) {
        for x in left.min(right)..=left.max(right) {
            self.fill_vertical(x, waterline_y, waterline_y + 0.94 * draft);
        }
    }

    /// Extrémité immergée : raccord au fond puis remontée progressive vers
    /// la flottaison. Le modèle représente une silhouette, pas une carène 3D.
    fn stamp_underwater_end(&mut self, base_x: isize, tip_x: isize, waterline_y: f32, draft: f32, bow: bool) {
        let length = (tip_x - base_x).unsigned_abs().max(1) as isize;
        let direction = if tip_x >= base_x { 1 } else { -1 };
        for s in 0..=length {
            let x = base_x + direction * s;
            let t = s as f32 / length as f32;
            // Étrave avant plus relevée, poupe avec une remontée plus douce.
            let rise = if bow { smoothstep(t) } else { t * t * (2.0 - t) };
            let depth = draft * (0.94 - 0.86 * rise);
            self.fill_vertical(x, waterline_y, waterline_y + depth);
        }
    }

    /// Partie émergée de la proue ou de la poupe, en vue de côté.
    /// L'extrémité est construite entre x=base_x (coque rectangulaire) et x=tip_x.
    /// Le bas de la silhouette émergée est aligné avec la ligne de flottaison.
    pub fn stamp_side_hull_end(
        &mut self,
        base_x: isize,
        tip_x: isize,
        deck_y: f32,
        waterline_y: f32,
        bow: bool,
        settings: &SideProfileSettings,
    ) {
        let length = (tip_x - base_x).unsigned_abs().max(1) as isize;
        let direction = if tip_x >= base_x { 1 } else { -1 };
        let height = (waterline_y - deck_y).max(1.0);
        let rake = if bow { settings.bow_rake } else { settings.stern_rake }.clamp(0.0, 1.0);
        let exponent = if bow { settings.bow_exponent } else { settings.stern_exponent }.clamp(0.2, 6.0);

        for s in 0..=length {
            let x = base_x + direction * s;
            let t = s as f32 / length as f32;
            // La partie haute se relève légèrement vers la proue : gaillard d'avant.
            let rise = if bow {
                let start = 1.0 - settings.forecastle_length_ratio.clamp(0.01, 1.0);
                let progress = ((t - start) / (1.0 - start)).clamp(0.0, 1.0);
                height * settings.forecastle_rise.clamp(0.0, 0.8) * smoothstep(progress)
            } else { 0.0 };
            let top = deck_y - rise;
            // L'étrave rejoint la ligne de flottaison en progressant vers l'avant.
            // Quand rake=0, la silhouette devient courte et presque verticale.
            // Quand rake=1, l'obliquité s'étend sur toute la longueur indiquée.
            let bottom_factor = if rake < 0.001 {
                if t < 0.94 { 1.0 } else { ((1.0 - t) / 0.06).clamp(0.0, 1.0) }
            } else {
                (1.0 - t.powf(exponent / rake.max(0.1))).clamp(0.0, 1.0)
            };
            let bottom = top + (waterline_y - top) * bottom_factor;
            self.fill_vertical(x, top, bottom);
        }
    }

    /// API historique conservée, utile pour les autres types de silhouette.
    pub fn stamp_curved_hull_end(&mut self, base_x: isize, tip_x: isize, center_y: isize, half_height: f32, curvature: f32) {
        let curvature = curvature.clamp(0.0, 1.0);
        let dir = if tip_x >= base_x { 1 } else { -1 };
        let length = (tip_x - base_x).unsigned_abs().max(1) as isize;
        for s in 0..=length {
            let x = base_x + dir * s;
            let t = 1.0 - s as f32 / length as f32;
            let shape = t * (1.0 - curvature)
                + (1.0 - (1.0 - t).powi(2)).max(0.0).sqrt() * curvature;
            let h = half_height * shape;
            self.fill_vertical(x, center_y as f32 - h, center_y as f32 + h);
        }
    }

    pub fn stamp_fairing(&mut self, fairing: &Fairing) {
        let length = fairing.length.max(1.0) as isize;
        let x0 = fairing.position.0;
        let deck_y = fairing.position.1 as f32;
        let full_h = fairing.half_height.max(1.0) * fairing.height_scale.clamp(0.15, 2.5);

        for s in 0..=length {
            let x = x0 + s;
            let u = s as f32 / length as f32;
            let q = match fairing.side {
                FairingSide::Bow => u,
                FairingSide::Stern => 1.0 - u,
            };
            let h = match fairing.profile {
                FairingProfile::Triangular => full_h * q,
                FairingProfile::Rounded => full_h * (1.0 - (1.0 - q).powi(2)).sqrt().clamp(0.0, 1.0),
                FairingProfile::Parabolic => full_h * (1.0 - (1.0 - q).powi(2)),
                FairingProfile::Stepped => {
                    let levels = 5.0;
                    let stepped = (q * levels).ceil() / levels;
                    full_h * stepped
                }
                FairingProfile::TopCover => {
                    if q < 0.25 { full_h * (q / 0.25) }
                    else { full_h * (0.95 - 0.15 * ((q - 0.25) / 0.75)) }
                }
                FairingProfile::Sail => full_h * q.powf(0.55),
                FairingProfile::Arc => full_h * (1.0 - (q - 1.0).abs().powf(1.8)),
            }.max(0.0);
            self.fill_vertical(x, deck_y - h, deck_y);
        }
    }

    /// API existante, avec paramètres de profil par défaut.
    pub fn apply_layout(&mut self, layout: &ShipLayout) -> HashMap<usize, WallKind> {
        self.apply_layout_with_profile(layout, &SideProfileSettings::default())
    }

    /// Variante paramétrable : permettra de brancher les nouveaux sliders egui.
    pub fn apply_layout_with_profile(&mut self, layout: &ShipLayout, settings: &SideProfileSettings) -> HashMap<usize, WallKind> {
        let mut labels = HashMap::new();
        let Some(&(main_center, main_width, main_height)) = layout.hull_rects.first() else {
            return labels;
        };
        let deck_y = main_center.1 as f32 - main_height / 2.0;
        let waterline_y = deck_y + main_height * settings.freeboard_ratio.clamp(0.05, 0.95);

        // Ligne de séparation air/eau : seulement une bande fine, pour garder
        // la coque immergée visible à l'écran.
        let band_thickness = settings.waterline_thickness.max(1) as isize;
        let band_center = waterline_y.round() as isize;
        let band_start = (band_center - band_thickness / 2).max(0);
        let band_end = (band_start + band_thickness - 1).min(N as isize - 1);
        for y in band_start..=band_end {
            for x in 0..(N as isize) {
                self.wall_init(y as usize, x as usize, true);
            }
        }
        for y in band_start..=band_end {
            for x in 0..(N as usize) {
                let idx = self.to_index(x, y as usize);
                if idx < self.cells.len() && self.cells[idx].wall {
                    labels.insert(idx, WallKind::Water);
                }
            }
        }

        // Corps principal : on sépare désormais la partie émergée et la carène
        // sous-marine pour obtenir une silhouette plus réaliste en vue de côté.
        let draft = (main_height * (1.0 - settings.freeboard_ratio.clamp(0.05, 0.95))).max(2.0);
        let main_left = main_center.0 - (main_width / 2.0) as isize;
        let main_right = main_center.0 + (main_width / 2.0) as isize;

        let before_mid: HashSet<usize> = self.cells.iter().enumerate().filter(|(_, c)| c.wall).map(|(i, _)| i).collect();
        for &(center, width, height) in &layout.hull_rects {
            let top = center.1 as f32 - height / 2.0;
            let left = center.0 - (width / 2.0) as isize;
            let right = center.0 + (width / 2.0) as isize;
            for x in left..=right { self.fill_vertical(x, top, waterline_y); }
        }
        self.stamp_underwater_midship(main_left, main_right, waterline_y, draft);
        for (idx, cell) in self.cells.iter().enumerate() {
            if cell.wall && !before_mid.contains(&idx) && !labels.contains_key(&idx) {
                labels.insert(idx, WallKind::Hull);
            }
        }

        if let Some(curve) = layout.hull_curve {
            let half_w = (main_width / 2.0) as isize;
            if curve.curve_bow && curve.bow_length > 0.0 {
                let before: HashSet<usize> = self.cells.iter().enumerate().filter(|(_, c)| c.wall).map(|(i, _)| i).collect();
                let base = main_center.0 - half_w;
                let tip = base - curve.bow_length as isize;
                self.stamp_side_hull_end(base, tip, deck_y, waterline_y, true, settings);
                self.stamp_underwater_end(base, tip, waterline_y, draft, true);
                for (idx, cell) in self.cells.iter().enumerate() {
                    if cell.wall && !before.contains(&idx) && !labels.contains_key(&idx) {
                        labels.insert(idx, WallKind::BowHull);
                    }
                }
            }
            if curve.curve_stern && curve.stern_length > 0.0 {
                let before: HashSet<usize> = self.cells.iter().enumerate().filter(|(_, c)| c.wall).map(|(i, _)| i).collect();
                let base = main_center.0 + half_w;
                let tip = base + curve.stern_length as isize;
                self.stamp_side_hull_end(base, tip, deck_y, waterline_y, false, settings);
                self.stamp_underwater_end(base, tip, waterline_y, draft * 0.92, false);
                for (idx, cell) in self.cells.iter().enumerate() {
                    if cell.wall && !before.contains(&idx) && !labels.contains_key(&idx) {
                        labels.insert(idx, WallKind::SternHull);
                    }
                }
            }
        }

        for c in &layout.containers {
            let before: HashSet<usize> = self.cells.iter().enumerate().filter(|(_, c)| c.wall).map(|(i, _)| i).collect();
            self.rectangle(c.center.0, c.center.1, c.width, c.height);
            for (idx, cell) in self.cells.iter().enumerate() {
                if cell.wall && !before.contains(&idx) { labels.insert(idx, WallKind::Container(c.id)); }
            }
        }
        for fairing in &layout.fairings {
            let before: HashSet<usize> = self.cells.iter().enumerate().filter(|(_, c)| c.wall).map(|(i, _)| i).collect();
            self.stamp_fairing(fairing);
            for (idx, cell) in self.cells.iter().enumerate() {
                if cell.wall && !before.contains(&idx) {
                    labels.insert(idx, match fairing.side {
                        FairingSide::Bow => WallKind::BowFairing,
                        FairingSide::Stern => WallKind::SternFairing,
                    });
                }
            }
        }
        labels
    }
}

pub fn test_layout(layout: &ShipLayout, steps: usize, hole_pos: &[usize]) -> LayoutResult {
    let mut grid = Grid::new();
    let labels = grid.apply_layout(layout);
    grid.setup_wind_tunnel_walls(hole_pos);
    let max_hole_row = hole_pos.iter().copied().max().unwrap_or(N as usize);
    for _ in 0..steps {
        let left_wall = 1;
        let box_width = (N as usize / 30).max(2);
        let right_wall = left_wall + box_width;
        for i in (left_wall + 2)..=right_wall.saturating_sub(5) {
            for j in 2..max_hole_row {
                let idx = grid.to_index(i, j);
                if grid.cells[idx].wall { continue; }
                let wf = wind_profile_factor(j);
                grid.cell_init(i, j, FLOW_VELOCITY * wf, 0.0, FLOW_DENSITY);
            }
        }
        grid.vel2_step(FLOW_VELOCITY);
    }
    let cell_forces = grid.compute_wall_forces();
    let mut hull_force = Vector2::default();
    let mut container_forces: HashMap<usize, Vector2> = HashMap::new();
    for (&idx, &kind) in &labels {
        let f = cell_forces[idx];
        match kind {
            WallKind::Hull | WallKind::BowHull | WallKind::SternHull | WallKind::BowFairing | WallKind::SternFairing => {
                hull_force.x += f.x;
                hull_force.y += f.y;
            }
            WallKind::Container(id) => {
                let entry = container_forces.entry(id).or_insert(Vector2::default());
                entry.x += f.x;
                entry.y += f.y;
            }
            WallKind::Water => {}
        }
    }
    let mut total = hull_force;
    for f in container_forces.values() { total.x += f.x; total.y += f.y; }
    let total_drag = total.x * layout.flow_angle.cos() + total.y * layout.flow_angle.sin();
    let total_torque: f32 = grid.compute_object_forces().iter().map(|o| o.torque.abs()).sum();
    let score = W_DRAG * total_drag.abs() + W_TORQUE * total_torque;
    LayoutResult { layout_name: layout.name.clone(), hull_force, container_forces, total_drag, total_torque, score }
}

pub fn compare_layouts(layouts: &[ShipLayout], steps: usize, hole_pos: &[usize]) {
    let mut results: Vec<LayoutResult> = layouts.iter().map(|l| test_layout(l, steps, hole_pos)).collect();
    results.sort_by(|a, b| a.score.total_cmp(&b.score));
    println!("\n=== Classement des dispositions de conteneurs ===");
    for r in &results {
        println!("{:<20} | traînée: {:8.4} | couple total: {:8.4} | score: {:8.4}",
            r.layout_name, r.total_drag, r.total_torque, r.score);
    }
    println!("===================================================\n");
}

#[derive(Clone, Copy, Debug)]
pub enum ContainerLayoutKind {
    Grid { rows: usize, cols: usize },
    Pyramid { rows: usize },
    Staggered { rows: usize, cols: usize },
}

#[derive(Clone, Copy, Debug)]
pub struct BoatParams {
    pub center: (isize, isize),
    pub hull_width: f32,
    pub hull_height: f32,
    pub container_width: f32,
    pub container_height: f32,
    pub container_gap: f32,
    pub kind: ContainerLayoutKind,
    pub flow_angle: f32,
    pub curve_bow: bool,
    pub curve_stern: bool,
    pub hull_curvature: f32,
    pub bow_length: f32,
    pub stern_length: f32,
    pub add_bow_fairing: bool,
    pub add_stern_fairing: bool,
    pub bow_fairing_length: f32,
    pub stern_fairing_length: f32,
    pub bow_fairing_profile: FairingProfile,
    pub stern_fairing_profile: FairingProfile,
    pub bow_fairing_height_scale: f32,
    pub stern_fairing_height_scale: f32,
    /// 0 = au plus près de la proue, 1 = collé aux conteneurs.
    pub bow_fairing_position: f32,
    /// 0 = collé aux conteneurs, 1 = au plus près de la poupe.
    pub stern_fairing_position: f32,
}

pub fn build_boat_layout(name: &str, p: &BoatParams) -> ShipLayout {
    let hull_rects = vec![(p.center, p.hull_width, p.hull_height)];
    let hull_curve = if p.curve_bow || p.curve_stern {
        Some(HullCurveSpec { curve_bow: p.curve_bow, curve_stern: p.curve_stern,
            curvature: p.hull_curvature, bow_length: p.bow_length, stern_length: p.stern_length })
    } else { None };
    let mut containers = Vec::new();
    let mut id = 0usize;
    let top = p.center.1 - (p.hull_height / 2.0) as isize;
    let step_x = p.container_width + p.container_gap;
    let step_y = p.container_height + p.container_gap;
    let push_row = |row: usize, ncols: usize, offset: f32, containers: &mut Vec<Container>, id: &mut usize| {
        let total = ncols as f32 * step_x - p.container_gap;
        let start = p.center.0 - (total / 2.0) as isize + offset as isize;
        let cy = top - (p.container_height / 2.0) as isize
            - (row as f32 * (p.container_height + p.container_gap)) as isize;
        for c in 0..ncols {
            containers.push(Container { id: *id,
                center: (start + (c as f32 * step_x + p.container_width / 2.0) as isize, cy),
                width: p.container_width, height: p.container_height });
            *id += 1;
        }
    };
    match p.kind {
        ContainerLayoutKind::Grid { rows, cols } => {
            for r in 0..rows { push_row(r, cols, 0.0, &mut containers, &mut id); }
        }
        ContainerLayoutKind::Pyramid { rows } => {
            for r in 0..rows { push_row(r, rows.saturating_sub(r).max(1), 0.0, &mut containers, &mut id); }
        }
        ContainerLayoutKind::Staggered { rows, cols } => {
            for r in 0..rows {
                push_row(r, cols, if r % 2 == 1 { step_x / 2.0 } else { 0.0 }, &mut containers, &mut id);
            }
        }
    }
    let mut fairings = Vec::new();
    if !containers.is_empty() && (p.add_bow_fairing || p.add_stern_fairing) {
        let stack_left = containers.iter().map(|c| c.center.0 - (c.width / 2.0) as isize).min().unwrap();
        let stack_right = containers.iter().map(|c| c.center.0 + (c.width / 2.0) as isize).max().unwrap();
        let stack_top = containers.iter().map(|c| c.center.1 - (c.height / 2.0) as isize).min().unwrap();
        let deck_y = top;
        let fairing_h = ((deck_y - stack_top) as f32).max(4.0);
        let main_left = p.center.0 - (p.hull_width / 2.0) as isize;
        let main_right = p.center.0 + (p.hull_width / 2.0) as isize;
        let bow_tip = if p.curve_bow { main_left - p.bow_length as isize } else { main_left };
        let stern_tip = if p.curve_stern { main_right + p.stern_length as isize } else { main_right };

        if p.add_bow_fairing {
            let max_len = ((stack_left - bow_tip - 2).max(2)) as f32;
            let len = p.bow_fairing_length.clamp(2.0, max_len);
            let start_min = bow_tip + 1;
            let start_max = stack_left - len as isize;
            let start = if start_max <= start_min { start_min } else {
                let t = p.bow_fairing_position.clamp(0.0, 1.0);
                (start_min as f32 + (start_max - start_min) as f32 * t).round() as isize
            };
            fairings.push(Fairing {
                position: (start, deck_y), length: len,
                half_height: fairing_h, side: FairingSide::Bow,
                profile: p.bow_fairing_profile,
                height_scale: p.bow_fairing_height_scale,
            });
        }
        if p.add_stern_fairing {
            let max_len = ((stern_tip - stack_right - 2).max(2)) as f32;
            let len = p.stern_fairing_length.clamp(2.0, max_len);
            let start_min = stack_right + 1;
            let start_max = stern_tip - len as isize;
            let start = if start_max <= start_min { start_min } else {
                let t = p.stern_fairing_position.clamp(0.0, 1.0);
                (start_min as f32 + (start_max - start_min) as f32 * t).round() as isize
            };
            fairings.push(Fairing {
                position: (start, deck_y), length: len,
                half_height: fairing_h, side: FairingSide::Stern,
                profile: p.stern_fairing_profile,
                height_scale: p.stern_fairing_height_scale,
            });
        }
    }
    ShipLayout { name: name.to_string(), hull_rects, hull_curve, containers, fairings, flow_angle: p.flow_angle }
}

pub fn quick_variants(base: BoatParams) -> Vec<ShipLayout> {
    vec![
        build_boat_layout("grille_3x3", &BoatParams { kind: ContainerLayoutKind::Grid { rows: 3, cols: 3 }, ..base }),
        build_boat_layout("pyramide_4", &BoatParams { kind: ContainerLayoutKind::Pyramid { rows: 4 }, ..base }),
        build_boat_layout("quinconce_3x3", &BoatParams { kind: ContainerLayoutKind::Staggered { rows: 3, cols: 3 }, ..base }),
    ]
}

pub fn random_layout(name: &str, base: &BoatParams, rng: &mut impl Rng) -> ShipLayout {
    let rows = rng.random_range(1..=6usize);
    let cols = rng.random_range(1..=6usize);
    let kind = match rng.random_range(0..3) {
        0 => ContainerLayoutKind::Grid { rows, cols },
        1 => ContainerLayoutKind::Pyramid { rows },
        _ => ContainerLayoutKind::Staggered { rows, cols },
    };
    let profiles = [
        FairingProfile::Triangular,
        FairingProfile::Rounded,
        FairingProfile::Parabolic,
        FairingProfile::Stepped,
        FairingProfile::TopCover,
        FairingProfile::Sail,
        FairingProfile::Arc,
    ];
    let params = BoatParams {
        kind,
        container_gap: rng.random_range(0.0..=(base.container_gap.max(1.0) * 3.0)),
        curve_bow: rng.random_bool(0.6),
        curve_stern: rng.random_bool(0.6),
        hull_curvature: rng.random_range(0.0..=1.0f32),
        add_bow_fairing: rng.random_bool(0.5),
        add_stern_fairing: rng.random_bool(0.5),
        bow_fairing_length: rng.random_range(2.0..=(base.hull_width.max(4.0) * 0.25)),
        stern_fairing_length: rng.random_range(2.0..=(base.hull_width.max(4.0) * 0.25)),
        bow_fairing_profile: profiles[rng.random_range(0..profiles.len())],
        stern_fairing_profile: profiles[rng.random_range(0..profiles.len())],
        bow_fairing_height_scale: rng.random_range(0.5..=1.6),
        stern_fairing_height_scale: rng.random_range(0.5..=1.6),
        bow_fairing_position: rng.random_range(0.0..=1.0),
        stern_fairing_position: rng.random_range(0.0..=1.0),
        ..*base
    };
    build_boat_layout(name, &params)
}

pub fn random_variants(base: BoatParams, n: usize) -> Vec<ShipLayout> {
    let mut rng = rand::rng();
    (0..n).map(|i| random_layout(&format!("alea_{i}"), &base, &mut rng)).collect()
}
