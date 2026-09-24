use crate::conditions::*;
use crate::grid::Grid;
use rayon::prelude::*;

/*
FR :
    Module de simulation bi-fluide (eau + air) par méthode VOF (Volume Of Fluid),
    greffé sur la grille existante `Grid` (grid.rs) sans modifier son architecture :
    un seul champ de vitesse partagé pour les deux fluides, plus un champ scalaire
    `phase` par cellule (0.0 = air pur, 1.0 = eau pure, valeurs intermédiaires =
    interface diffuse entre les deux).

    Fonctionnement :
    - `phase` est advecté exactement comme `density` (même schéma semi-lagrangien).
    - La densité physique locale ρ(i,j) est interpolée entre RHO_AIR et RHO_WATER
      en fonction de `phase`, via `local_rho`.
    - La gravité est appliquée aux faces de vitesse (apply_gravity), ce qui crée
      la surface libre et la poussée d'Archimède une fois combinée à la
      projection de pression pondérée par densité (voir la modification demandée
      dans project(), grid.rs — cf. INTEGRATION.md).
    - Les effets "inter-fluides" (vagues, éclaboussures, sillage à l'interface)
      ne sont PAS codés à la main : ils émergent naturellement de l'advection de
      `phase` combinée à la gravité et à la projection de pression correcte.

    Prérequis dans grid.rs (voir INTEGRATION.md) :
    - Le struct `Cell` doit avoir un champ `pub phase: f32,`.
    - `project()` doit pondérer chaque face par 1/ρ_locale au lieu de 1/4.

ENG :
    Two-fluid (water + air) simulation module using the VOF (Volume Of Fluid)
    method, grafted onto the existing `Grid` grid (grid.rs) without changing its
    architecture: a single shared velocity field for both fluids, plus a scalar
    `phase` field per cell (0.0 = pure air, 1.0 = pure water, intermediate values
    = diffuse interface between the two).

    How it works:
    - `phase` is advected exactly like `density` (same semi-Lagrangian scheme).
    - The local physical density ρ(i,j) is interpolated between RHO_AIR and
      RHO_WATER based on `phase`, via `local_rho`.
    - Gravity is applied at velocity faces (apply_gravity), which creates the
      free surface and buoyancy once combined with density-weighted pressure
      projection (see the required change in project(), grid.rs — see
      INTEGRATION.md).
    - "Inter-fluid" effects (waves, splashes, wake at the interface) are NOT
      hand-coded: they emerge naturally from advecting `phase` combined with
      gravity and correct pressure projection.

    Prerequisites in grid.rs (see INTEGRATION.md):
    - The `Cell` struct must have a `pub phase: f32,` field.
    - `project()` must weight each face by 1/ρ_local instead of 1/4.
*/

impl Grid {
    /// Densité physique locale de la cellule `idx`, interpolée entre
    /// RHO_AIR (phase = 0) et RHO_WATER (phase = 1).
    #[inline]
    pub fn local_rho(&self, idx: usize) -> f32 {
        let phi = self.cells[idx].phase.clamp(0.0, 1.0);
        RHO_AIR * (1.0 - phi) + RHO_WATER * phi
    }

    /// Initialise une ligne de flottaison plane : toute cellule avec
    /// j >= water_level devient eau (phase = 1.0), le reste devient air
    /// (phase = 0.0). Les cellules murales ne sont pas modifiées.
    ///
    /// À appeler une fois juste après `Grid::new()` (et après avoir posé vos
    /// murs de coque/conteneurs), avant la boucle de simulation.
    pub fn init_water(&mut self, water_level: f32) {
        // On collecte d'abord les (i, j, idx) à traiter : iter_morton()
        // emprunte `self` immutablement, on ne peut donc pas muter
        // `self.cells` tant que cet itérateur est encore utilisé.
        let targets: Vec<(usize, usize, usize)> = self
            .iter_morton()
            .filter(|&(i, j, _)| self.in_bounds(i, j))
            .collect();

        for (_, j, idx) in targets {
            if self.cells[idx].wall {
                continue;
            }
            self.cells[idx].phase = if (j as f32) >= water_level { 1.0 } else { 0.0 };
        }
    }

    /// Remplit toute la grille (hors murs) d'un seul fluide. Pratique pour
    /// débugger / comparer avec le comportement mono-fluide d'origine :
    /// `grid.fill_uniform_phase(0.0)` désactive de fait l'eau même si
    /// ENABLE_WATER est true.
    pub fn fill_uniform_phase(&mut self, phase: f32) {
        for cell in self.cells.iter_mut() {
            if !cell.wall {
                cell.phase = phase.clamp(0.0, 1.0);
            }
        }
    }

    /// Applique la gravité aux faces de vitesse (u et v), en sautant les
    /// faces adjacentes à un mur (pas de force sur/dans un solide). Contrairement
    /// à une gravité appliquée au centre des cellules, ceci reste cohérent avec
    /// votre grille décalée (staggered) existante où u/v vivent sur les faces.
    pub fn apply_gravity(&mut self, gx: f32, gy: f32, dt: f32) {
        let n = N as usize;

        // Composante horizontale : faces verticales u(i,j), i = 1..=n+1
        for j in 1..=n {
            for i in 1..=n + 1 {
                let idx_here = self.to_index(i.min(n), j);
                if self.cells[idx_here].wall {
                    continue;
                }
                self.cells[idx_here].velocity_x += gx * dt;
            }
        }

        // Composante verticale : faces horizontales v(i,j), j = 1..=n+1
        for j in 1..=n + 1 {
            for i in 1..=n {
                let idx_here = self.to_index(i, j.min(n));
                if self.cells[idx_here].wall {
                    continue;
                }
                self.cells[idx_here].velocity_y += gy * dt;
            }
        }
    }

