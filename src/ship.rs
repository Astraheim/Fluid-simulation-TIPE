use crate::conditions::*;
use crate::grid::{Grid, Vector2};
use std::collections::HashMap;
use rand::Rng;

/*
Module de test pour l'organisation des conteneurs sur le bateau.

Idée générale :
    - `wall : bool` (dans Cell) reste la seule chose que le solveur fluide connaît.
    - On garde EN PLUS une table cell_idx → WallKind pour savoir, après coup,
      quelle cellule murale appartenait à la coque ou à quel conteneur.
    - compute_wall_forces() (déjà existant, pressure_computation.rs) donne la force
      PAR CELLULE, avant l'agrégation par objet connexe. On s'en sert directement
      ici, donc même si un conteneur touche la coque et qu'ils fusionnent dans
      identify_objects(), on peut quand même isoler leur contribution respective.

Nouveauté (session du jour) :
    - `build_boat_layout` génère une ShipLayout (coque + conteneurs) à partir
      de quelques paramètres géométriques et d'une organisation choisie
      (Grille / Pyramide / Quinconce), pour pouvoir tester rapidement
      différentes dispositions sans les écrire à la main.
    - `quick_variants` renvoie directement 3 variantes prêtes à comparer via
      `compare_layouts`.

Nouveauté (session courante) :
    - La coque peut désormais avoir une proue et/ou une poupe courbée
      (`HullCurveSpec`), au lieu d'un simple rectangle plein — se rapprochant
      d'une vraie silhouette de coque de navire. `curvature` interpole entre
      un profil pointu (triangulaire) et un profil arrondi (quart d'ellipse).
    - Des "fairings" (carénages/protections aérodynamiques, cf. la littérature
      sur les gap-flow-protectors et forecastle fairings) peuvent être ajoutés
      devant et/ou derrière la pile de conteneurs pour réduire la traînée.
    - `random_layout` / `random_variants` génèrent des configurations de
      conteneurs (et de carénages) aléatoires, pour explorer l'espace des
      configurations plutôt que de comparer seulement 3 dispositions fixes.
*/

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WallKind {
    Hull,
    Container(usize), // id du conteneur
    Fairing,
}

#[derive(Clone, Debug)]
pub struct Container {
    pub id: usize,
    pub center: (isize, isize), // en cellules de grille
    pub width: f32,
    pub height: f32,
}

/// Spécification de la courbure de la coque (proue / poupe).
/// `curvature` va de 0.0 (extrémité pointue / triangulaire, comme un étrave
/// "clipper") à 1.0 (extrémité arrondie, quart d'ellipse, plus proche d'un
/// bulbe d'étrave / d'une poupe à tableau arrondi).
#[derive(Clone, Copy, Debug)]
pub struct HullCurveSpec {
    pub curve_bow: bool,
    pub curve_stern: bool,
    pub curvature: f32,
    pub bow_length: f32,
    pub stern_length: f32,
}

/// Côté sur lequel un carénage (fairing) est placé par rapport à la pile de
/// conteneurs : Bow = à l'avant (face au vent relatif), Stern = à l'arrière.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FairingSide {
    Bow,
    Stern,
}

/// Un carénage aérodynamique simple (profil en coin/triangle) placé devant
/// ou derrière la pile de conteneurs, pour limiter le décollement de la
/// couche limite (cf. "gap-flow protectors" / "forecastle fairings").
#[derive(Clone, Copy, Debug)]
pub struct Fairing {
    pub position: (isize, isize), // point d'attache, côté pile de conteneurs
    pub length: f32,               // longueur (protrusion) du carénage
    pub half_height: f32,          // demi-hauteur à la base (= hauteur de la pile / 2)
    pub side: FairingSide,
}

#[derive(Clone, Debug)]
pub struct ShipLayout {
    pub name: String,
    /// Rectangles définissant la silhouette de la coque pour cette vue
    /// (dessus / côté / face). Le corps principal (rectangulaire) de la
    /// coque ; si `hull_curve` est renseigné, ce rectangle est déjà
    /// raccourci pour laisser la place aux extrémités courbées.
    pub hull_rects: Vec<((isize, isize), f32, f32)>,
    /// Courbure optionnelle de la proue/poupe, appliquée en plus de
    /// `hull_rects[0]`.
    pub hull_curve: Option<HullCurveSpec>,
    pub containers: Vec<Container>,
    /// Carénages aéro optionnels devant/derrière la pile de conteneurs.
    pub fairings: Vec<Fairing>,
    /// Direction du flux pour CETTE vue, en radians (0.0 = vers +x, la direction
    /// "traînée" par rapport à laquelle on projette la force).
    pub flow_angle: f32,
}

