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
    pub pressure: f32,
    pub density: f32,
    pub density_x: f32,
    pub density_y: f32,
    pub density_xx: f32,
    pub density_yy: f32,
    pub density_xy: f32,
    pub wall: bool,
}

#[derive(Clone, Debug)]
pub struct Grid {
    pub cells: Vec<Cell>,
    // Grilles décalées pour les composantes de vitesse
    pub u: Vec<f32>, // Vitesse horizontale (sur les faces verticales) - dimension (N+1) x N
    pub v: Vec<f32>, // Vitesse verticale (sur les faces horizontales) - dimension N x (N+1)
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

    /// Exactly the same as magnitude, returns vector magnitude
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
    /// Conversion des indices en indices pour les grilles décalées
    /// u_idx pour les vitesses horizontales (dimension (N+1) x N)
    pub fn u_idx(&self, i: usize, j: usize) -> usize {
        i + j * (N as usize + 1)
    }

    /// v_idx pour les vitesses verticales (dimension N x (N+1))
    pub fn v_idx(&self, i: usize, j: usize) -> usize {
        i * (N as usize + 1) + j
    }

    /// Check if the coordinates (i, j) are within the grid bounds
    pub fn in_bounds(&self, i: usize, j: usize) -> bool {
        i >= 1 && i <= N as usize && j >= 1 && j <= N as usize
    }

    /// Check if the coordinates are within the u-grid bounds
    pub fn in_bounds_u(&self, i: usize, j: usize) -> bool {
        i >= 0 && i <= N as usize && j >= 1 && j <= N as usize
    }

