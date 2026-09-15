use crate::conditions::*;
use crate::grid::{Grid, Vector2, ObjectForce};
// Last update to rendu_code_tex: 2025-05-02
// last modif: 2025-05-02

/*
FR :
    Fonctionnalités clés de l'analyse des forces sur objets solides (murs)

    Détection des objets : Les cellules marquées comme "wall" sont regroupées en objets connexes via une exploration en profondeur (DFS).
    Calcul des forces murales : Pour chaque cellule murale, on évalue les forces dues à la pression du fluide, la traînée (drag) et le cisaillement (shear).
    Agrégation par objet : Les forces sont ensuite regroupées par objet pour obtenir la force totale, le centre de masse et le moment de force (torque).
    Analyse directionnelle : L'orientation principale de la force résultante est estimée, avec une indication directionnelle (ex : ↑, ↘, ←).
    Visualisation : Les résultats sont affichés dans la console pour aider au diagnostic des interactions fluide-objet.

    Améliorations possibles :

    L'accumulation du couple (torque) pourrait tenir compte de la distance physique réelle plutôt que des indices de cellule.
    Une visualisation graphique des forces serait utile pour le debugging et la validation.


ENG :
    Key Features of Solid Object Force Analysis (walls)

    Object Detection : Cells marked as "wall" are grouped into connected objects using depth first search (DFS).
    Wall Force Computation : Each wall cell is subject to pressure, drag, and shear forces from adjacent fluid cells.
    Object-wise Aggregation : Forces are aggregated per object to compute total force, center of mass, and torque.
    Directional Analysis : The main direction of the resulting force is estimated and reported (e.g., ↑, ↘, ←).
    Visualization : Results are printed to the console to support debugging of fluid object interactions.

    Possible Improvements :

    Torque accumulation could use actual physical distances instead of cell indices.
    Graphical visualization of the forces would help with debugging and validation.
*/



impl Grid {


    /// Process the forces of pressure caused by fluid on the walls of the grid
    pub fn compute_wall_forces(&mut self) -> Vec<Vector2> {
        self.refresh_wall_topology_cache();

        let h = 1.0 / N;
        let mut forces = vec![Vector2::default(); self.cells.len()];

        for &idx in &self.wall_cells_cache {
            let (i, j) = self.decode_index(idx);

            let neighbors = [
                (i.wrapping_sub(1), j, Vector2 { x: -1.0, y: 0.0 }),
                (i + 1, j, Vector2 { x: 1.0, y: 0.0 }),
                (i, j.wrapping_sub(1), Vector2 { x: 0.0, y: -1.0 }),
                (i, j + 1, Vector2 { x: 0.0, y: 1.0 }),
            ];

            for &(ni, nj, normal) in &neighbors {
                if let Some(nidx) = self.try_index(ni, nj) {
                    if !self.cells[nidx].wall {
                        let p = self.cells[nidx].pressure;
                        let vx = &self.cells[nidx].velocity_x;
                        let vy = &self.cells[nidx].velocity_y;
                        let v_normal = vx * normal.x + vy * normal.y;
                        let v_tang_x = vx - v_normal * normal.x;
                        let v_tang_y = vy - v_normal * normal.y;

                        let f_pressure = -p * normal;

                        let drag_coef = 0.5;
                        let f_drag_x = drag_coef * v_normal.abs() * v_normal * normal.x;
                        let f_drag_y = drag_coef * v_normal.abs() * v_normal * normal.y;

                        let visc_coef = VISCOSITY;
                        let f_shear_x = visc_coef * v_tang_x;
                        let f_shear_y = visc_coef * v_tang_y;

                        forces[idx].x += (f_pressure.x + f_drag_x + f_shear_x) * h;
                        forces[idx].y += (f_pressure.y + f_drag_y + f_shear_y) * h;
                    }
                }
            }
        }

        forces
    }

    /// Computes total forces on each object
    pub fn compute_object_forces(&mut self) -> Vec<ObjectForce> {
        // compute_wall_forces() rafraîchit le cache en interne (ne rescanne
        // la grille que si des murs ont réellement changé depuis le dernier appel).
        let cell_forces = self.compute_wall_forces();

        let max_id = self.max_object_id_cache;
        if max_id == 0 {
            return Vec::new();
        }

        let mut objects = Vec::with_capacity(max_id);
        for id in 1..=max_id {
            objects.push(ObjectForce {
                id,
                center_of_mass: Vector2::default(),
                total_force: Vector2::default(),
                torque: 0.0,
                cell_count: 0,
            });
        }

        let mut labeled_cells: Vec<(usize, usize, usize, usize)> =
            Vec::with_capacity(self.wall_cells_cache.len());

        for &idx in &self.wall_cells_cache {
            let obj_id = self.object_ids_cache[idx];
            if obj_id > 0 {
                let obj_idx = obj_id - 1;
                let (i, j) = self.decode_index(idx);

                objects[obj_idx].center_of_mass.x += i as f32;
                objects[obj_idx].center_of_mass.y += j as f32;
                objects[obj_idx].cell_count += 1;

                objects[obj_idx].total_force.x += cell_forces[idx].x;
                objects[obj_idx].total_force.y += cell_forces[idx].y;

                labeled_cells.push((i, j, idx, obj_idx));
            }
        }

        for obj in &mut objects {
            if obj.cell_count > 0 {
                obj.center_of_mass.x /= obj.cell_count as f32;
                obj.center_of_mass.y /= obj.cell_count as f32;
            }
        }

        for &(i, j, idx, obj_idx) in &labeled_cells {
            let com = objects[obj_idx].center_of_mass;
            let r_x = i as f32 - com.x;
            let r_y = j as f32 - com.y;
            objects[obj_idx].torque += r_x * cell_forces[idx].y - r_y * cell_forces[idx].x;
        }

        objects
    }