/// Résultat d'un test : forces séparées coque / conteneurs + score global
#[derive(Clone, Debug)]
pub struct LayoutResult {
    pub layout_name: String,
    pub hull_force: Vector2,
    pub container_forces: HashMap<usize, Vector2>,
    pub total_drag: f32,   // projection de la force totale sur flow_angle
    pub total_torque: f32, // somme des couples individuels (approx, cf. note plus bas)
    pub score: f32,
}

impl Grid {
    /// Stamp un ShipLayout dans la grille (murs) et renvoie la table d'étiquetage.
    /// À appeler juste après Grid::new().
    pub fn apply_layout(&mut self, layout: &ShipLayout) -> HashMap<usize, WallKind> {
        let mut labels = HashMap::new();

        // 1. Coque : corps principal (rectangle(s))
        for &(center, w, h) in &layout.hull_rects {
            self.rectangle(center.0, center.1, w, h);
        }

        // 1b. Extrémités de coque courbées (proue / poupe), si demandé.
        // On se base sur le premier rectangle de hull_rects comme corps
        // principal (c'est la convention utilisée par `build_boat_layout`).
        if let Some(curve) = layout.hull_curve {
            if let Some(&(center, w, h)) = layout.hull_rects.first() {
                let half_h = h / 2.0;
                let half_w = (w / 2.0) as isize;

                if curve.curve_bow && curve.bow_length > 0.0 {
                    let base_x = center.0 - half_w;
                    let tip_x = base_x - curve.bow_length as isize;
                    self.stamp_curved_hull_end(base_x, tip_x, center.1, half_h, curve.curvature);
                }
                if curve.curve_stern && curve.stern_length > 0.0 {
                    let base_x = center.0 + half_w;
                    let tip_x = base_x + curve.stern_length as isize;
                    self.stamp_curved_hull_end(base_x, tip_x, center.1, half_h, curve.curvature);
                }
            }
        }

        // On relit la grille pour capturer TOUTES les cellules murales posées par
        // la coque avant d'ajouter les conteneurs (sinon on ne peut plus les distinguer).
        for (i, j, idx) in self.iter_morton() {
            if self.cells[idx].wall {
                labels.insert(idx, WallKind::Hull);
            }
            let _ = (i, j); // évite un warning si non utilisé ailleurs
        }

        // 2. Conteneurs : on note les indices murés AVANT/APRÈS pour ne
        //    labelliser que les nouvelles cellules de CE conteneur.
        for container in &layout.containers {
            let before: std::collections::HashSet<usize> = self.cells.iter().enumerate()
                .filter(|(_, c)| c.wall)
                .map(|(idx, _)| idx)
                .collect();

            self.rectangle(container.center.0, container.center.1, container.width, container.height);

            for (idx, cell) in self.cells.iter().enumerate() {
                if cell.wall && !before.contains(&idx) {
                    labels.insert(idx, WallKind::Container(container.id));
                }
            }
        }

        // 3. Carénages aéro devant/derrière la pile de conteneurs.
        for fairing in &layout.fairings {
            let before: std::collections::HashSet<usize> = self.cells.iter().enumerate()
                .filter(|(_, c)| c.wall)
                .map(|(idx, _)| idx)
                .collect();

            self.stamp_fairing(fairing);

            for (idx, cell) in self.cells.iter().enumerate() {
                if cell.wall && !before.contains(&idx) {
                    labels.insert(idx, WallKind::Fairing);
                }
            }
        }

        labels
    }

    /// Stamp une extrémité de coque courbée (proue ou poupe) entre `base_x`
    /// (où la coque a sa pleine demi-hauteur `half_height`) et `tip_x` (la
    /// pointe, demi-hauteur nulle). `curvature` interpole entre un profil
    /// triangulaire (0.0) et un profil en quart d'ellipse (1.0, plus arrondi
    /// / réaliste pour une étrave/poupe de navire).
    pub fn stamp_curved_hull_end(&mut self, base_x: isize, tip_x: isize, center_y: isize, half_height: f32, curvature: f32) {
        let curvature = curvature.clamp(0.0, 1.0);
        let dir: isize = if tip_x >= base_x { 1 } else { -1 };
        let length = (tip_x - base_x).unsigned_abs().max(1) as isize;

        for s in 0..=length {
            let x = base_x + dir * s;
            // t : 1.0 à la base (pleine largeur), 0.0 à la pointe
            let t = 1.0 - (s as f32 / length as f32);
            let linear = t;
            let elliptical = (1.0 - (1.0 - t).powi(2)).max(0.0).sqrt();
            let shape = linear * (1.0 - curvature) + elliptical * curvature;
            let h = (half_height * shape).round() as isize;

            for dy in -h..=h {
                let y = center_y + dy;
                if x >= 0 && y >= 0 {
                    self.wall_init(y as usize, x as usize, true);
                }
            }
        }
    }