    /// Échantillonnage bilinéaire du champ `phase`, avec réflexion aux murs
    /// (même logique que `get_density` dans `advect_density`, grid.rs).
    fn sample_phase(&self, x: f32, y: f32, fallback: f32) -> f32 {
        let x = x.clamp(0.5, N + 0.5);
        let y = y.clamp(0.5, N + 0.5);

        let i0 = x.floor() as usize;
        let i1 = i0 + 1;
        let j0 = y.floor() as usize;
        let j1 = j0 + 1;

        let s1 = x - i0 as f32;
        let s0 = 1.0 - s1;
        let t1 = y - j0 as f32;
        let t0 = 1.0 - t1;

        let get_phase = |i: usize, j: usize| -> f32 {
            if i <= N as usize && j <= N as usize {
                let idx = self.to_index(i, j);
                if idx < self.cells.len() {
                    return if !self.cells[idx].wall {
                        self.cells[idx].phase
                    } else {
                        fallback
                    };
                }
            }
            fallback
        };

        s0 * (t0 * get_phase(i0, j0) + t1 * get_phase(i0, j1))
            + s1 * (t0 * get_phase(i1, j0) + t1 * get_phase(i1, j1))
    }

    /// Advecte le champ `phase` (fraction volumique d'eau) le long du champ
    /// de vitesse courant, avec le même schéma semi-lagrangien que
    /// `advect_density` (grid.rs). C'est cette fonction qui fait "avancer"
    /// la surface libre / l'interface eau-air à chaque pas de temps.
    pub fn advect_phase(&mut self, dt: f32) {
        let dt0 = dt * N;

        let new_phase: Vec<f32> = self
            .cells
            .par_iter()
            .enumerate()
            .map(|(idx, cell)| {
                let (i, j) = self.decode_index(idx);

                if cell.wall || i == 0 || i > N as usize || j == 0 || j > N as usize {
                    return cell.phase;
                }

                let u_left = self.get_u(i, j);
                let u_right = self.get_u(i + 1, j);
                let v_bottom = self.get_v(i, j);
                let v_top = self.get_v(i, j + 1);

                let u_center = 0.5 * (u_left + u_right);
                let v_center = 0.5 * (v_bottom + v_top);

                let x = i as f32 - dt0 * u_center;
                let y = j as f32 - dt0 * v_center;

                self.sample_phase(x, y, cell.phase).clamp(0.0, 1.0)
            })
            .collect();

        for (idx, &p) in new_phase.iter().enumerate() {
            self.cells[idx].phase = p;
        }
    }

    /// Volume total d'eau dans la grille (somme de `phase` sur les cellules
    /// non murales). Utile pour vérifier la conservation approximative de la
    /// masse d'eau au fil de la simulation (le VOF sur grille eulérienne
    /// diffuse un peu l'interface, un volume qui dérive légèrement est
    /// normal, une dérive massive indique un bug).
    pub fn water_volume(&self) -> f32 {
        self.cells
            .iter()
            .filter(|c| !c.wall)
            .map(|c| c.phase)
            .sum()
    }

    /// NOTE IMPORTANTE SUR L'ORDRE D'APPEL :
    /// `apply_gravity` doit être appelé AVANT `project()` (pour que la
    /// pression puisse réagir à la gravité), mais `advect_phase` doit être
    /// appelé APRÈS `project()` ET `apply_boundary_conditions()` — jamais
    /// avant, et jamais regroupés dans un seul appel en début de tick.
    ///
    /// Raison : entre l'ajout de la gravité et l'application des conditions
    /// aux limites, la face de fond (v(i, N+1)) porte une petite vitesse
    /// vers le bas qui n'a pas encore été annulée par la condition de sol
    /// solide. Si `advect_phase` utilise ce champ non corrigé, l'eau fuit
    /// légèrement par le fond à chaque tick — c'est ce qui produit
    /// l'impression d'implosion observée. Voir INTEGRATION.md, section 4,
    /// pour le placement exact des deux appels dans vel2_step.

    /// Sécurité anti-divergence : limite la norme de chaque composante de
    /// vitesse. À utiliser pendant la mise au point du solveur bi-fluide
    /// (voir STABILITE_PERF.md, section C) ; une fois la convergence de
    /// `project()` validée, cette fonction ne devrait plus jamais être
    /// sollicitée en régime normal — si elle l'est en permanence, c'est le
    /// signe que le solveur de pression ne converge pas assez vite pour le
    /// ratio de densité actuel (voir STABILITE_PERF.md, section D).
    pub fn clamp_velocity_field(&mut self, max_speed: f32) {
        for cell in self.cells.iter_mut() {
            if cell.wall {
                continue;
            }
            cell.velocity_x = cell.velocity_x.clamp(-max_speed, max_speed);
            cell.velocity_y = cell.velocity_y.clamp(-max_speed, max_speed);
        }
    }
}
