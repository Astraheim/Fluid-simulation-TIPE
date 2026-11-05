use crate::conditions::*;
use std::vec::Vec;
use rayon::prelude::*;
use std::fmt::Write;
use std::ops::Mul;

#[derive(Clone, Copy, Debug, Default)]
pub struct Vector2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Cell {
    // Staggered grid: vitesses stockées sur les interfaces
    pub velocity_x: f32,  // vitesse horizontale (stockée sur interfaces E/O)
    pub velocity_y: f32,  // vitesse verticale (stockée sur interfaces N/S)
    pub pressure: f32,    // pression au centre
    pub density: f32,     // densité au centre
    pub density_x: f32,
    pub density_y: f32,
    pub density_xx: f32,
    pub density_yy: f32,
    pub density_xy: f32,
    pub wall: bool,
    pub phase: f32,
}


// Modifier Grid pour accéder aux vitesses correctement
impl Grid {
    // Nouvelles méthodes pour accéder aux vitesses selon leur position

    /// Obtenir la vitesse horizontale u(i, j) (située sur la face entre (i-1,j) et (i, j))
    pub fn get_u(&self, i: usize, j: usize) -> f32 {
        if i > 0 && i <= (N + 1.0) as usize && j <= (N + 1.0) as usize {
            let idx = self.to_index(i, j);
            self.cells[idx].velocity_x
        } else {
            0.0 // Hors limites
        }
    }

    /// Obtenir la vitesse verticale v(i, j) (située sur la face entre (i, j-1) et (i, j))
    pub fn get_v(&self, i: usize, j: usize) -> f32 {
        if i <= (N + 1.0) as usize && j > 0 && j <= (N + 1.0) as usize {
            let idx = self.to_index(i, j);
            self.cells[idx].velocity_y
        } else {
            0.0 // Hors limites
        }
    }

    /// Obtenir un vecteur de vitesse au centre de la cellule (i, j) par interpolation
    pub fn get_velocity_at_center(&self, i: usize, j: usize) -> Vector2 {
        let u_left = self.get_u(i, j);
        let u_right = self.get_u(i + 1, j);
        let v_bottom = self.get_v(i, j);
        let v_top = self.get_v(i, j + 1);

        Vector2 {
            x: 0.5 * (u_left + u_right),
            y: 0.5 * (v_bottom + v_top),
        }
    }

    /// Définir la vitesse horizontale u(i,j)
    pub fn set_u(&mut self, i: usize, j: usize, value: f32) {
        if i > 0 && i <= (N + 1.0) as usize && j <= (N + 1.0) as usize {
            let idx = self.to_index(i, j);
            self.cells[idx].velocity_x = value;
        }
    }

    /// Définir la vitesse verticale v(i,j)
    pub fn set_v(&mut self, i: usize, j: usize, value: f32) {
        if i <= (N + 1.0) as usize && j > 0 && j <= (N + 1.0) as usize {
            let idx = self.to_index(i, j);
            self.cells[idx].velocity_y = value;
        }
    }
}

#[derive(Clone, Debug)]
pub struct Grid {
    pub wind_min_height: f32,
    pub realistic_wind: bool,
    pub projection_iterations: usize,
    pub projection_residual: f64,
    pub projection_converged: bool,
    pub cells: Vec<Cell>,
    /// True dès qu'un mur a été ajouté depuis le dernier calcul du cache
    /// ci-dessous (voir `wall_init`, seule fonction qui pose un mur).
    pub(crate) wall_topology_dirty: bool,
    /// Liste en cache des indices des cellules murales (ordre morton).
    pub(crate) wall_cells_cache: Vec<usize>,
    /// Id d'objet par cellule en cache (0 = pas un mur), issu du dernier DFS.
    pub(crate) object_ids_cache: Vec<usize>,
    /// Id d'objet maximum en cache, issu du dernier DFS.
    pub(crate) max_object_id_cache: usize,
}


#[derive(Clone, Debug)]
pub struct ObjectForce {
    pub id: usize,
    pub center_of_mass: Vector2,
    pub total_force: Vector2,
    pub torque: f32,
    pub cell_count: usize,
}



impl Mul<f32> for Vector2 {
    type Output = Vector2;