    /// Stamp un carénage aéro (profil en coin, qui s'amincit en s'éloignant
    /// de la pile de conteneurs) devant (Bow) ou derrière (Stern) celle-ci.
    pub fn stamp_fairing(&mut self, fairing: &Fairing) {
        let dir: isize = match fairing.side {
            FairingSide::Bow => -1,
            FairingSide::Stern => 1,
        };
        let length = (fairing.length.max(1.0)) as isize;
        let (base_x, base_y) = fairing.position;

        for s in 0..=length {
            let t = 1.0 - (s as f32 / length as f32); // 1.0 à la base, 0.0 à la pointe
            let h = (fairing.half_height * t).round() as isize;
            let x = base_x + dir * s;

            for dy in -h..=h {
                let y = base_y + dy;
                if x >= 0 && y >= 0 {
                    self.wall_init(y as usize, x as usize, true);
                }
            }
        }
    }
}

/// Fait tourner `steps` pas de simulation SANS fenêtre (headless), puis
/// calcule les forces séparées coque/conteneurs et le score pondéré.
///
/// NB IMPORTANT sur le couple : compute_object_forces() calcule un couple par
/// OBJET CONNEXE (via identify_objects/DFS), pas par étiquette. Si un conteneur
/// touche la coque, ils forment un seul objet et le couple sera celui du bloc
/// entier, pas celui du conteneur isolé. Pour un couple isolé par conteneur, il
/// faudrait réimplémenter le calcul de couple ici à partir de compute_wall_forces()
/// + labels (même logique que compute_object_forces mais filtrée par WallKind).
/// Je laisse cette version simple (couple global de l'ensemble coque+conteneurs)
/// pour commencer — on l'affine si le couple par conteneur s'avère nécessaire.
pub fn test_layout(layout: &ShipLayout, steps: usize, hole_pos: &[usize]) -> LayoutResult {
    let mut grid = Grid::new();
    let labels = grid.apply_layout(layout);

    for _ in 0..steps {
        grid.initialize_wind_tunnel(FLOW_DENSITY, FLOW_VELOCITY, hole_pos);
        grid.vel2_step(FLOW_VELOCITY);
    }

    let cell_forces = grid.compute_wall_forces();

    let mut hull_force = Vector2::default();
    let mut container_forces: HashMap<usize, Vector2> = HashMap::new();

    for (idx, kind) in &labels {
        let f = cell_forces[*idx];
        match kind {
            WallKind::Hull | WallKind::Fairing => {
                hull_force.x += f.x;
                hull_force.y += f.y;
            }
            WallKind::Container(id) => {
                let entry = container_forces.entry(*id).or_insert(Vector2::default());
                entry.x += f.x;
                entry.y += f.y;
            }
        }
    }

    let mut total = hull_force;
    for f in container_forces.values() {
        total.x += f.x;
        total.y += f.y;
    }

    let (cos_a, sin_a) = (layout.flow_angle.cos(), layout.flow_angle.sin());
    let total_drag = total.x * cos_a + total.y * sin_a;

    // Couple global (objets connexes) — cf. note ci-dessus
    let objects = grid.compute_object_forces();
    let total_torque: f32 = objects.iter().map(|o| o.torque.abs()).sum();

    let score = W_DRAG * total_drag.abs() + W_TORQUE * total_torque;

    LayoutResult {
        layout_name: layout.name.clone(),
        hull_force,
        container_forces,
        total_drag,
        total_torque,
        score,
    }
}

/// Lance plusieurs dispositions et affiche un classement par score
/// (plus petit score = meilleur compromis traînée/couple).
pub fn compare_layouts(layouts: &[ShipLayout], steps: usize, hole_pos: &[usize]) {
    let mut results: Vec<LayoutResult> = layouts.iter()
        .map(|l| test_layout(l, steps, hole_pos))
        .collect();

    results.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap());

    println!("\n=== Classement des dispositions de conteneurs ===");
    for r in &results {
        println!(
            "{:<20} | traînée: {:8.4} | couple total: {:8.4} | score: {:8.4}",
            r.layout_name, r.total_drag, r.total_torque, r.score
        );
    }
    println!("===================================================\n");
}

