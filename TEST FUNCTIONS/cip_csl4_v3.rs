use crate::grid::{Grid};
use crate::conditions::*;
use crate::grid::*;
use rayon::prelude::*;

/// Effectue une étape d'advection CIP-CSL4 de la densité sur la grille.
///
/// Cette méthode conserve la masse transportée.
/// Elle est explicitement conservative et utilise un polynôme bicubique de degré 3.
pub fn advect_cip_csl4(grid: &mut Grid, dt: f32) {
    let n = (N + 2.0) as usize;
    let mut new_density = vec![0.0; grid.cells.len()];

    // Pour chaque cellule non mur
    (0..grid.cells.len()).into_par_iter().for_each(|idx| {
        let (i, j) = grid.decode_index(idx);

        if grid.cells[idx].wall {
            new_density[idx] = 0.0;
            return;
        }

        let vel = grid.cells[idx].velocity;
        let back_x = i as f32 - vel.x * dt;
        let back_y = j as f32 - vel.y * dt;

        // Coordonnées flottantes en arrière (en remontant le flot)
        let i0 = back_x.floor() as isize;
        let j0 = back_y.floor() as isize;

        // Borne de sécurité
        if i0 < 1 || j0 < 1 || i0 + 2 >= n as isize || j0 + 2 >= n as isize {
            new_density[idx] = grid.cells[idx].density;
            return;
        }

        let dx = back_x - i0 as f32;
        let dy = back_y - j0 as f32;

        // Récupération des 16 valeurs autour de la position arrière
        let mut d = [[0.0f32; 4]; 4];
        for dj in 0..4 {
            for di in 0..4 {
                let ni = (i0 + di as isize - 1) as usize;
                let nj = (j0 + dj as isize - 1) as usize;
                let neighbor_idx = grid.to_index(ni, nj);
                d[dj][di] = if grid.cells[neighbor_idx].wall {
                    0.0
                } else {
                    grid.cells[neighbor_idx].density
                };
            }
        }

        // Interpolation bicubique (tensorielle)
        let px = cubic_interp_weights(dx);
        let py = cubic_interp_weights(dy);

        let mut interpolated_density = 0.0;
        for j in 0..4 {
            for i in 0..4 {
                interpolated_density += d[j][i] * px[i] * py[j];
            }
        }

        new_density[idx] = interpolated_density;
    });

    // Mise à jour finale (conservative)
    for idx in 0..grid.cells.len() {
        if !grid.cells[idx].wall {
            grid.cells[idx].density = new_density[idx].max(0.0);
        }
    }
}

/// Renvoie les poids de Lagrange cubic (ou spline) pour x appartenant à [0, 1].
fn cubic_interp_weights(x: f32) -> [f32; 4] {
    let x2 = x * x;
    let x3 = x2 * x;
    [
        -0.5 * x3 + x2 - 0.5 * x,
        1.5 * x3 - 2.5 * x2 + 1.0,
        -1.5 * x3 + 2.0 * x2 + 0.5 * x,
        0.5 * x3 - 0.5 * x2,
    ]
}
