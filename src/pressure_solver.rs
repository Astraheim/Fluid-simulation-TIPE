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
// Packed sparse rows reduce allocation indirection in repeated matrix products.
struct Csr {
    offsets: Vec<usize>,
    columns: Vec<usize>,
    weights: Vec<f64>,
}
impl Csr {
    fn build(rows: &[Row], filter: impl Fn(usize, usize, f64) -> Option<f64>) -> Self {
        let mut out=Self{offsets:vec![0],columns:Vec::with_capacity(rows.len()*4),weights:Vec::with_capacity(rows.len()*4)};
        for (i,row) in rows.iter().enumerate() {
            for &(j,w) in &row.neighbors {
                if let Some(v)=filter(i,j,w) {out.columns.push(j);out.weights.push(v);}
            }
            out.offsets.push(out.columns.len());
        }
        out
    }
    fn sum(&self, i:usize, x:&[f64]) -> f64 {
        (self.offsets[i]..self.offsets[i+1]).map(|k|self.weights[k]*x[self.columns[k]]).sum()
    }
    fn apply(&self, diagonal:&[f64], x:&[f64], out:&mut[f64]) {
        for i in 0..out.len() {out[i]=diagonal[i]*x[i]-self.sum(i,x);}
    }
}
#[cfg(test)]
fn apply(rows:&[Row],x:&[f64],out:&mut[f64]) {
    for (i,row) in rows.iter().enumerate() {out[i]=row.neighbors.iter().map(|&(j,w)|w*(x[i]-x[j])).sum();}
}
// Modified incomplete Cholesky, zero fill, for the Cartesian five-point graph.
// With no triangles, lower-factor entries remain -w/Ljj. Fold a fraction of
// dropped fill into the diagonal (tau=0.97), retaining positive pivots.
struct Mic {
    inverse: Vec<f64>,
    lower: Csr,
    upper: Csr,
}
impl Mic {
    fn new(rows: &[Row]) -> Self {
        let mut inverse=vec![0.0;rows.len()];
        for (i,row) in rows.iter().enumerate() {
            let diagonal: f64=row.neighbors.iter().map(|v|v.1).sum();
            if diagonal==0.0 {inverse[i]=1.0;continue;}
            let mut pivot=diagonal;
            for &(j,w) in &row.neighbors {
                if j>=i {continue;}
                let other_upper:f64=rows[j].neighbors.iter().filter(|&&(k,_)|k>j && k!=i).map(|&(_,w)|w).sum();
                pivot-=(w*w+0.97*w*other_upper)*inverse[j]*inverse[j];
            }
            if !pivot.is_finite() || pivot<0.25*diagonal {pivot=diagonal;}
            inverse[i]=1.0/pivot.sqrt();
        }
        Self {
            lower:Csr::build(rows,|i,j,w|if j<i {Some(w*inverse[j])}else{None}),
            upper:Csr::build(rows,|i,j,w|if j>i {Some(w*inverse[i])}else{None}),
            inverse,
        }
    }
    fn apply(&self,rhs:&[f64],out:&mut[f64]) {
        for i in 0..rhs.len() {out[i]=(rhs[i]+self.lower.sum(i,out))*self.inverse[i];}
        for i in (0..rhs.len()).rev() {out[i]=(out[i]+self.upper.sum(i,out))*self.inverse[i];}
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
    let matrix=Csr::build(rows,|_,_,w|Some(w));
    let preconditioner=Mic::new(rows);
    let mut ax = vec![0.0; n];
    matrix.apply(&diagonal, &x, &mut ax);
    let mut r: Vec<f64> = b.iter().zip(&ax).map(|(b, a)| b - a).collect();
    let mut z=vec![0.0;n];
    preconditioner.apply(&r,&mut z);
    let mut p = z.clone();
    let mut rz = dot(&r, &z);
    let mut iterations = 0;
    while iterations < max_iterations && norm(&r) > tolerance {
        matrix.apply(&diagonal, &p, &mut ax);
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
            matrix.apply(&diagonal, &x, &mut ax);
            for i in 0..n {
                r[i] = b[i] - ax[i];
            }
        }
        preconditioner.apply(&r,&mut z);
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

#[cfg(test)] mod mic_regression {
    use super::*;
    #[test]
    fn weighted_grid_with_obstacle_matches_manufactured_solution() {
        let side=20;
        let blocked=|i:usize| (8..12).contains(&(i/side)) && (7..13).contains(&(i%side));
        let mut rows=vec![Row{neighbors:Vec::new()};side*side];
        for i in 0..rows.len() {
            if blocked(i) {continue;}
            for j in [i.checked_sub(side),if i+side<rows.len(){Some(i+side)}else{None},if i%side>0{Some(i-1)}else{None},if i%side+1<side{Some(i+1)}else{None}].into_iter().flatten() {
                if !blocked(j) {
                    let weight=0.2+((i.min(j)*7+i.max(j)*3)%17) as f64/5.0;
                    rows[i].neighbors.push((j,weight));
                }
            }
        }
        let expected:Vec<f64>=(0..rows.len()).map(|i|(i as f64*0.17).sin()).collect();
        let mut rhs=vec![0.0;rows.len()];apply(&rows,&expected,&mut rhs);
        let solution=solve(&rows,&rhs,vec![0.0;rows.len()],1e-9,1000);
        let mut actual=vec![0.0;rows.len()];apply(&rows,&solution.pressure,&mut actual);
        assert!(solution.incompatible_rhs<1e-12);
        assert!(rhs.iter().zip(actual).all(|(a,b)|(a-b).abs()<1e-9));
    }
    #[test]
    fn preconditioner_is_symmetric_positive_on_weighted_chain() {
        let rows=vec![Row{neighbors:vec![(1,2.0)]},Row{neighbors:vec![(0,2.0),(2,0.3)]},Row{neighbors:vec![(1,0.3)]}];
        let m=Mic::new(&rows);let a=[1.0,-2.0,3.0];let b=[-4.0,2.0,1.0];
        let mut ma=[0.0;3];let mut mb=[0.0;3];m.apply(&a,&mut ma);m.apply(&b,&mut mb);
        assert!(dot(&a,&ma)>0.0);
        assert!((dot(&a,&mb)-dot(&b,&ma)).abs()<1e-10);
    }
}