// ============================================================================
// GÉNÉRATEUR DE BATEAU PARAMÉTRABLE
// ============================================================================

/// Organisation des conteneurs sur le pont.
#[derive(Clone, Copy, Debug)]
pub enum ContainerLayoutKind {
    /// Rangées x colonnes régulières.
    Grid { rows: usize, cols: usize },
    /// Une rangée de `rows` niveaux, chaque niveau plus étroit que le précédent
    /// (empilement en pyramide, base large en bas).
    Pyramid { rows: usize },
    /// Rangées x colonnes, avec un décalage d'une demi-largeur une rangée sur deux.
    Staggered { rows: usize, cols: usize },
}

/// Paramètres géométriques pour générer rapidement un bateau + ses conteneurs.
#[derive(Clone, Copy, Debug)]
pub struct BoatParams {
    /// Centre de la coque, en cellules de grille.
    pub center: (isize, isize),
    pub hull_width: f32,
    pub hull_height: f32,
    pub container_width: f32,
    pub container_height: f32,
    /// Espace (en cellules) entre deux conteneurs adjacents et entre la coque
    /// et le premier niveau de conteneurs.
    pub container_gap: f32,
    pub kind: ContainerLayoutKind,
    pub flow_angle: f32,

    // --- Courbure de coque ---
    pub curve_bow: bool,
    pub curve_stern: bool,
    /// 0.0 = pointu (triangulaire/clipper), 1.0 = arrondi (quart d'ellipse).
    pub hull_curvature: f32,
    /// Longueur de la proue/poupe courbée, en cellules.
    pub bow_length: f32,
    pub stern_length: f32,

    // --- Carénages aéro (gap-flow protectors) ---
    pub add_bow_fairing: bool,
    pub add_stern_fairing: bool,
    pub fairing_length: f32,
}

/// Construit une ShipLayout (coque rectangulaire + conteneurs empilés dessus)
/// à partir de `BoatParams'. Les conteneurs sont empilés "vers le haut" (y
/// décroissant) à partir du sommet de la coque.
pub fn build_boat_layout(name: &str, p: &BoatParams) -> ShipLayout {
    let hull_rects = vec![(p.center, p.hull_width, p.hull_height)];

    let hull_curve = if p.curve_bow || p.curve_stern {
        Some(HullCurveSpec {
            curve_bow: p.curve_bow,
            curve_stern: p.curve_stern,
            curvature: p.hull_curvature,
            bow_length: p.bow_length,
            stern_length: p.stern_length,
        })
    } else {
        None
    };

    let mut containers = Vec::new();
    let mut id = 0usize;

    let top_of_hull_y = p.center.1 - (p.hull_height / 2.0) as isize;
    let step_x = p.container_width + p.container_gap;
    let step_y = p.container_height + p.container_gap;

    let mut push_row = |row: usize, cols_in_row: usize, x_offset: f32, containers: &mut Vec<Container>, id: &mut usize| {
        let total_w = cols_in_row as f32 * step_x - p.container_gap;
        let start_x = p.center.0 - (total_w / 2.0) as isize + x_offset as isize;
        let cy = top_of_hull_y - (row as f32 * step_y) as isize - (p.container_height / 2.0) as isize - p.container_gap as isize;

        for c in 0..cols_in_row {
            let cx = start_x + (c as f32 * step_x + p.container_width / 2.0) as isize;
            containers.push(Container {
                id: *id,
                center: (cx, cy),
                width: p.container_width,
                height: p.container_height,
            });
            *id += 1;
        }
    };

    match p.kind {
        ContainerLayoutKind::Grid { rows, cols } => {
            for r in 0..rows {
                push_row(r, cols, 0.0, &mut containers, &mut id);
            }
        }
        ContainerLayoutKind::Pyramid { rows } => {
            for r in 0..rows {
                let cols_in_row = rows.saturating_sub(r).max(1);
                push_row(r, cols_in_row, 0.0, &mut containers, &mut id);
            }
        }
        ContainerLayoutKind::Staggered { rows, cols } => {
            for r in 0..rows {
                let offset = if r % 2 == 1 { step_x / 2.0 } else { 0.0 };
                push_row(r, cols, offset, &mut containers, &mut id);
            }
        }
    }

    // Carénages aéro devant/derrière la pile de conteneurs, dimensionnés sur
    // l'emprise réelle (bounding box) des conteneurs générés.
    let mut fairings = Vec::new();
    if !containers.is_empty() && (p.add_bow_fairing || p.add_stern_fairing) {
        let min_x = containers.iter().map(|c| c.center.0 - (c.width / 2.0) as isize).min().unwrap();
        let max_x = containers.iter().map(|c| c.center.0 + (c.width / 2.0) as isize).max().unwrap();
        let min_y = containers.iter().map(|c| c.center.1 - (c.height / 2.0) as isize).min().unwrap();
        let max_y = containers.iter().map(|c| c.center.1 + (c.height / 2.0) as isize).max().unwrap();
        let stack_half_h = ((max_y - min_y) as f32 / 2.0).max(1.0);
        let stack_cy = (min_y + max_y) / 2;

        if p.add_bow_fairing {
            fairings.push(Fairing {
                position: (min_x, stack_cy),
                length: p.fairing_length,
                half_height: stack_half_h,
                side: FairingSide::Bow,
            });
        }
        if p.add_stern_fairing {
            fairings.push(Fairing {
                position: (max_x, stack_cy),
                length: p.fairing_length,
                half_height: stack_half_h,
                side: FairingSide::Stern,
            });
        }
    }

    ShipLayout {
        name: name.to_string(),
        hull_rects,
        hull_curve,
        containers,
        fairings,
        flow_angle: p.flow_angle,
    }
}