    /// Check if the coordinates are within the v-grid bounds
    pub fn in_bounds_v(&self, i: usize, j: usize) -> bool {
        i >= 1 && i <= N as usize && j >= 0 && j <= N as usize
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
        let n = N as usize;
        let mut grid = Self {
            cells: vec![Cell::default(); SIZE as usize],
            u: vec![0.0; (n + 1) * n], // (N+1) × N
            v: vec![0.0; n * (n + 1)], // N × (N+1)
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
        let a = dt * diff * (N * N);
        let mut new_density = vec![0.0; self.cells.len()];

        for _ in 0..20 {
            new_density
                .par_iter_mut()
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

    /// Diffusion des composantes de vitesse
    pub fn diffuse_velocity(&mut self, viscosity: f32, dt: f32) {
        let a = dt * viscosity * (N * N);
        let n = N as usize;

        // Diffusion de u
        let mut new_u = self.u.clone();
        for _ in 0..20 {
            for j in 1..=n {
                for i in 1..n {
                    let mut sum_u = 0.0;
                    let mut count_u = 0;

                    // Vérification des cellules adjacentes
                    for &di in &[-1, 1] {
                        let ni = (i as isize + di) as usize;
                        if ni >= 1 && ni < n {
                            sum_u += self.u[self.u_idx(ni, j)];
                            count_u += 1;
                        }
                    }

                    for &dj in &[-1, 1] {
                        let nj = (j as isize + dj) as usize;
                        if nj >= 1 && nj <= n {
                            sum_u += self.u[self.u_idx(i, nj)];
                            count_u += 1;
                        }
                    }

                    let idx_u = self.u_idx(i, j);
                    new_u[idx_u] = (self.u[idx_u] + a * sum_u) / (1.0 + a * count_u as f32);
                }
            }
            self.u.copy_from_slice(&new_u);
        }

        // Diffusion de v
        let mut new_v = self.v.clone();
        for _ in 0..20 {
            for j in 1..n {
                for i in 1..=n {
                    let mut sum_v = 0.0;
                    let mut count_v = 0;

                    // Vérification des cellules adjacentes
                    for &di in &[-1, 1] {
                        let ni = (i as isize + di) as usize;
                        if ni >= 1 && ni <= n {
                            sum_v += self.v[self.v_idx(ni, j)];
                            count_v += 1;
                        }
                    }

                    for &dj in &[-1, 1] {
                        let nj = (j as isize + dj) as usize;
                        if nj >= 1 && nj < n {
                            sum_v += self.v[self.v_idx(i, nj)];
                            count_v += 1;
                        }
                    }

                    let idx_v = self.v_idx(i, j);
                    new_v[idx_v] = (self.v[idx_v] + a * sum_v) / (1.0 + a * count_v as f32);
                }
            }
            self.v.copy_from_slice(&new_v);
        }
    }

    /// Advection de la vitesse horizontale u
    pub fn advect_u(&mut self, dt: f32) {
        let dt0 = dt * N;
        let n = N as usize;
        let mut new_u = vec![0.0; self.u.len()];

        // Pour chaque face verticale où u est défini
        for j in 1..=n {
            for i in 0..=n {
                // Vérifier si la face est associée à un mur
                let is_wall = (i > 0 && self.cells[self.to_index(i, j)].wall) ||
                    (i < n && self.cells[self.to_index(i+1, j)].wall);

                if is_wall {
                    new_u[self.u_idx(i, j)] = 0.0; // Pas de vitesse aux murs
                    continue;
                }

                // Position actuelle de la face
                let pos_x = i as f32;
                let pos_y = j as f32 - 0.5; // Centre de la face verticale

                // Interpolation des vitesses au point
                let u_here = self.u[self.u_idx(i, j)];
                let v_here = self.interpolate_v(pos_x, pos_y);

                // Rétroprojection
                let back_x = pos_x - dt0 * u_here;
                let back_y = pos_y - dt0 * v_here;

                // Limites pour rester dans la grille
                let back_x = back_x.clamp(0.0, N);
                let back_y = back_y.clamp(0.5, N + 0.5);

                // Interpolation bilinéaire pour trouver u au point rétropropagé
                new_u[self.u_idx(i, j)] = self.interpolate_u(back_x, back_y);
            }
        }

        self.u = new_u;
    }

    /// Advection de la vitesse verticale v
    pub fn advect_v(&mut self, dt: f32) {
        let dt0 = dt * N;
        let n = N as usize;
        let mut new_v = vec![0.0; self.v.len()];

        // Pour chaque face horizontale où v est défini
        for j in 0..=n {
            for i in 1..=n {
                // Vérifier si la face est associée à un mur
                let is_wall = (j > 0 && self.cells[self.to_index(i, j)].wall) ||
                    (j < n && self.cells[self.to_index(i, j+1)].wall);

                if is_wall {
                    new_v[self.v_idx(i, j)] = 0.0; // Pas de vitesse aux murs
                    continue;
                }

                // Position actuelle de la face
                let pos_x = i as f32 - 0.5; // Centre de la face horizontale
                let pos_y = j as f32;

                // Interpolation des vitesses au point
                let u_here = self.interpolate_u(pos_x, pos_y);
                let v_here = self.v[self.v_idx(i, j)];

                // Rétroprojection
                let back_x = pos_x - dt0 * u_here;
                let back_y = pos_y - dt0 * v_here;

                // Limites pour rester dans la grille
                let back_x = back_x.clamp(0.5, N + 0.5);
                let back_y = back_y.clamp(0.0, N);

                // Interpolation bilinéaire pour trouver v au point rétropropagé
                new_v[self.v_idx(i, j)] = self.interpolate_v(back_x, back_y);
            }
        }

        self.v = new_v;
    }

    /// Interpolation bilinéaire pour u
    fn interpolate_u(&self, x: f32, y: f32) -> f32 {
        let n = N as usize;

        // Si hors limites, retourner 0
        if x < 0.0 || x > N || y < 0.5 || y > N + 0.5 {
            return 0.0;
        }

        // Trouver les indices des cellules environnantes
        let i0 = x.floor() as usize;
        let i1 = (i0 + 1).min(n);
        let j0 = (y - 0.5).floor() as usize;
        let j1 = (j0 + 1).min(n);

        // Calculer les poids pour l'interpolation
        let sx = x - i0 as f32;
        let sy = y - 0.5 - j0 as f32;

        // Récupérer les valeurs de u aux points environnants
        let u00 = if self.in_bounds_u(i0, j0 + 1) { self.u[self.u_idx(i0, j0 + 1)] } else { 0.0 };
        let u10 = if self.in_bounds_u(i1, j0 + 1) { self.u[self.u_idx(i1, j0 + 1)] } else { 0.0 };
        let u01 = if self.in_bounds_u(i0, j1 + 1) { self.u[self.u_idx(i0, j1 + 1)] } else { 0.0 };
        let u11 = if self.in_bounds_u(i1, j1 + 1) { self.u[self.u_idx(i1, j1 + 1)] } else { 0.0 };

        // Interpolation bilinéaire
        (1.0 - sx) * (1.0 - sy) * u00 +
            sx * (1.0 - sy) * u10 +
            (1.0 - sx) * sy * u01 +
            sx * sy * u11
    }

    /// Interpolation bilinéaire pour v
    fn interpolate_v(&self, x: f32, y: f32) -> f32 {
        let n = N as usize;

        // Si hors limites, retourner 0
        if x < 0.5 || x > N + 0.5 || y < 0.0 || y > N {
            return 0.0;
        }

        // Trouver les indices des cellules environnantes
        let i0 = (x - 0.5).floor() as usize;
        let i1 = (i0 + 1).min(n);
        let j0 = y.floor() as usize;
        let j1 = (j0 + 1).min(n);

        // Calculer les poids pour l'interpolation
        let sx = x - 0.5 - i0 as f32;
        let sy = y - j0 as f32;

        // Récupérer les valeurs de v aux points environnants
        let v00 = if self.in_bounds_v(i0 + 1, j0) { self.v[self.v_idx(i0 + 1, j0)] } else { 0.0 };
        let v10 = if self.in_bounds_v(i1 + 1, j0) { self.v[self.v_idx(i1 + 1, j0)] } else { 0.0 };
        let v01 = if self.in_bounds_v(i0 + 1, j1) { self.v[self.v_idx(i0 + 1, j1)] } else { 0.0 };
        let v11 = if self.in_bounds_v(i1 + 1, j1) { self.v[self.v_idx(i1 + 1, j1)] } else { 0.0 };

        // Interpolation bilinéaire
        (1.0 - sx) * (1.0 - sy) * v00 +
            sx * (1.0 - sy) * v10 +
            (1.0 - sx) * sy * v01 +
            sx * sy * v11
    }

    /// Project the velocity field to ensure incompressibility
    pub fn project(&mut self) {
        let n = N as usize;
        let h = 1.0 / N;

        // Initialiser le champ de divergence et de pression
        let mut div = vec![0.0; SIZE as usize];
        let mut p = vec![0.0; SIZE as usize];

        // Calculer la divergence pour chaque cellule
        for j in 1..=n {
            for i in 1..=n {
                if self.cells[self.to_index(i, j)].wall {
                    continue;
                }

                // Calculer la divergence = (u_right - u_left + v_top - v_bottom) / h
                let u_right = self.u[self.u_idx(i, j)];
                let u_left = self.u[self.u_idx(i-1, j)];
                let v_top = self.v[self.v_idx(i, j)];
                let v_bottom = self.v[self.v_idx(i, j-1)];

                div[self.to_index(i, j)] = (u_right - u_left + v_top - v_bottom) / h;
            }
        }

        // Résoudre l'équation de Poisson pour la pression
        let tolerance = 1e-5;
        for _ in 0..20 {
            let mut max_error:f32 = 0.0;

            for j in 1..=n {
                for i in 1..=n {
                    if self.cells[self.to_index(i, j)].wall {
                        continue;
                    }

                    let mut sum_p = 0.0;
                    let mut count = 0;

                    // Pression des cellules adjacentes
                    for &(di, dj) in &[(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let ni = (i as isize + di) as usize;
                        let nj = (j as isize + dj) as usize;

                        if ni >= 1 && ni <= n && nj >= 1 && nj <= n {
                            let n_idx = self.to_index(ni, nj);
                            if !self.cells[n_idx].wall {
                                sum_p += p[n_idx];
                                count += 1;
                            }
                        }
                    }

                    let idx = self.to_index(i, j);
                    let p_new = (sum_p - h * h * div[idx]) / count as f32;
                    let error = (p_new - p[idx]).abs();
                    max_error = max_error.max(error);
                    p[idx] = p_new;
                }
            }

            if max_error < tolerance {
                break;
            }
        }

        // Mettre à jour les vitesses en fonction du gradient de pression
        for j in 1..=n {
            for i in 0..=n {
                if i == 0 || i == n || self.cells[self.to_index(i.min(n), j)].wall || (i > 0 && self.cells[self.to_index(i, j)].wall) {
                    continue;
                }

                // Gradient horizontal de pression
                let p_right = if i < n { p[self.to_index(i+1, j)] } else { p[self.to_index(i, j)] };
                let p_left = if i > 0 { p[self.to_index(i, j)] } else { p[self.to_index(1, j)] };

                // Mettre à jour u
                self.u[self.u_idx(i, j)] -= (p_right - p_left) / h;
            }
        }

        for j in 0..=n {
            for i in 1..=n {
                if j == 0 || j == n || self.cells[self.to_index(i, j.min(n))].wall || (j > 0 && self.cells[self.to_index(i, j)].wall) {
                    continue;
                }

                // Gradient vertical de pression
                let p_top = if j < n { p[self.to_index(i, j+1)] } else { p[self.to_index(i, j)] };
                let p_bottom = if j > 0 { p[self.to_index(i, j)] } else { p[self.to_index(i, 1)] };

                // Mettre à jour v
                self.v[self.v_idx(i, j)] -= (p_top - p_bottom) / h;
            }
        }

        // Mettre à jour la pression dans les cellules
        for (i, pressure) in self.cells.iter_mut().zip(p.iter()) {
            i.pressure = *pressure;
        }
    }

    /// Advect density in the grid
    pub fn advect_density(&mut self, dt: f32) {
        let dt0 = dt * N;
        let n = N as usize;
        let mut new_density = vec![0.0; self.cells.len()];

        for j in 1..=n {
            for i in 1..=n {
                let idx = self.to_index(i, j);
                if self.cells[idx].wall {
                    new_density[idx] = self.cells[idx].density;
                    continue;
                }

                // Position actuelle
                let pos_x = i as f32;
                let pos_y = j as f32;

                // Interpoler les vitesses au centre de la cellule
                let u_here = 0.5 * (self.u[self.u_idx(i-1, j)] + self.u[self.u_idx(i, j)]);
                let v_here = 0.5 * (self.v[self.v_idx(i, j-1)] + self.v[self.v_idx(i, j)]);

                // Rétroprojection
                let back_x = pos_x - dt0 * u_here;
                let back_y = pos_y - dt0 * v_here;

                // Limites pour rester dans la grille
                let back_x = back_x.clamp(0.5, N + 0.5);
                let back_y = back_y.clamp(0.5, N + 0.5);

                // Indices et poids pour l'interpolation bilinéaire
                let i0 = back_x.floor() as usize;
                let i1 = (i0 + 1).min(n);
                let j0 = back_y.floor() as usize;
                let j1 = (j0 + 1).min(n);

                let s1 = back_x - i0 as f32;
                let s0 = 1.0 - s1;
                let t1 = back_y - j0 as f32;
                let t0 = 1.0 - t1;

                // Fonction pour obtenir la densité avec réflexion aux murs
                let get_density = |i: usize, j: usize| -> f32 {
                    let idx = self.to_index(i, j);
                    if idx < self.cells.len() && !self.cells[idx].wall {
                        self.cells[idx].density
                    } else {
                        // Réflexion : utiliser la densité de la cellule actuelle
                        self.cells[self.to_index(i, j)].density
                    }};}}}}