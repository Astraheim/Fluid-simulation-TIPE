// Last update to rendu_code_tex: 2025-05-02
// last modif: 2025-05-09 (+ ajout module eau)

/*
FR :
    Ce fichier centralise tous les paramètres de la simulation : affichage, grille, fluides, méthodes numériques, etc.
    Il permet de configurer facilement la fenêtre (taille, adaptation à la grille), le comportement du fluide (viscosité, flux, vortex),
    le choix des algorithmes (projection, advection), ainsi que les sources de densité et de vitesse.

    NOUVEAU : paramètres pour la simulation bi-fluide (eau + air) via une méthode VOF
    (Volume Of Fluid) : un champ `phase` par cellule (0 = air, 1 = eau), une densité
    locale interpolée entre RHO_AIR et RHO_WATER, et une gravité réactivée pour faire
    apparaître la surface libre, la poussée d'Archimède et les vagues.

ENG :
    This file centralizes all parameters for the simulation : display, grid, fluid properties, numerical methods, etc.
    It allows configuring the window (size, grid adaptation), fluid behavior (viscosity, flows, vortex),
    algorithm choices (projection, advection), and sources of density and velocity.

    NEW: parameters for two-fluid (water + air) simulation via a VOF (Volume Of Fluid)
    method: a per-cell `phase` field (0 = air, 1 = water), a local density interpolated
    between RHO_AIR and RHO_WATER, and gravity re-enabled to produce the free surface,
    buoyancy, and waves.
*/
use crate::grid2::Vector22;

// Window parameters
pub const ADAPT_TO_WINDOW: bool = true;  // Adapt the window size to the grid size
pub const WINDOW_HEIGHT: usize = if ADAPT_TO_WINDOW == true { (N as usize +2) * DY as usize } else { 720 };  // Height of the window
pub const WINDOW_WIDTH: usize = if ADAPT_TO_WINDOW == true { (N as usize +2) * DX as usize } else { 720 };   // Width of the window



// Simulation parameters
pub const GRID : &str = "3"; // "1" for grid, "2" for grid2
pub const SIM_STEPS : usize = 5000; // Potentially the number of simulation steps
pub const PRINT_FORCES : bool = true; // Print forces
pub const OBJET_SIZE_PRINT_LIMIT : usize = 10; //Limit size for the object forces to be printed
pub const CIP_CSL4 : bool = false; // (For now have to keep it on false) Use CIP-CSL4 method for density advection
pub const DENS_ADV_FAC: f32 = 0.1; // Factor for density advection
pub const VEL_STEP: &str = "2"; // "1" for vel_step, "2" for vel2_step, "cip_csl4" for vel_step_cip_csl4
pub const PROJECT : &str = "1"; // "1" for 'project', "2" for 'project2'
pub const KARMAN_VORTEX : bool = false; // Use Karman vortex (at least tries to)
pub const CENTER_SOURCE : bool = false; // Use a circular source in the center of the grid
pub const CENTER_SOURCE_TYPE : bool = false; // "false" for a circular output, "true" for a radial output (like a fan)
pub const CENTER_SOURCE_RADIUS : f32 = N/40.0; // Radius of the center source
pub const CENTER_SOURCE_DENSITY : f32 = 0.15; // Density of the center source
pub const CENTER_SOURCE_VELOCITY : f32 = 0.5; // Velocity of the center source




// Flow parameters
pub const AIR_FLOW: bool = true; // Simulate air flow
pub const FLOW_DIRECTION: &str = "right"; //"left","right","up","down" // Direction of the flow (for now only right is properly implemented)
pub const FLOW_SPACE: usize = 2; // Space between two rows of flow
pub const FLOW_DENSITY: f32 = 15.0; // Density of the flow
pub const FLOW_VELOCITY: f32 = if AIR_FLOW == true {2.7} else { 0.0 }; // Velocity of the flow
pub const DRAW_VELOCITY_VECTORS: bool = false; // Draw velocity vectors
pub const VECTOR_SIZE_FACTOR : f32 = 8.0 ; // Factor for the size of the velocity vector
pub const INFLOW_VELOCITY: f32 = FLOW_VELOCITY; // Always force inflow velocity to be equal to the flow velocity
pub const PAINT_VORTICITY: bool = false; // Paint vorticity


