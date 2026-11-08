#![allow(dead_code)]
#[path="../../src/conditions.rs"] mod conditions;
#[path="../../src/grid.rs"] mod grid;
#[path="../../src/grid2.rs"] mod grid2;
#[path="../../src/ship.rs"] mod ship;
#[path="../../src/water.rs"] mod water;
#[path="../../src/objects.rs"] mod objects;
#[path="../../src/pressure_computation.rs"] mod pressure_computation;
#[path="../../src/pressure_computation2.rs"] mod pressure_computation2;
#[path="../../src/cargo_safety.rs"] mod cargo_safety;
#[path="../../src/experiments.rs"] mod experiments;
#[cfg(test)] mod replay_regression {
use super::*;
#[test] fn saved_boat_start() {
 let mut c=experiments::default_campaign().boats.remove(0);
 c.warmup=0; c.measure=2;
 let c=experiments::Config::parse(&c.text().unwrap()).unwrap();
 let (g,l)=c.grid().unwrap();
 let mut recaptured=experiments::Config::capture(&g,&l);
 recaptured.warmup=0;recaptured.measure=2;
 let root=std::env::temp_dir().join("tipe-crash-repro");
 experiments::run(recaptured,&root).unwrap();
}
}

#[path="../../src/pressure_solver.rs"] mod pressure_solver;
#[cfg(test)] mod pressure_smoke {
use super::*;
#[test] fn full_boat_projection() {
 let c=experiments::default_campaign().boats.remove(0);let (mut g,_)=c.grid().unwrap();
 experiments::inject(&mut g,c.density,1.0);
 let t=std::time::Instant::now();g.vel2_step(1.0);
 println!("BOAT before={} after={} final={} iterations={} converged={} incompatible_floor={} ms={}",g.divergence_before_projection,g.divergence_after_projection,g.divergence_end_step,g.projection_iterations,g.projection_converged,g.projection_incompatible_divergence,t.elapsed().as_secs_f64()*1000.0);
 assert!(g.projection_converged);
 assert!(g.divergence_after_projection<g.divergence_before_projection);
 assert_eq!(g.divergence_after_projection,g.divergence_end_step);
}
}

// Same workload for baseline and optimized solver. No rendering, force recording
// or geometry generation is included in the timed numerical step.
#[cfg(test)] mod speed_benchmark {
    use super::*;
    #[test]
    #[ignore = "manual release-mode benchmark"]
    fn benchmark_boat_steps() {
        for boat in [0,5] {
            let c=experiments::default_campaign().boats.remove(boat);
            for repeat in 0..3 {
                let (mut g,_)=c.grid().unwrap();
                for step in 0..3 {
                    experiments::inject(&mut g,c.density,1.0);
                    let t=std::time::Instant::now();
                    g.vel2_step(1.0);
                    println!("BENCH,{boat},{repeat},{step},{:.6},{},{:.9},{:.9},{}",
                        t.elapsed().as_secs_f64()*1000.0,g.projection_iterations,
                        g.divergence_before_projection,g.divergence_after_projection,g.projection_converged);
                }
            }
        }
    }
}