    fn mul(self, rhs: f32) -> Vector2 {
        Vector2 {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl Mul<Vector2> for f32 {
    type Output = Vector2;

    fn mul(self, rhs: Vector2) -> Vector2 {
        Vector2 {
            x: self * rhs.x,
            y: self * rhs.y,
        }
    }
}



impl Vector2 {

    /// Return vector magnitude
    pub fn magnitude(&self) -> f32 {
        (self.x.powi(2) + self.y.powi(2)).sqrt()
    }

    /// Create a new vector
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Exactly the same as magnitude, return vector magnitude
    pub fn length(&self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Create a new vector with the same direction but normalized
    pub fn normalize(&self) -> Self {
        let len = self.length();
        if len == 0.0 {
            Self::new(0.0, 0.0)
        } else {
            Self::new(self.x / len, self.y / len)
        }
    }
}


/// Encode Morton coordinates (x, y) into a single index
fn morton_encode(x: usize, y: usize) -> usize {
    part1by1(x) | (part1by1(y) << 1)
}

/// Decode Morton index into coordinates (x, y)
fn morton_decode(z: usize) -> (usize, usize) {
    (compact1by1(z), compact1by1(z >> 1))
}

/// Interleave bits of x and y (morton_encode)
fn part1by1(mut n: usize) -> usize {
    n &= 0x0000ffff;
    n = (n | (n << 8)) & 0x00ff00ff;
    n = (n | (n << 4)) & 0x0f0f0f0f;
    n = (n | (n << 2)) & 0x33333333;
    n = (n | (n << 1)) & 0x55555555;
    n
}

/// Compact bits of n into a single number (morton_decode)
fn compact1by1(mut n: usize) -> usize {
    n &= 0x55555555;
    n = (n | (n >> 1)) & 0x33333333;
    n = (n | (n >> 2)) & 0x0f0f0f0f;
    n = (n | (n >> 4)) & 0x00ff00ff;
    n = (n | (n >> 8)) & 0x0000ffff;
    n
}

impl Grid {

    /// Check if the coordinates (i, j) are within the grid bounds
    pub fn in_bounds(&self, i: usize, j: usize) -> bool {
        i >= 1 && i <= N as usize && j >= 1 && j <= N as usize
    }

    /// Encoding to morton index
    pub fn to_index(&self, i: usize, j: usize) -> usize {
        morton_encode(i, j)
    }

    /// Return the coordinates (i, j) from the morton index
    pub fn decode_index(&self, idx: usize) -> (usize, usize) {
        morton_decode(idx)
    }

    /// Return Some(idx) if the index is valid, otherwise None
    pub fn try_index(&self, i: usize, j: usize) -> Option<usize> {
        let idx = self.to_index(i, j);
        if idx < self.cells.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Iterate on every cell in Morton order
    pub fn iter_morton(&self) -> impl Iterator<Item = (usize, usize, usize)> + '_ {
        self.cells
            .iter()
            .enumerate()
            .map(move |(idx, _)| {
                let (i, j) = self.decode_index(idx);
                (i, j, idx)
            })
    }


    /// Create a new grid with the specified size (with wall if EXT_BORDER is enabled)
    pub fn new() -> Self {
        let mut grid = Self {
            wind_min_height: 0.0,
            realistic_wind: true,
            projection_iterations: 0,
            projection_residual: 0.0,
            projection_converged: true,
            cells: vec![Cell::default(); SIZE as usize],
            wall_topology_dirty: true,
            wall_cells_cache: Vec::new(),
            object_ids_cache: vec![0; SIZE as usize],
            max_object_id_cache: 0,
        };

        if EXT_BORDER == true {
            for i in 0..=(N + 1.0) as usize {
                for &j in &[0, (N + 1.0) as usize] {
                    if let Some(idx) = grid.try_index(i, j) {
                        grid.cells[idx].wall = true;
                    }
                }
            }
            for j in 0..=(N + 1.0) as usize {
                for &i in &[0, (N + 1.0) as usize] {
                    if let Some(idx) = grid.try_index(i, j) {
                        grid.cells[idx].wall = true;
                    }
                }
            }
        }
        grid
    }


    /// Adds a source to the pressure of the cells
    pub fn add_source(&mut self, source: &[f32], dt: f32) {
        self.cells.par_iter_mut().enumerate().for_each(|(i, cell)| {
            cell.pressure += dt * source[i];
        });
    }

    /// Diffuse density in the grid
    pub fn diffuse(&mut self, diff: f32, dt: f32) {
        let a = dt * diff * (N * N) ;
        let mut new_density = vec![0.0; self.cells.len()];

        for _ in 0..20 {
            new_density.par_iter_mut()
                .zip(self.cells.par_iter())
                .enumerate()
                .for_each(|(idx, (new_cell, cell))| {
                    let (x, y) = morton_decode(idx);
                    if cell.wall {
                        *new_cell = cell.density; // Conservation of density in walls
                        return;
                    }

                    let mut sum = 0.0;
                    let mut count = 0;

                    for &(di, dj) in &[(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let ni = (x as isize + di) as usize;
                        let nj = (y as isize + dj) as usize;
                        if ni > 0 && ni <= N as usize && nj > 0 && nj <= N as usize {
                            let n_idx = morton_encode(ni, nj);
                            if !self.cells[n_idx].wall {
                                sum += self.cells[n_idx].density;
                                count += 1;
                            } else {
                                sum += cell.density;
                                count += 1;
                            }
                        }
                    }

                    if count > 0 {
                        *new_cell = (cell.density + a * sum) / (1.0 + a * count as f32);
                    }
                });

            // Update densities
            for (cell, &new_dens) in self.cells.iter_mut().zip(new_density.iter()) {
                cell.density = new_dens;
            }
        }
    }

    /// Advect the velocity of the cells in the grid
    // Modification de advect_velocity pour une staggered grid — PARALLÉLISÉE :
    // chaque new_u[idx]/new_v[idx] ne dépend que de l'état figé de self (comme
    // pour advect_density), donc on calcule les deux champs avec rayon plutôt
    // qu'avec une double boucle séquentielle.
    pub fn advect_velocity(&mut self, dt: f32) {
        let dt0 = dt * N;
        let n = N as usize;

        let u_updates: Vec<(usize, f32)> = (0..n * n)
            .into_par_iter()
            .map(|k| {
                let i = k / n + 1;
                let j = k % n + 1;
                let idx = self.to_index(i, j);

                if self.cells[idx].wall {
                    return (idx, 0.0);
                }

                // Position de u(i,j) sur la grille
                let pos_x = i as f32; // face entre (i-1,j) et (i,j)
                let pos_y = j as f32 + 0.5; // centré verticalement

                let u_center = self.get_u(i, j);

                let v_nw = self.get_v(i - 1, j);
                let v_ne = self.get_v(i, j);
                let v_sw = self.get_v(i - 1, j + 1);
                let v_se = self.get_v(i, j + 1);
                let v_center = 0.25 * (v_nw + v_ne + v_sw + v_se);

                // Backtracking + clamp aux frontières
                let x = (pos_x - dt0 * u_center).clamp(1.0, N + 1.0);
                // Convertir la position physique en indices du tableau u.
                let y = (pos_y - dt0 * v_center - 0.5).clamp(0.5, N + 0.5);

                let i0 = x.floor() as usize;
                let i1 = i0 + 1;
                let j0 = y.floor() as usize;
                let j1 = j0 + 1;

                let s1 = x - i0 as f32;
                let s0 = 1.0 - s1;
                let t1 = y - j0 as f32;
                let t0 = 1.0 - t1;

                let sample_u = |i: usize, j: usize| -> f32 {
                    if let Some(sidx) = self.try_index(i, j) {
                        if self.cells[sidx].wall {
                            return 0.0;
                        }
                    }
                    self.get_u(i, j)
                };

                let value = s0 * (t0 * sample_u(i0, j0) + t1 * sample_u(i0, j1))
                    + s1 * (t0 * sample_u(i1, j0) + t1 * sample_u(i1, j1));

                (idx, value)
            })
            .collect();

        let v_updates: Vec<(usize, f32)> = (0..n * n)
            .into_par_iter()
            .map(|k| {
                let i = k / n + 1;
                let j = k % n + 1;
                let idx = self.to_index(i, j);

                if self.cells[idx].wall {
                    return (idx, 0.0);
                }

                // Position de v(i,j) sur la grille
                let pos_x = i as f32 + 0.5; // centré horizontalement
                let pos_y = j as f32; // face entre (i,j-1) et (i,j)

                let v_center = self.get_v(i, j);

                let u_nw = self.get_u(i, j - 1);
                let u_ne = self.get_u(i + 1, j - 1);
                let u_sw = self.get_u(i, j);
                let u_se = self.get_u(i + 1, j);
                let u_center = 0.25 * (u_nw + u_ne + u_sw + u_se);

                // Convertir la position physique en indices du tableau v.
                let x = (pos_x - dt0 * u_center - 0.5).clamp(0.5, N + 0.5);
                let y = (pos_y - dt0 * v_center).clamp(0.5, N + 0.5);

                let i0 = x.floor() as usize;
                let i1 = i0 + 1;
                let j0 = y.floor() as usize;
                let j1 = j0 + 1;

                let s1 = x - i0 as f32;
                let s0 = 1.0 - s1;
                let t1 = y - j0 as f32;
                let t0 = 1.0 - t1;

                let sample_v = |i: usize, j: usize| -> f32 {
                    if let Some(sidx) = self.try_index(i, j) {
                        if self.cells[sidx].wall { return 0.0; }
                    }
                    self.get_v(i, j)
                };

                let value = s0 * (t0 * sample_v(i0, j0) + t1 * sample_v(i0, j1))
                    + s1 * (t0 * sample_v(i1, j0) + t1 * sample_v(i1, j1));

                (idx, value)
            })
            .collect();

        // Mettre à jour les vitesses (partie séquentielle, mais O(n²) simple
        // écriture, négligeable comparé au calcul ci-dessus)
        for (idx, value) in u_updates {
            self.cells[idx].velocity_x = value;
        }
        for (idx, value) in v_updates {
            self.cells[idx].velocity_y = value;
        }
    }


    /// Advect density in the grid
    // Modification de advect_density pour une staggered grid
    pub fn advect_density(&mut self, dt: f32) {
        let dt0 = dt * N;

        // Nouvelles densités après advection
        let new_densities: Vec<f32> = self.cells.par_iter().enumerate().map(|(idx, cell)| {
            let (i, j) = morton_decode(idx);

            // Préserver la densité pour les murs et les frontières
            if cell.wall || i == 0 || i > N as usize || j == 0 || j > N as usize {
                return cell.density;
            }

            // Obtenir les vitesses au centre de la cellule (i, j) par interpolation
            let u_left = self.get_u(i, j);
            let u_right = self.get_u(i+1, j);
            let v_bottom = self.get_v(i, j);
            let v_top = self.get_v(i, j+1);

            let u_center = 0.5 * (u_left + u_right);
            let v_center = 0.5 * (v_bottom + v_top);

            // Backtracking: d'où venait la particule de fluide
            let x = i as f32 - dt0 * u_center;
            let y = j as f32 - dt0 * v_center;

            // Limiter avec réflexion aux frontières
            let x = x.clamp(0.5, N + 0.5);
            let y = y.clamp(0.5, N + 0.5);

            // Indices et poids pour l'interpolation bilinéaire
            let i0 = x.floor() as usize;
            let i1 = i0 + 1;
            let j0 = y.floor() as usize;
            let j1 = j0 + 1;

            let s1 = x - i0 as f32;
            let s0 = 1.0 - s1;
            let t1 = y - j0 as f32;
            let t0 = 1.0 - t1;

            // Fonction améliorée pour obtenir la densité avec réflexion aux murs
            let get_density = |i: usize, j: usize| -> f32 {
                if i <= N as usize && j <= N as usize {
                    let idx = self.to_index(i, j);
                    if idx < self.cells.len() {
                        return if !self.cells[idx].wall {
                            self.cells[idx].density
                        } else {
                            // Réflexion : utiliser la densité de la cellule actuelle
                            cell.density
                        }
                    }
                }
                // Pour les cellules hors limites, utiliser la densité de la cellule actuelle
                cell.density
            };

            // Interpolation bilinéaire
            let density = s0 * (t0 * get_density(i0, j0) + t1 * get_density(i0, j1)) +
                s1 * (t0 * get_density(i1, j0) + t1 * get_density(i1, j1));

            density.max(0.0) // Éviter les valeurs négatives
        }).collect();

        // Mettre à jour les densités
        for (i, &new_density) in new_densities.iter().enumerate() {
            self.cells[i].density = new_density;
        }
    }

    /// Generate 5 test shapes evenly distributed vertically, aligned for force testing
    pub fn setup_force_test_objects(&mut self) {
        let n = N as isize;
        let x_center = n / 4; // Aligned on the left quarter to be hit by the inflow

        // 5 shapes evenly spaced over N pixels
        // Spacing: N / 6 gives margins top and bottom
        let spacing = n / 7;
        let size = (spacing / 4) as f32; // Size ~1/3 of spacing so they don't influence each other

        let positions_y = [
            spacing,          // 1 — Circle
            spacing * 2,      // 2 — Square
            spacing * 3,      // 3 — Triangle
            spacing * 4,      // 4 — Horizontal bar
            spacing * 5,      // 5 — Vertical bar
        ];

        // 1 — Circle
        self.circle(x_center, positions_y[1], size );

        // 2 — Square
        self.square(x_center, positions_y[0], size *2.0);

        // 3 — Triangle
        self.triangle(x_center, positions_y[2], size * 2.0);

        // 4 — Horizontal bar (thin rectangle, wide)
        self.rectangle(x_center, positions_y[3], size * 4.0, (size * 0.3).max(2.0));

        // 5 — Vertical bar (thin rectangle, tall)
        self.rectangle(x_center, positions_y[4], (size * 0.3).max(2.0), size * 4.0);

        println!("✔ Force test setup: 5 shapes placed at x={}, y={:?}", x_center, positions_y);
    }

    /// Generate a splitter structure: a square with two angled bars forming a V in front (left side)
    pub fn setup_splitter(&mut self, angle: f32, size: f32, length: f32, gap: f32) {
        let n = N as isize;
        let center_x = n / 2;
        let center_y = n / 2;
        let square_size = size * 0.6;

        // Central square
        self.square(center_x, center_y, square_size);

        let angle_rad = (-angle) * std::f32::consts::PI / 180.0;
        let dx = (length * angle_rad.sin()) as isize;
        let dy = (length * angle_rad.cos()) as isize;

        // Attachment point: gap away from the left face of the square
        let attach_x = center_x - (square_size / 2.0) as isize - gap as isize;
        let attach_y = center_y;

        // End points of each bar
        let top_end_x = attach_x - dx;
        let top_end_y = attach_y - dy;
        let bot_end_x = attach_x - dx;
        let bot_end_y = attach_y + dy;

        // Draw thick bars (5 px width)
        for offset in -2isize..=2isize {
            // Upper bar
            self.draw_line(attach_x, attach_y + offset, top_end_x, top_end_y + offset);
            self.draw_line(attach_x + offset, attach_y, top_end_x + offset, top_end_y);

            // Lower bar
            self.draw_line(attach_x, attach_y + offset, bot_end_x, bot_end_y + offset);
            self.draw_line(attach_x + offset, attach_y, bot_end_x + offset, bot_end_y);
        }
    }


    /// Compte le nombre de voisins non-mur pour la cellule (i, j)
    pub fn count_non_wall_neighbors(&self, i: usize, j: usize) -> usize {
        let mut count = 0;
        let directions = [(-1isize, 0), (1, 0), (0, -1), (0, 1)];
        for (di, dj) in directions.iter() {
            let ni = i as isize + di;
            let nj = j as isize + dj;
            if ni >= 0 && nj >= 0 && ni <= N as isize && nj <= N as isize {
                let idx = self.to_index(ni as usize, nj as usize);
                if !self.cells[idx].wall {
                    count += 1;
                }
            }
        }
        count
    }


    /// Project the velocity field to ensure incompressibility
    // Modification of project to work with staggered grid and walls
    pub fn project(&mut self) {
        self.enforce_solid_faces();
        let n = N as usize;
        let h = 1.0 / N as f64;
        let mut cells = Vec::new();
        let mut ids = vec![usize::MAX; self.cells.len()];
        for i in 1..=n {
            for j in 1..=n {
                let idx = self.to_index(i, j);
                if !self.cells[idx].wall {
                    ids[idx] = cells.len();
                    cells.push((i, j, idx));
                }
            }
        }
        let count = cells.len();
        let mut neighbors = vec![[0; 4]; count];
        let mut weights = vec![[0.0; 4]; count];
        let mut diagonal = vec![0.0; count];
        let mut rhs = vec![0.0; count];
        for (row, &(i, j, idx)) in cells.iter().enumerate() {
            neighbors[row] = [row; 4];
            rhs[row] = -h
                * (self.get_u(i + 1, j) as f64 - self.get_u(i, j) as f64
                    + self.get_v(i, j + 1) as f64
                    - self.get_v(i, j) as f64);
            let rho = if ENABLE_WATER {
                self.local_rho(idx) as f64
            } else {
                1.0
            };
            for (direction, (ni, nj)) in [(i - 1, j), (i + 1, j), (i, j - 1), (i, j + 1)]
                .into_iter()
                .enumerate()
            {
                let other = self.to_index(ni, nj);
                if self.cells[other].wall {
                    continue;
                }
                if self.in_bounds(ni, nj) {
                    let other_rho = if ENABLE_WATER {
                        self.local_rho(other) as f64
                    } else {
                        1.0
                    };
                    let weight = 2.0 / (rho + other_rho);
                    neighbors[row][direction] = ids[other];
                    weights[row][direction] = weight;
                    diagonal[row] += weight;
                } else if ni == n + 1 {
                    // p=0 à la face de sortie, distante de h/2 du centre.
                    diagonal[row] += 2.0 / rho;
                }
            }
        }
        let (pressure, iterations, residual, converged) =
            solve_pressure(&neighbors, &weights, &diagonal, &rhs, 4000, 1e-5, 1e-10);
        self.projection_iterations = iterations;
        self.projection_residual = residual;
        self.projection_converged = converged;
        for (row, &(i, j, idx)) in cells.iter().enumerate() {
            self.cells[idx].pressure = pressure[row] as f32;
            if i > 1 && weights[row][0] > 0.0 {
                self.cells[idx].velocity_x -=
                    (weights[row][0] * (pressure[row] - pressure[neighbors[row][0]]) / h) as f32;
            }
            if j > 1 && weights[row][2] > 0.0 {
                self.cells[idx].velocity_y -=
                    (weights[row][2] * (pressure[row] - pressure[neighbors[row][2]]) / h) as f32;
            }
            if i == n {
                let outlet = self.to_index(n + 1, j);
                if !self.cells[outlet].wall {
                    let rho = if ENABLE_WATER {
                        self.local_rho(idx) as f64
                    } else {
                        1.0
                    };
                    self.cells[outlet].velocity_x += (2.0 * pressure[row] / (rho * h)) as f32;
                }
            }
        }
    }

    /// Annule le flux normal sur les deux côtés de chaque cellule solide.
    fn enforce_solid_faces(&mut self) {
        let n = N as usize;
        for i in 1..=n + 1 {
            for j in 1..=n {
                let right = self.to_index(i, j);
                let left = self.to_index(i - 1, j);
                if self.cells[left].wall || self.cells[right].wall {
                    self.cells[right].velocity_x = 0.0;
                }
            }
        }
        for i in 1..=n {
            for j in 1..=n + 1 {
                let top = self.to_index(i, j);
                let bottom = self.to_index(i, j - 1);
                if self.cells[bottom].wall || self.cells[top].wall {
                    self.cells[top].velocity_y = 0.0;
                }
            }
        }
    }


    /// Impose u=0, v=0 in obstacles and walls, constant inflow on the left, outflow (∂/∂x=0) on the right and on the top/bottom
    // Modification de apply_boundary_conditions pour une staggered grid
    /// Profil d'entrée au-dessus d'une hauteur mesurée depuis le bas.
    pub fn inlet_velocity(&self, j: usize, speed: f32) -> f32 {
        if N - j as f32 + 0.5 < self.wind_min_height {
            return 0.0;
        }
        let idx = self.to_index(1, j);
        let air = if ENABLE_WATER {
            1.0 - self.cells[idx].phase.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let profile = if self.realistic_wind {
            wind_profile_factor(j)
        } else {
            1.0
        };
        speed * air * profile
    }

    /// Entrée imposée, haut/bas imperméables à glissement libre.
    /// La vitesse de sortie est déterminée par project(), avec p=0 à droite.
    pub fn apply_boundary_conditions(&mut self, inflow_velocity: f32) {
        let n = N as usize;
        for j in 1..=n {
            self.set_u(1, j, self.inlet_velocity(j, inflow_velocity));
        }
        for i in 1..=n {
            self.set_v(i, 1, 0.0);
            self.set_v(i, n + 1, 0.0);
        }
        self.enforce_solid_faces();
    }

    /// Extrapolate the velocity field at the boundaries
    pub fn extrapolate(&mut self) {
        let n = N as usize;

        // Vertical u extrapolation
        for i in 1..=n {
            let y0 = 0;
            let y1 = 1;
            if let (Some(i0), Some(i1)) = (self.try_index(i, y0), self.try_index(i, y1)) {
                if !self.cells[i0].wall && !self.cells[i1].wall {
                    self.cells[i0].velocity_x = self.cells[i1].velocity_x;
                }
            }

            let y0 = n + 1;
            let y1 = n;
            if let (Some(i0), Some(i1)) = (self.try_index(i, y0), self.try_index(i, y1)) {
                if !self.cells[i0].wall && !self.cells[i1].wall {
                    self.cells[i0].velocity_x = self.cells[i1].velocity_x;
                }
            }
        }

        // Horizontal v extrapolation
        for j in 1..=n {
            let x0 = 0;
            let x1 = 1;
            if let (Some(i0), Some(i1)) = (self.try_index(x0, j), self.try_index(x1, j)) {
                if !self.cells[i0].wall && !self.cells[i1].wall {
                    self.cells[i0].velocity_y = self.cells[i1].velocity_y;
                }
            }

            let x0 = n + 1;
            let x1 = n;
            if let (Some(i0), Some(i1)) = (self.try_index(x0, j), self.try_index(x1, j)) {
                if !self.cells[i0].wall && !self.cells[i1].wall {
                    self.cells[i0].velocity_y = self.cells[i1].velocity_y;
                }
            }
        }
    }




    /// Initialize the grid with a given velocity and density
    pub fn cell_init(&mut self, line: usize, column: usize, vx: f32, vy: f32, density: f32) {
        if line <= (N + 1.0) as usize && column <= (N + 1.0) as usize {
            let idx = self.to_index(line, column);
            if !self.cells[idx].wall {
                self.cells[idx].velocity_x = vx;
                self.cells[idx].velocity_y = vy;
                self.cells[idx].density = density;
            } else {
                println!("Impossible to modify a wall!");
            }
        } else {
            println!("Error: indices out of bounds!");
        }
    }

    /// Initialize the wall character of a cell
    pub fn wall_init(&mut self, line: usize, column: usize, wall: bool) {
        if line <= (N + 1.0) as usize && column <= (N + 1.0) as usize {
            let idx = self.to_index(column, line);
            if !self.cells[idx].wall {
                self.cells[idx].wall = wall;
                if wall {
                    // Un mur vient d'être posé : le cache objets/forces est périmé.
                    self.wall_topology_dirty = true;
                }
            } else {
                println!("Impossible to modify a wall!");
            }
        } else {
            println!("Error: indices out of bounds!");
        }
    }

    /// Initialize the velocity of a cell
    pub fn velocity_init(&mut self, line: usize, column: usize, vx: f32, vy: f32) {
        if line <= (N + 1.0) as usize && column <= (N + 1.0) as usize {
            let idx = self.to_index(line, column);
            if !self.cells[idx].wall {
                self.cells[idx].velocity_x = vx;
                self.cells[idx].velocity_y = vy;
            } else {
                println!("Impossible to modify a wall!");
            }
        } else {
            println!("Error: indices out of bounds!");
        }
    }


    /// Add a circular density and velocity source in the middle of the grid
    pub fn center_source(&mut self, radius: f32, density_value: f32, velocity_magnitude: f32, is_radial: bool) {
        // Process the center of the grid
        let center_x = (N / 2.0) as usize;
        let center_y = (N / 2.0) as usize;

        // Scan every cell in the inscribed square of the circle
        let r_int = radius.floor() as usize;
        let start_i = if center_x > r_int { center_x - r_int } else { 1 };
        let end_i = (center_x + r_int).min(N as usize);
        let start_j = if center_y > r_int { center_y - r_int } else { 1 };
        let end_j = (center_y + r_int).min(N as usize);

        for i in start_i..=end_i {
            for j in start_j..=end_j {
                // Compute the distance from the center
                let dx = i as f32 - center_x as f32;
                let dy = j as f32 - center_y as f32;
                let distance_squared = dx * dx + dy * dy;

                // If the current cell is not a wall
                if distance_squared <= radius * radius {
                    let idx = self.to_index(i, j);
                    if !self.cells[idx].wall {
                        // Add density
                        self.cells[idx].density += density_value;

                        // Compute velocity direction
                        if is_radial {
                            // Initialize radial velocity
                            if distance_squared > 0.0 {
                                let distance = distance_squared.sqrt();
                                let dir_x = dx / distance;
                                let dir_y = dy / distance;

                                // Add velocity in the direction of the center
                                let factor = 1.0 - (distance / radius); // Plus fort au centre
                                self.cells[idx].velocity_x += dir_x * velocity_magnitude * factor;
                                self.cells[idx].velocity_y += dir_y * velocity_magnitude * factor;
                            }
                        } else {
                            // Circular velocity (whirlwind)
                            if distance_squared > 0.0 {
                                let distance = distance_squared.sqrt();
                                let dir_x = -dy / distance;
                                let dir_y = dx / distance;

                                // Velocity increases with distance from the center up to a certain point
                                let factor = (distance / radius) * (1.0 - distance / radius) * 4.0;
                                self.cells[idx].velocity_x += dir_x * velocity_magnitude * factor;
                                self.cells[idx].velocity_y += dir_y * velocity_magnitude * factor;
                            }
                        }
                    }
                }
            }
        }
    }



    /// Print the grid with walls
    pub fn print_grid_walls(&self) {
        let lines: Vec<String> = (0..=(N + 1.0) as usize).into_par_iter().map(|j| {
            let mut line = String::with_capacity(((N + 2.0) as usize) * 3);
            for i in 0..=(N + 1.0) as usize {
                let idx = self.to_index(i, j);
                line.push_str(if self.cells[idx].wall { " █ " } else { " · " });
            }
            line.push('\n');
            line
        }).collect();

        for line in lines {
            print!("{line}");
        }
    }

    /// Print the grid with velocities
    pub fn print_grid_velocity(&self) {
        let lines: Vec<String> = (0..=(N + 1.0) as usize).into_par_iter().map(|j| {
            let mut line = String::with_capacity(((N + 2.0) as usize) * 18);
            for i in 0..=(N + 1.0) as usize {
                let idx = self.to_index(i, j);
                if self.cells[idx].wall {
                    line.push_str("   | █████ |   ");
                } else {
                    let vx = &self.cells[idx].velocity_x;
                    let vy = &self.cells[idx].velocity_y;
                    line.push_str(&format!("|{:.3}, {:.3}| ", vx, vy));
                }
            }
            line.push('\n');
            line
        }).collect();

        for line in lines {
            print!("{line}");
        }
    }

    /// Print the grid with densities
    pub fn print_grid_density(&self) {
        let output: String = (0..=(N + 1.0) as usize).into_par_iter().map(|j| {
            let mut line = String::with_capacity(((N + 2.0) as usize) * 10);
            for i in 0..=(N + 1.0) as usize {
                let idx = self.to_index(i, j);
                if self.cells[idx].wall {
                    line.push_str("   |█████|   ");
                } else {
                    let _ = write!(line, "  |{:5.2}|  ", self.cells[idx].density);
                }
            }
            line.push('\n');
            line
        }).collect();

        print!("{output}");
    }

    /// Compute the total density of the grid
    pub fn total_density(&self) -> f32 {
        self.cells.iter().map(|cell| cell.density).sum()
    }

    /// Create a circle in the grid
    pub fn circle(&mut self, center_x: isize, center_y: isize, radius: f32) {
        let center_x = center_x as usize;
        let center_y = center_y as usize;
        for i in 0..= N as usize{
            for j in 0..= N as usize {
                let dx = i as isize - center_x as isize;
                let dy = j as isize - center_y as isize;
                if dx * dx + dy * dy <= (radius * radius) as isize {
                    self.wall_init(j, i, true);
                }
            }
        }
    }


    // Compatibilité avec les appels historiques : le tunnel est désormais
    // défini par ses conditions aux limites, sans coffre solide.
    pub fn setup_wind_tunnel_walls(&mut self, _hole_positions: &[usize]) {}

    pub fn inject_wind_tunnel_flow(&mut self, density: f32, flow_velocity: f32) {
        self.apply_boundary_conditions(flow_velocity);
        for j in 1..=N as usize {
            let idx = self.to_index(1, j);
            if !self.cells[idx].wall {
                self.cells[idx].density = if self.inlet_velocity(j, flow_velocity) > 0.0 {
                    density
                } else { 0.0 };
            }
        }
    }

    pub fn initialize_wind_tunnel(&mut self, density: f32, flow_velocity: f32, _hole_positions: &[usize]) {
        self.inject_wind_tunnel_flow(density, flow_velocity);
    }


    /// Attempt to create karman vortex formation favorable conditions
    pub fn setup_karman_vortex(&mut self) {
        use crate::conditions::*;
        let radius = (N as usize / 6) as f32 -10.5 ; // Obstacle radius
        let center = ((N as usize + 2) / 6, (N as usize + 2) / 2);

        self.circle(center.0 as isize, center.1 as isize, radius);

        // Horizontal flow
        for j in 1..(N as usize + 1) {
            for i in 1..15 {
                let idx = self.to_index(i, j);
                if !self.cells[idx].wall {
                    self.cells[idx].density = FLOW_DENSITY;
                    self.cells[idx].velocity_x = FLOW_VELOCITY
                }
            }
        }

        println!("✔ Kármán vortex setup complete: central obstacle + inflow.");
    }


    /// Perform a step in the simulation
    pub fn vel_step(&mut self) {
        //self.diffuse(VISCOSITY, DT);
        //println!("Total density after diffuse {:2}", self.total_density());
        if PROJECT == "1"{
            self.project();
        } else if PROJECT == "2"{
            panic!("PROJECT2 non implémenté pour le moment");
        } else {
            panic!("Valeur PROJECT invalide");
        }
        //println!("Total density after project {:2}", self.total_density());
        self.advect_velocity(DT);
        //println!("Total density after advect velocity {:2}", self.total_density());
        self.advect_density(DT);
        //println!("Total density after apres advect density {:2}", self.total_density());
        //self.apply_boundary_conditions();
    }



    /// Perform a step in the simulation with another method
    pub fn vel2_step(&mut self, inflow_velocity: f32) {
        self.apply_boundary_conditions(inflow_velocity);
        self.extrapolate();
        self.advect_velocity(DT);
        if ENABLE_WATER {
            self.apply_gravity(GRAVITY_X, GRAVITY_Y, DT);
            self.clamp_velocity_field(MAX_VELOCITY);
        }
        self.apply_boundary_conditions(inflow_velocity);
        self.project();
        self.extrapolate();
        self.advect_density(DT);
        if ENABLE_WATER { self.advect_phase(DT); }
    }


    /// Perform a step in the simulation with cip-csl4 method (not working)
    pub fn vel_step_cip_csl4(&mut self, inflow_velocity : f32) {
        // Diffusion only if needed
        // self.diffuse(VISCOSITY, DT);

        // Projection to ensure incompressibility
        if PROJECT == "1"{
            self.project();
        } else if PROJECT == "2"{
            panic!("PROJECT2 non implémenté pour le moment");
        } else {
            panic!("Valeur PROJECT invalide");
        }

        // Velocity advection
        self.advect_velocity(DT);

        // Use CIP-CSL4 method over other ones
        //Self::advect_density_cip_csl4(self, DT);

        // Apply boundary conditions
        self.apply_boundary_conditions(inflow_velocity);
    }



}

/// Gradient conjugué préconditionné par la diagonale, en double précision.
/// Le critère d'arrêt porte sur ||b-Ap||_2, avec plafond d'itérations explicite.
fn solve_pressure(
    neighbors: &[[usize; 4]],
    weights: &[[f64; 4]],
    diagonal: &[f64],
    rhs: &[f64],
    max_iterations: usize,
    relative_tolerance: f64,
    absolute_tolerance: f64,
) -> (Vec<f64>, usize, f64, bool) {
    let apply = |x: &[f64], out: &mut [f64]| {
        for row in 0..x.len() {
            out[row] = diagonal[row] * x[row];
            for d in 0..4 {
                out[row] -= weights[row][d] * x[neighbors[row][d]];
            }
        }
    };
    let dot = |a: &[f64], b: &[f64]| -> f64 { a.iter().zip(b).map(|(x, y)| x * y).sum() };
    let mut pressure = vec![0.0; rhs.len()];
    let mut residual = rhs.to_vec();
    let mut z: Vec<f64> = residual
        .iter()
        .zip(diagonal)
        .map(|(r, d)| r / d.max(1e-30))
        .collect();
    let mut direction = z.clone();
    let mut product = vec![0.0; rhs.len()];
    let target = absolute_tolerance.max(relative_tolerance * dot(rhs, rhs).sqrt());
    let mut norm = dot(&residual, &residual).sqrt();
    let mut rz = dot(&residual, &z);
    let mut iterations = 0;
    while norm > target && iterations < max_iterations {
        apply(&direction, &mut product);
        let denominator = dot(&direction, &product);
        if denominator <= 0.0 || !denominator.is_finite() {
            break;
        }
        let alpha = rz / denominator;
        for row in 0..rhs.len() {
            pressure[row] += alpha * direction[row];
            residual[row] -= alpha * product[row];
        }
        iterations += 1;
        norm = dot(&residual, &residual).sqrt();
        // Vérifier le résidu réel avant d'annoncer la convergence.
        if norm <= target {
            apply(&pressure, &mut product);
            for row in 0..rhs.len() {
                residual[row] = rhs[row] - product[row];
            }
            norm = dot(&residual, &residual).sqrt();
            if norm <= target {
                break;
            }
            for row in 0..rhs.len() {
                direction[row] = residual[row] / diagonal[row].max(1e-30);
            }
            rz = dot(&residual, &direction);
            continue;
        }
        for row in 0..rhs.len() {
            z[row] = residual[row] / diagonal[row].max(1e-30);
        }
        let next_rz = dot(&residual, &z);
        let beta = next_rz / rz;
        for row in 0..rhs.len() {
            direction[row] = z[row] + beta * direction[row];
        }
        rz = next_rz;
    }
    apply(&pressure, &mut product);
    norm = rhs
        .iter()
        .zip(&product)
        .map(|(b, a)| (b - a) * (b - a))
        .sum::<f64>()
        .sqrt();
    (pressure, iterations, norm, norm <= target)
}

#[cfg(test)]
mod projection_tests {
    use super::*;

    fn divergence(grid: &Grid) -> f32 {
        let mut maximum = 0.0f32;
        for i in 1..=N as usize {
            for j in 1..=N as usize {
                if !grid.cells[grid.to_index(i, j)].wall {
                    let value = N
                        * (grid.get_u(i + 1, j) - grid.get_u(i, j) + grid.get_v(i, j + 1)
                            - grid.get_v(i, j));
                    maximum = maximum.max(value.abs());
                }
            }
        }
        maximum
    }

    #[test]
    fn inlet_controls_height_and_profile() {
        let mut grid = Grid::new();
        grid.realistic_wind = false;
        grid.wind_min_height = N * 0.5;
        assert_eq!(grid.inlet_velocity(1, 2.0), 2.0);
        assert_eq!(grid.inlet_velocity(N as usize, 2.0), 0.0);
        grid.realistic_wind = true;
        assert_eq!(grid.inlet_velocity(1, 2.0), 2.0 * wind_profile_factor(1));
        grid.wind_min_height = N;
        assert_eq!(grid.inlet_velocity(1, 2.0), 0.0);
    }

    #[test]
    fn projection_balances_inlet_and_preserves_solid_faces() {
        let mut grid = Grid::new();
        grid.realistic_wind = false;
        let n = N as usize;
        grid.square((n / 2) as isize, (n / 2) as isize, (n / 5) as f32);
        grid.apply_boundary_conditions(1.0);
        grid.project();
        assert!(
            grid.projection_converged,
            "residual {}",
            grid.projection_residual
        );
        assert!(divergence(&grid) < 0.02, "divergence {}", divergence(&grid));
        let incoming: f64 = (1..=n).map(|j| grid.get_u(1, j) as f64).sum();
        let outgoing: f64 = (1..=n).map(|j| grid.get_u(n + 1, j) as f64).sum();
        assert!((incoming - outgoing).abs() < incoming * 1e-4);
        let original = grid.cells.clone();
        for _ in 0..3 {
            grid.project();
        }
        let change = grid
            .cells
            .iter()
            .zip(&original)
            .map(|(a, b)| {
                (a.velocity_x - b.velocity_x)
                    .abs()
                    .max((a.velocity_y - b.velocity_y).abs())
            })
            .fold(0.0f32, f32::max);
        assert!(change < 0.002, "repeated projection change {change}");
        for i in 1..=n {
            for j in 1..=n {
                if grid.cells[grid.to_index(i, j)].wall {
                    assert_eq!(grid.get_u(i, j), 0.0);
                    assert_eq!(grid.get_u(i + 1, j), 0.0);
                    assert_eq!(grid.get_v(i, j), 0.0);
                    assert_eq!(grid.get_v(i, j + 1), 0.0);
                }
            }
        }
    }

    #[test]
    fn uniform_tunnel_survives_full_time_steps() {
        let mut grid = Grid::new();
        grid.realistic_wind = false;
        for cell in &mut grid.cells {
            cell.velocity_x = 1.0;
        }
        for _ in 0..3 {
            grid.vel2_step(1.0);
        }
        assert!(grid.projection_converged);
        assert!(divergence(&grid) < 0.02);
        for i in 1..=N as usize {
            for j in 1..=N as usize {
                assert!((grid.get_u(i, j) - 1.0).abs() < 0.002);
                assert!(grid.get_v(i, j).abs() < 0.002);
            }
        }
    }

    #[test]
    fn incompatible_neumann_system_reports_failure() {
        let (_, _, residual, converged) =
            solve_pressure(&[[0; 4]], &[[0.0; 4]], &[0.0], &[1.0], 20, 1e-5, 1e-10);
        assert!(!converged);
        assert_eq!(residual, 1.0);
    }
}
