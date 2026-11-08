//! Experimental preconditioned conjugate gradient for a symmetric Neumann graph.
//! Closed components need a zero-sum RHS. Report the removed component means;
//! never mistake convergence of the compatible part for physical convergence.
#[derive(Clone)]
pub struct Row {
    pub neighbors: Vec<(usize, f64)>,
}
pub struct Solution {
    pub pressure: Vec<f64>,
    pub iterations: usize,
    pub incompatible_rhs: f64,
}
fn apply(rows: &[Row], x: &[f64], out: &mut [f64]) {
    for (i, row) in rows.iter().enumerate() {
        out[i] = row.neighbors.iter().map(|&(j, w)| w * (x[i] - x[j])).sum();
    }
}
fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn norm(a: &[f64]) -> f64 {
    a.iter().fold(0.0, |m, x| {
        if x.is_finite() {
            m.max(x.abs())
        } else {
            f64::INFINITY
        }
    })
}
pub fn solve(
    rows: &[Row],
    rhs: &[f64],
    mut x: Vec<f64>,
    tolerance: f64,
    max_iterations: usize,
) -> Solution {
    let n = rows.len();
    let mut b = rhs.to_vec();
    let mut seen = vec![false; n];
    let mut incompatible: f64 = 0.0;
    for seed in 0..n {
        if seen[seed] {
            continue;
        }
        let mut component = vec![seed];
        seen[seed] = true;
        let mut k = 0;
        while k < component.len() {
            for &(j, _) in &rows[component[k]].neighbors {
                if !seen[j] {
                    seen[j] = true;
                    component.push(j);
                }
            }
            k += 1;
        }
        let mean_b = component.iter().map(|&i| b[i]).sum::<f64>() / component.len() as f64;
        let mean_x = component.iter().map(|&i| x[i]).sum::<f64>() / component.len() as f64;
        incompatible = incompatible.max(mean_b.abs());
        for i in component {
            b[i] -= mean_b;
            x[i] -= mean_x;
        }
    }
    let diagonal: Vec<f64> = rows
        .iter()
        .map(|r| r.neighbors.iter().map(|v| v.1).sum::<f64>().max(1e-30))
        .collect();
    let mut ax = vec![0.0; n];
    apply(rows, &x, &mut ax);
    let mut r: Vec<f64> = b.iter().zip(&ax).map(|(b, a)| b - a).collect();
    let mut z: Vec<f64> = r.iter().zip(&diagonal).map(|(r, d)| r / d).collect();
    let mut p = z.clone();
    let mut rz = dot(&r, &z);
    let mut iterations = 0;
    while iterations < max_iterations && norm(&r) > tolerance {
        apply(rows, &p, &mut ax);
        let pap = dot(&p, &ax);
        if !pap.is_finite() || pap <= 0.0 || !rz.is_finite() {
            break;
        }
        let alpha = rz / pap;
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ax[i];
        }
        iterations += 1;
        // Verify the true residual periodically; restart only when a tentative
        // convergence was not confirmed, preserving long-wave CG progress.
        let restart = norm(&r) <= tolerance;
        if iterations % 50 == 0 || restart {
            apply(rows, &x, &mut ax);
            for i in 0..n {
                r[i] = b[i] - ax[i];
            }
        }
        for i in 0..n {
            z[i] = r[i] / diagonal[i];
        }
        let next = dot(&r, &z);
        let beta = if restart || rz == 0.0 { 0.0 } else { next / rz };
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
        rz = next;
    }
    Solution {
        pressure: x,
        iterations,
        incompatible_rhs: incompatible,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disconnected_components_report_incompatible_flux() {
        let rows = vec![
            Row {
                neighbors: vec![(1, 1.0)],
            },
            Row {
                neighbors: vec![(0, 1.0)],
            },
            Row { neighbors: vec![] },
        ];
        let s = solve(&rows, &[2.0, 0.0, 3.0], vec![0.0; 3], 1e-10, 100);
        assert_eq!(s.incompatible_rhs, 3.0);
        assert!((s.pressure[0] - s.pressure[1] - 1.0).abs() < 1e-10);
    }
    #[test]
    fn compatible_weighted_graph_converges() {
        let rows = vec![
            Row {
                neighbors: vec![(1, 2.0)],
            },
            Row {
                neighbors: vec![(0, 2.0), (2, 0.3)],
            },
            Row {
                neighbors: vec![(1, 0.3)],
            },
        ];
        let expected = [1.0, 3.0, -2.0];
        let mut b = vec![0.0; 3];
        apply(&rows, &expected, &mut b);
        let s = solve(&rows, &b, vec![0.0; 3], 1e-10, 100);
        let mut ap = vec![0.0; 3];
        apply(&rows, &s.pressure, &mut ap);
        assert!(b.iter().zip(ap).all(|(b, a)| (b - a).abs() < 1e-10));
        assert!(s.incompatible_rhs < 1e-12);
    }
}