    /// Objects identification (mise en cache — voir refresh_wall_topology_cache).
    /// Gardée publique pour compatibilité, mais compute_wall_forces /
    /// compute_object_forces n'appellent plus cette fonction en interne :
    /// ils lisent directement le cache.
    pub fn identify_objects(&mut self) -> Vec<usize> {
        self.refresh_wall_topology_cache();
        self.object_ids_cache.clone()
    }

    /// Recalcule la connectivité des murs (ids d'objets) et la liste des
    /// cellules murales, mais seulement si la topologie a réellement changé
    /// depuis le dernier appel (voir `wall_init` dans grid.rs, qui bascule
    /// le drapeau `wall_topology_dirty`). Transforme
    /// identify_objects/compute_wall_forces/compute_object_forces d'un
    /// "scan complet de la grille à chaque frame" en "scan complet
    /// seulement quand un mur est réellement ajouté".
    fn refresh_wall_topology_cache(&mut self) {
        if !self.wall_topology_dirty {
            return;
        }

        let mut object_ids = vec![0; self.cells.len()];
        let mut wall_cells = Vec::new();
        let mut current_id = 1;
        let mut stack = Vec::new();

        for (i, j, idx) in self.iter_morton() {
            if self.cells[idx].wall {
                wall_cells.push(idx);

                if object_ids[idx] == 0 {
                    object_ids[idx] = current_id;
                    stack.push((i, j));

                    while let Some((ci, cj)) = stack.pop() {
                        let neighbors = [
                            (ci.wrapping_sub(1), cj),
                            (ci + 1, cj),
                            (ci, cj.wrapping_sub(1)),
                            (ci, cj + 1),
                        ];

                        for &(ni, nj) in &neighbors {
                            if let Some(nidx) = self.try_index(ni, nj) {
                                if self.cells[nidx].wall && object_ids[nidx] == 0 {
                                    object_ids[nidx] = current_id;
                                    stack.push((ni, nj));
                                }
                            }
                        }
                    }

                    current_id += 1;
                }
            }
        }

        self.max_object_id_cache = current_id - 1;
        self.object_ids_cache = object_ids;
        self.wall_cells_cache = wall_cells;
        self.wall_topology_dirty = false;
    }


    /// Identifies objects, compute forces and print them
    pub fn print_object_forces(&mut self) {
        let objects = self.compute_object_forces();

        if objects.is_empty() {
            println!("Aucun objet détecté.");
            return;
        }

        println!("\n=== Forces sur les objets ===");

        for obj in &objects {
            if obj.cell_count < OBJET_SIZE_PRINT_LIMIT {
                continue;
            }

            if 1==1 {
                let force_magnitude = obj.total_force.magnitude();

                println!("Objet #{} :", obj.id);
                println!("  - Nombre de cellules: {}", obj.cell_count);
                println!("  - Centre de masse: ({:.2}, {:.2})", obj.center_of_mass.x, obj.center_of_mass.y);
                println!("  - Force totale: ({:.4}, {:.4}) [magnitude: {:.4}]",
                         obj.total_force.x, obj.total_force.y, force_magnitude);
                println!("  - Moment de force: {:.4}", obj.torque);

                if force_magnitude > 0.001 {
                    let direction = obj.total_force.normalize();
                    println!("  - Direction de la force: ({:.2}, {:.2})", direction.x, direction.y);

                    let angle = direction.y.atan2(direction.x) * 180.0 / std::f32::consts::PI;
                    let direction_desc = match angle {
                        a if a > -22.5 && a <= 22.5 => "→ (droite)",
                        a if a > 22.5 && a <= 67.5 => "↗ (haut-droite)",
                        a if a > 67.5 && a <= 112.5 => "↑ (haut)",
                        a if a > 112.5 && a <= 157.5 => "↖ (haut-gauche)",
                        a if a > 157.5 || a <= -157.5 => "← (gauche)",
                        a if a > -157.5 && a <= -112.5 => "↙ (bas-gauche)",
                        a if a > -112.5 && a <= -67.5 => "↓ (bas)",
                        _ => "↘ (bas-droite)",
                    };
                    println!("  - Orientation: {} ({:.1}°)", direction_desc, angle);
                } else {
                    println!("  - Force négligeable");
                }
                println!();

                println!("===================\n \n");
            }
        }
    }
}