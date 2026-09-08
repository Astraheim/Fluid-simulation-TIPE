use crate::conditions::*;
use crate::grid::{Grid, Vector2};
use std::collections::HashMap;

/*
Module de test pour l'organisation des conteneurs sur le bateau.

Idée générale :
    - `wall: bool` (dans Cell) reste la seule chose que le solveur fluide connaît.
    - On garde EN PLUS une table cell_idx -> WallKind pour savoir, après coup,
      quelle cellule murale appartenait à la coque ou à quel conteneur.
    - compute_wall_forces() (déjà existant, pressure_computation.rs) donne la force
      PAR CELLULE, avant l'agrégation par objet connexe. On s'en sert directement
      ici, donc même si un conteneur touche la coque et qu'ils fusionnent dans
      identify_objects(), on peut quand même isoler leur contribution respective.
*/

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WallKind {
    Hull,
    Container(usize), // id du conteneur
}

#[derive(Clone, Debug)]
pub struct Container {
    pub id: usize,
    pub center: (isize, isize), // en cellules de grille
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug)]
pub struct ShipLayout {
    pub name: String,
    /// Rectangles définissant la silhouette de la coque pour cette vue
    /// (dessus / côté / face). Simple pour l'instant : une liste de rectangles
    /// (centre, largeur, hauteur) qu'on stamp comme murs.
    pub hull_rects: Vec<((isize, isize), f32, f32)>,
    pub containers: Vec<Container>,
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

        // 1. Coque
        for &(center, w, h) in &layout.hull_rects {
            self.rectangle(center.0, center.1, w, h);
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

        labels
    }
}

/// Fait tourner `steps` pas de simulation SANS fenêtre (headless), puis
/// calcule les forces séparées coque/conteneurs et le score pondéré.
///
/// NB IMPORTANT sur le couple : compute_object_forces() calcule un couple par
/// OBJET CONNEXE (via identify_objects/DFS), pas par étiquette. Si un conteneur
/// touche la coque, ils forment un seul objet et le couple sera celui du bloc
/// entier, pas celui du conteneur isolé. Pour un couple isolé par conteneur il
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
            WallKind::Hull => {
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