/// Génère 3 variantes courantes (grille / pyramide / quinconce) à partir des
/// mêmes dimensions de base, prêtes à passer à `compare_layouts`.
pub fn quick_variants(base: BoatParams) -> Vec<ShipLayout> {
    vec![
        build_boat_layout("grille_3x3", &BoatParams { kind: ContainerLayoutKind::Grid { rows: 3, cols: 3 }, ..base }),
        build_boat_layout("pyramide_4", &BoatParams { kind: ContainerLayoutKind::Pyramid { rows: 4 }, ..base }),
        build_boat_layout("quinconce_3x3", &BoatParams { kind: ContainerLayoutKind::Staggered { rows: 3, cols: 3 }, ..base }),
    ]
}

// ============================================================================
// CONFIGURATIONS ALÉATOIRES (exploration de l'espace des dispositions)
// ============================================================================
//
// Inspiré de la démarche du papier joint (étude de l'effet de la disposition
// des conteneurs et des carénages d'étrave/forecastle sur la traînée
// aérodynamique) : plutôt que de comparer seulement quelques dispositions
// choisies à la main, on tire aléatoirement rows/cols/gap/organisation/
// courbure de coque/carénages pour explorer plus largement l'espace des
// configurations, puis on les classe avec `compare_layouts`.

/// Tire une unique disposition aléatoire de conteneurs (+ coque/carénages),
/// en gardant les dimensions de base (`base`) de la coque et des conteneurs.
pub fn random_layout(name: &str, base: &BoatParams, rng: &mut impl Rng) -> ShipLayout {
    let rows = rng.random_range(1..=6usize);
    let cols = rng.random_range(1..=6usize);
    let kind = match rng.random_range(0..3) {
        0 => ContainerLayoutKind::Grid { rows, cols },
        1 => ContainerLayoutKind::Pyramid { rows },
        _ => ContainerLayoutKind::Staggered { rows, cols },
    };

    let gap = rng.random_range(0.0..=(base.container_gap.max(1.0) * 3.0));
    let curve_bow = rng.random_bool(0.6);
    let curve_stern = rng.random_bool(0.6);
    let curvature = rng.random_range(0.0..=1.0f32);
    let add_bow_fairing = rng.random_bool(0.5);
    let add_stern_fairing = rng.random_bool(0.5);
    let fairing_length = rng.random_range(2.0..=(base.hull_width.max(4.0) * 0.5));

    let params = BoatParams {
        kind,
        container_gap: gap,
        curve_bow,
        curve_stern,
        hull_curvature: curvature,
        add_bow_fairing,
        add_stern_fairing,
        fairing_length,
        ..*base
    };

    build_boat_layout(name, &params)
}

/// Génère `n` configurations aléatoires prêtes à être comparées via
/// `compare_layouts`.
pub fn random_variants(base: BoatParams, n: usize) -> Vec<ShipLayout> {
    let mut rng = rand::rng();
    (0..n)
        .map(|i| random_layout(&format!("alea_{i}"), &base, &mut rng))
        .collect()
}