// Grid parameters
pub const EXT_BORDER: bool = false;  // enable or disable external borders
pub const N: f32 = 510.0; // There are special conditions for the size of the grid: when using Morton encoding, the grid size must be a power of 2, then subtract 2. // IT MUST BE AN INTEGER \\
pub const DX: f32 = 2.0; // Size of a cell (in pixel), horizontal. // IT MUST BE AN INTEGER \\
pub const DY: f32 = 2.0; // Size of a cell (in pixel), vertical. // IT MUST BE AN INTEGER \\
pub const SIZE: f32 = (N + 2.0) * (N + 2.0); // IT WILL BE AN INTEGER \\




// Physical parameters
pub const DT: f32 = 1.0/60.0; // Time step (in seconds)
pub fn gravity() -> Vector22 {
    // Utilisée par Grid2 (grid2.rs). Laissée à zéro par défaut : Grid2 est le
    // module expérimental Morton, indépendant du module eau ci-dessous, qui
    // cible Grid (grid.rs). Si vous voulez de la gravité sur Grid2 aussi,
    // remplacez cette valeur par Vector22::new(GRAVITY_X, GRAVITY_Y).
    let gravity: Vector22 = Vector22::new(0.0, 0.0); // Gravity (in m/s^)
    gravity
}

// Fluid parameters
pub const VISCOSITY: f32 = 0.000; // A bit of viscosity for Karman vortex


// Log parameters
pub const LOG: bool = false; // Log simulation data


// Ship parameters
pub const W_DRAG: f32 = 1.0;
pub const W_TORQUE: f32 = 1.0;


// ============================================================================
// EAU / SIMULATION BI-FLUIDE (VOF) — voir water.rs
// ============================================================================

/// Active la simulation bi-fluide (eau + air) sur `Grid` (grid.rs).
/// Quand false, le comportement est strictement identique à avant : gravité
/// nulle, pas de champ `phase` pris en compte dans la projection.
pub const ENABLE_WATER: bool = false;

/// Densité de l'air (kg/m^3, ordre de grandeur réel — l'échelle absolue
/// importe peu, seul le RATIO RHO_WATER/RHO_AIR compte pour la physique).
pub const RHO_AIR: f32 = 1.0;

/// Densité de l'eau (kg/m^3).
///
/// IMPORTANT : ratio volontairement réduit (25:1 au lieu du ratio physique
/// ~833:1) pour que le solveur de pression Gauss-Seidel actuel converge en
/// un nombre d'itérations raisonnable. Une fois le comportement validé
/// (surface libre stable, flottaison correcte, pas de bruit numérique),
/// remontez progressivement cette valeur vers 1000.0 en augmentant en
/// parallèle WATER_PROJECT_ITERATIONS et en surveillant la convergence
/// (voir STABILITE_PERF.md, sections A et D).
pub const RHO_WATER: f32 = 25.0;

/// Hauteur initiale de la ligne de flottaison, en indices de cellule j
/// (0 = haut de la grille, N = bas). Toute cellule avec j >= WATER_LEVEL est
/// initialisée comme eau (phase = 1.0), le reste comme air (phase = 0.0).
/// Ajustez selon l'orientation de votre grille à l'écran.
pub const WATER_LEVEL: f32 = N * 0.6;

/// Composantes de la gravité appliquée à `Grid` (grid.rs) lorsque
/// ENABLE_WATER est actif. Exprimée en unités de grille par seconde^2 :
/// avec DT = 1/60 s et une grille de N=510 cellules, une valeur physique de
/// 9.81 m/s^2 doit être mise à l'échelle de votre simulation — commencez par
/// une petite valeur et augmentez progressivement pour garder la stabilité
/// numérique du solveur de pression.
pub const GRAVITY_X: f32 = 0.0;
pub const GRAVITY_Y: f32 = 9.81;

/// Nombre d'itérations Gauss-Seidel pour la projection de pression en mode
/// bi-fluide. Le contraste de densité ralentit la convergence : à ajuster à
/// la hausse si vous augmentez RHO_WATER (voir STABILITE_PERF.md).
pub const WATER_PROJECT_ITERATIONS: usize = 60;

/// Vitesse maximale autorisée par composante (unités de grille/s) en mode
/// eau, purement défensive le temps de stabiliser le solveur de pression.
pub const MAX_VELOCITY: f32 = 15.0;
