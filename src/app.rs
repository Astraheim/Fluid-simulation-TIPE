use crate::conditions::*;
use crate::grid::{Grid, ObjectForce};
use crate::ship::{build_boat_layout, random_layout, BoatParams, ContainerLayoutKind};
use egui::Context;
use egui_wgpu::{Renderer as EguiRenderer, ScreenDescriptor};
use pixels::{Pixels, SurfaceTexture};
use pixels::wgpu; // réexporté par pixels : garantit la même version que celle utilisée en interne
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{EventLoop};
use winit::window::WindowBuilder;

/*
Remplace run_simulation() de visualization.rs.
*/

/// Champ affiché dans la grille.
#[derive(Clone, Copy, PartialEq)]
enum DisplayMode {
    Density,
    Vorticity,
    Pressure,
    Velocity,
    Phase,
}

/// Forme à stamper au centre de la grille lors d'un redémarrage.
#[derive(Clone, Copy, PartialEq)]
enum ShapeKind {
    None,
    Triangle,
    Square,
    Rectangle,
    Circle,
}

/// Configuration de flux/scénario appliquée lors d'un redémarrage, et
/// réappliquée chaque frame pour l'injection de densité/vitesse.
#[derive(Clone, Copy, PartialEq)]
enum ConfigKind {
    /// Tunnel de vent classique (coffre gauche percé, flux horizontal).
    ClassicFlow,
    /// Source circulaire (radiale ou tourbillonnante) au centre de la grille.
    CenterSource,
    /// Obstacle circulaire + flux horizontal, pour favoriser une allée de
    /// tourbillons de Kármán.
    KarmanVortex,
}

/// État des champs du panneau "Scénario / Réinitialisation".
struct ScenarioUiState {
    shape: ShapeKind,
    shape_size: f32,
    config: ConfigKind,
    center_source_radial: bool,
}

impl Default for ScenarioUiState {
    fn default() -> Self {
        Self {
            shape: ShapeKind::None,
            shape_size: 40.0,
            config: ConfigKind::ClassicFlow,
            center_source_radial: true,
        }
    }
}

/// Paramètres modifiables sans recréer la grille.
pub struct SimParams {
    pub flow_velocity: f32,
    pub flow_density: f32,
    pub viscosity: f32,
    pub w_drag: f32,
    pub w_torque: f32,
    pub paused: bool,
    pub pen_size: usize,
    pub straight_line_mode: bool,
    display_mode: DisplayMode,
    /// Seuil en dessous duquel un objet (nombre de cellules) n'est pas tracé.
    pub min_cells_to_plot: usize,
    /// Colonne (x) en dessous de laquelle un objet n'est pas tracé.
    pub min_column_x: f32,
    /// Facteur de "boost" appliqué au champ de vorticité avant colorisation :
    /// une valeur > 1 fait sortir du blanc plus vite les zones à vorticité
    /// modérée (au lieu de n'avoir de la couleur que sur les extrêmes).
    pub vorticity_gain: f32,
    /// Même principe pour la pression.
    pub pressure_gain: f32,
}

impl Default for SimParams {
    fn default() -> Self {
        Self {
            flow_velocity: FLOW_VELOCITY,
            flow_density: FLOW_DENSITY,
            viscosity: VISCOSITY,
            w_drag: 1.0,
            w_torque: 1.0,
            paused: false,
            pen_size: 1,
            straight_line_mode: false,
            display_mode: DisplayMode::Density,
            min_cells_to_plot: 20,
            min_column_x: 0.0,
            vorticity_gain: 1.8,
            pressure_gain: 1.8,
        }
    }
}

/// Historique glissant traînée/couple pour UN objet détecté, avec les
/// dernières métadonnées connues (taille, position) utilisées pour le filtrage.
pub struct ObjectHistory {
    pub drag: Vec<[f64; 2]>,
    pub torque: Vec<[f64; 2]>,
    pub last_cell_count: usize,
    pub last_center_x: f32,
}

/// Historiques séparés par id d'objet (au lieu d'un seul historique agrégé).
pub struct ForceHistories {
    pub per_object: HashMap<usize, ObjectHistory>,
    pub step: usize,
    pub max_points: usize,
}

impl ForceHistories {
    fn new(max_points: usize) -> Self {
        Self { per_object: HashMap::new(), step: 0, max_points }
    }

    fn push(&mut self, objects: &[ObjectForce]) {
        for obj in objects {
            let entry = self.per_object.entry(obj.id).or_insert_with(|| ObjectHistory {
                drag: Vec::new(),
                torque: Vec::new(),
                last_cell_count: 0,
                last_center_x: 0.0,
            });
            entry.drag.push([self.step as f64, obj.total_force.x as f64]);
            entry.torque.push([self.step as f64, obj.torque as f64]);
            entry.last_cell_count = obj.cell_count;
            entry.last_center_x = obj.center_of_mass.x;
            if entry.drag.len() > self.max_points {
                entry.drag.remove(0);
                entry.torque.remove(0);
            }
        }
        self.step += 1;
    }

    fn clear(&mut self) {
        self.per_object.clear();
        self.step = 0;
    }
}

/// Historique glissant du temps de calcul d'un tick (ms).
struct TickHistory {
    points: Vec<[f64; 2]>,
    max_points: usize,
    step: usize,
}

impl TickHistory {
    fn new(max_points: usize) -> Self {
        Self { points: Vec::new(), max_points, step: 0 }
    }

    fn push(&mut self, ms: f64) {
        self.points.push([self.step as f64, ms]);
        if self.points.len() > self.max_points {
            self.points.remove(0);
        }
        self.step += 1;
    }
}

/// État des champs du panneau "Générateur de bateau".
struct BoatUiState {
    hull_width: f32,
    hull_height: f32,
    container_width: f32,
    container_height: f32,
    container_gap: f32,
    kind_index: usize, // 0 = Grille, 1 = Pyramide, 2 = Quinconce
    rows: usize,
    cols: usize,

    // Courbure de coque
    curve_bow: bool,
    curve_stern: bool,
    hull_curvature: f32,
    bow_length: f32,
    stern_length: f32,

    // Carénages aéro
    add_bow_fairing: bool,
    add_stern_fairing: bool,
    fairing_length: f32,

    // Nombre de configurations aléatoires à comparer (console uniquement)
    random_batch_size: usize,
}

impl Default for BoatUiState {
    fn default() -> Self {
        Self {
            hull_width: 30.0,
            hull_height: 60.0,
            container_width: 8.0,
            container_height: 8.0,
            container_gap: 1.0,
            kind_index: 0,
            rows: 3,
            cols: 3,
            curve_bow: true,
            curve_stern: false,
            hull_curvature: 0.6,
            bow_length: 15.0,
            stern_length: 15.0,
            add_bow_fairing: false,
            add_stern_fairing: false,
            fairing_length: 10.0,
            random_batch_size: 8,
        }
    }
}

/// Convertit la densité en couleur (reprend density_color de visualization.rs)
fn density_color(density: f32) -> [u8; 4] {
    if density <= 20.0 {
        let intensity = (255.0 * (1.0 - density / 20.0)).clamp(0.0, 255.0) as u8;
        [intensity, intensity, 0xFF, 0xFF]
    } else {
        let excess = (density - 20.0).clamp(0.0, 20.0);
        let red = (255.0 * (excess / 20.0)).clamp(0.0, 255.0) as u8;
        [red, 0x00, 0x00, 0xFF]
    }
}

/// Colormap divergente bleu -> blanc -> rouge, pour des champs pouvant être
/// négatifs ou positifs (vorticité, pression). t attendu dans [-1, 1].
///
/// Avant : la transition passait linéairement par un blanc très étendu, donc
/// une grande partie de la grille restait quasi blanche et peu lisible.
/// Maintenant : on applique une courbe (`powf`) qui pousse les valeurs
/// intermédiaires plus vite vers une couleur saturée, ce qui réduit
/// nettement la zone blanche sans changer les bornes (toujours blanc pur à
/// t=0, bleu/rouge pur à t=±1).
fn diverging_colormap(t: f32) -> [u8; 4] {
    let t = t.clamp(-1.0, 1.0);
    let boosted = t.signum() * t.abs().powf(0.5);
    let (r, g, b) = if boosted < 0.0 {
        let s = 1.0 + boosted; // 0 à t=-1 -> 1 à t=0
        (s, s, 1.0)
    } else {
        let s = 1.0 - boosted; // 1 à t=0 -> 0 à t=1
        (1.0, s, s)
    };
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255]
}

/// Colormap séquentielle (type "viridis" simplifiée), pour des champs
/// toujours positifs (norme de vitesse). t attendu dans [0, 1].
fn sequential_colormap(t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    const STOPS: [(f32, f32, f32); 5] = [
        (0.05, 0.03, 0.30),
        (0.00, 0.35, 0.55),
        (0.00, 0.65, 0.35),
        (0.75, 0.85, 0.10),
        (0.99, 0.90, 0.15),
    ];
    let n = STOPS.len() - 1;
    let scaled = t * n as f32;
    let idx = (scaled.floor() as usize).min(n - 1);
    let frac = scaled - idx as f32;
    let (r0, g0, b0) = STOPS[idx];
    let (r1, g1, b1) = STOPS[idx + 1];
    let r = r0 + (r1 - r0) * frac;
    let g = g0 + (g1 - g0) * frac;
    let b = b0 + (b1 - b0) * frac;
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255]
}

/// Vorticity locale (reprise de visualization.rs, adaptée pour l'app egui/pixels)
fn vorticity_at(grid: &Grid, i: usize, j: usize) -> f32 {
    let dx = DX;
    let dy = DY;

    let im = i.saturating_sub(1).max(1);
    let ip = (i + 1).min(N as usize);
    let jm = j.saturating_sub(1).max(1);
    let jp = (j + 1).min(N as usize);

    let idx_up = grid.to_index(i, jp);
    let idx_down = grid.to_index(i, jm);
    let idx_left = grid.to_index(im, j);
    let idx_right = grid.to_index(ip, j);

    let du_dy = (grid.cells[idx_up].velocity_x - grid.cells[idx_down].velocity_x) / (2.0 * dy);
    let dv_dx = (grid.cells[idx_right].velocity_y - grid.cells[idx_left].velocity_y) / (2.0 * dx);

    dv_dx - du_dy
}

/// Rendu de la grille dans le buffer pixels, à résolution N x N, selon le
/// champ sélectionné. Pour les champs calculés (vorticité, pression,
/// vitesse), un premier passage calcule l'échelle courante (autoscale) afin
/// que le gradient reste lisible quelle que soit l'intensité du champ.
///
/// NOTE sur la pression : ce mode n'affichait rien avant car `Grid::project`
/// ne recopiait jamais le champ de pression résolu dans `cell.pressure`
/// (voir le correctif dans grid.rs) — `cell.pressure` restait donc toujours
/// à 0.0 et `max_abs` valait toujours ~1e-6, écrasant tout en blanc.
fn draw_grid(grid: &Grid, frame: &mut [u8], grid_w: usize, mode: DisplayMode, vorticity_gain: f32, pressure_gain: f32) {
    let n_max = N as usize + 1;

    let mut max_abs: f32 = 1e-6;
    if mode != DisplayMode::Density {
        for j in 1..n_max {
            for i in 1..n_max {
                let idx = grid.to_index(i, j);
                if grid.cells[idx].wall {
                    continue;
                }
                let v = match mode {
                    DisplayMode::Vorticity => vorticity_at(grid, i, j).abs(),
                    DisplayMode::Pressure => grid.cells[idx].pressure.abs(),
                    DisplayMode::Velocity => {
                        let vx = grid.cells[idx].velocity_x;
                        let vy = grid.cells[idx].velocity_y;
                        (vx * vx + vy * vy).sqrt()
                    }
                    DisplayMode::Density => 0.0,
                    DisplayMode::Phase => grid.cells[idx].phase.abs(),
                };
                if v > max_abs {
                    max_abs = v;
                }
            }
        }
    }

    for j in 1..n_max {
        for i in 1..n_max {
            let idx = grid.to_index(i, j);
            let px = (j * grid_w + i) * 4;
            let color = if grid.cells[idx].wall {
                [0, 0, 0, 255]
            } else {
                match mode {
                    DisplayMode::Density => density_color(grid.cells[idx].density),
                    DisplayMode::Vorticity => {
                        let t = (vorticity_at(grid, i, j) / max_abs) * vorticity_gain;
                        diverging_colormap(t)
                    }
                    DisplayMode::Pressure => {
                        let t = (grid.cells[idx].pressure / max_abs) * pressure_gain;
                        diverging_colormap(t)
                    }
                    DisplayMode::Velocity => {
                        let vx = grid.cells[idx].velocity_x;
                        let vy = grid.cells[idx].velocity_y;
                        let speed = (vx * vx + vy * vy).sqrt();
                        sequential_colormap(speed / max_abs)
                    }
                    DisplayMode::Phase => {
                        let t = grid.cells[idx].phase.clamp(0.0, 1.0);
                        // bleu foncé (eau) -> blanc/bleu clair (air)
                        let r = (200.0 * (1.0 - t) + 20.0 * t) as u8;
                        let g = (220.0 * (1.0 - t) + 60.0 * t) as u8;
                        let b = (255.0 * (1.0 - t) + 160.0 * t) as u8;
                        [r, g, b, 255]
                    }
                }
            };
            frame[px..px + 4].copy_from_slice(&color);
        }
    }
}

/// Tamponne un mur de taille `pen_size` (diamètre approx.) centré sur (cx, cy).
/// pen_size = 1 -> une seule cellule, comme avant.
fn stamp_wall(grid: &mut Grid, cx: usize, cy: usize, pen_size: usize) {
    let r = (pen_size as isize) / 2;
    let cxi = cx as isize;
    let cyi = cy as isize;

    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r + 1 {
                let x = cxi + dx;
                let y = cyi + dy;
                if x >= 1 && x <= N as isize && y >= 1 && y <= N as isize {
                    grid.wall_init(y as usize, x as usize, true);
                }
            }
        }
    }
}

/// Applique la forme choisie au centre de la grille (utilisé au redémarrage).
fn apply_shape(grid: &mut Grid, shape: ShapeKind, size: f32) {
    let cx = N as isize / 2;
    let cy = N as isize / 2;
    match shape {
        ShapeKind::None => {}
        ShapeKind::Triangle => grid.triangle(cx, cy, size),
        ShapeKind::Square => grid.square(cx, cy, size),
        ShapeKind::Rectangle => grid.rectangle(cx, cy, size, size * 0.6),
        ShapeKind::Circle => grid.circle(cx, cy, size / 2.0),
    }
}

/// (Ré)initialise une grille selon la configuration choisie (murs fixes
/// posés une seule fois ici ; l'injection continue est gérée à chaque frame
/// dans la boucle principale, voir `inject_for_config`).
fn setup_config(grid: &mut Grid, config: ConfigKind, hole_pos: &[usize]) {
    match config {
        ConfigKind::ClassicFlow => {
            grid.setup_wind_tunnel_walls(hole_pos);
            grid.init_water(WATER_LEVEL);
        }
        ConfigKind::CenterSource => {
            // Pas de murs fixes : seule l'injection au centre est nécessaire.
            grid.init_water(WATER_LEVEL);
        }
        ConfigKind::KarmanVortex => {
            grid.setup_karman_vortex();
            grid.init_water(WATER_LEVEL);
        }
    }
}

/// Injection continue (densité/vitesse) répétée à chaque frame, selon la
/// configuration active.
fn inject_for_config(grid: &mut Grid, config: ConfigKind, params: &SimParams, scenario: &ScenarioUiState) {
    match config {
        ConfigKind::ClassicFlow => {
            grid.inject_wind_tunnel_flow(params.flow_density, params.flow_velocity);
        }
        ConfigKind::CenterSource => {
            grid.center_source(
                CENTER_SOURCE_RADIUS,
                params.flow_density.max(0.1),
                params.flow_velocity.max(0.1),
                scenario.center_source_radial,
            );
        }
        ConfigKind::KarmanVortex => {
            // Entretient le flux entrant à gauche, en plus de la condition
            // aux limites (sinon la densité injectée une seule fois au
            // setup s'épuise très vite en aval).
            for j in 1..=(N as usize) {
                let idx = grid.to_index(1, j);
                if !grid.cells[idx].wall {
                    grid.cell_init(1, j, params.flow_velocity, 0.0, params.flow_density);
                }
            }
        }
    }
}

pub fn run_app(mut grid: Grid) -> ! {
    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_title("Simulation - pixels + egui")
        .with_inner_size(winit::dpi::LogicalSize::new(WINDOW_WIDTH as f64, WINDOW_HEIGHT as f64))
        .with_resizable(true)
        .build(&event_loop)
        .unwrap();
    let window = Arc::new(window);

    let grid_w = N as usize + 2;
    let grid_h = N as usize + 2;

    let win_size = window.inner_size();
    let surface_texture = SurfaceTexture::new(win_size.width, win_size.height, Arc::clone(&window));
    let mut pixels = Pixels::new(grid_w as u32, grid_h as u32, surface_texture).unwrap();

    // --- Setup egui ---
    let mut egui_ctx = Context::default();
    let mut egui_winit = egui_winit::State::new(egui_ctx.clone(), egui::ViewportId::ROOT, &window, None, None);
    let mut egui_renderer = EguiRenderer::new(pixels.device(), pixels.render_texture_format(), None, 1);

    let mut params = SimParams::default();
    let mut history = ForceHistories::new(300);
    let mut tick_history = TickHistory::new(300);
    let mut last_tick_ms: f64 = 0.0;
    let mut boat_ui = BoatUiState::default();
    let mut scenario = ScenarioUiState::default();
    let mut step: usize = 0;
    let start = Instant::now();

    // État pour le dessin de murs à la souris
    let mut mouse_down = false;
    let mut last_grid_pos: Option<(usize, usize)> = None; // pour le mode "libre"
    let mut line_anchor: Option<(usize, usize)> = None;   // pour le mode "ligne droite"
    let mut last_cursor_pixel: Option<(usize, usize)> = None; // dernière position connue (grille)

    let hole_pos: Vec<usize> = (1..=N as usize).filter(|&x| x % FLOW_SPACE == 0).collect();

    // Configuration initiale (par défaut : tunnel de vent classique), murs
    // fixes posés une seule fois ici.
    setup_config(&mut grid, scenario.config, &hole_pos);

    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { event, .. } => {
                let response = egui_winit.on_window_event(&window, &event);
                if response.consumed {
                    // egui a géré l'événement (clic sur un slider, etc.)
                } else if let WindowEvent::CloseRequested = event {
                    elwt.exit();
                } else if let WindowEvent::Resized(new_size) = event {
                    // Fenêtre redimensionnée (plein écran compris) : on redimensionne
                    // uniquement la SURFACE d'affichage, pas la résolution de la grille
                    // (grid_w x grid_h) -> pas de recréation de buffer, pas de crash.
                    if new_size.width > 0 && new_size.height > 0 {
                        if let Err(e) = pixels.resize_surface(new_size.width, new_size.height) {
                            eprintln!("Erreur redimensionnement surface: {e}");
                        }
                    }
                } else if let WindowEvent::MouseInput { state, button, .. } = event {
                    if button == winit::event::MouseButton::Left {
                        let pressed = state == winit::event::ElementState::Pressed;

                        if pressed {
                            mouse_down = true;
                            if params.straight_line_mode {
                                line_anchor = last_cursor_pixel;
                            } else {
                                last_grid_pos = None;
                                if let Some((gx, gy)) = last_cursor_pixel {
                                    stamp_wall(&mut grid, gx, gy, params.pen_size);
                                    last_grid_pos = Some((gx, gy));
                                }
                            }
                        } else {
                            if params.straight_line_mode {
                                if let (Some(start_p), Some(end_p)) = (line_anchor, last_cursor_pixel) {
                                    for (x, y) in bresenham_line(start_p.0, start_p.1, end_p.0, end_p.1) {
                                        stamp_wall(&mut grid, x, y, params.pen_size);
                                    }
                                }
                                line_anchor = None;
                            }
                            mouse_down = false;
                            last_grid_pos = None;
                        }
                    }
                } else if let WindowEvent::CursorMoved { position, .. } = event {
                    if let Ok((gx, gy)) = pixels.window_pos_to_pixel((position.x as f32, position.y as f32)) {
                        if gx >= 1 && gx <= N as usize && gy >= 1 && gy <= N as usize {
                            last_cursor_pixel = Some((gx, gy));

                            if mouse_down && !params.straight_line_mode {
                                stamp_wall(&mut grid, gx, gy, params.pen_size);
                                if let Some((lx, ly)) = last_grid_pos {
                                    for (x, y) in bresenham_line(lx, ly, gx, gy) {
                                        stamp_wall(&mut grid, x, y, params.pen_size);
                                    }
                                }
                                last_grid_pos = Some((gx, gy));
                            }
                        }
                    }
                } else if let WindowEvent::RedrawRequested = event {
                    // --- 1. UI egui (collectée avant le pas de simulation, pour
                    //        que "Step" / "Appliquer bateau" / "Redémarrer" / "Quitter"
                    //        agissent dès cette frame) ---
                    let mut do_step = false;
                    let mut apply_boat = false;
                    let mut apply_random_boat = false;
                    let mut do_restart = false;

                    let raw_input = egui_winit.take_egui_input(&window);
                    let full_output = egui_ctx.run(raw_input, |ctx| {
                        egui::SidePanel::right("controls").show(ctx, |ui| {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                ui.heading("Paramètres");
                                ui.add(egui::Slider::new(&mut params.flow_velocity, 0.0..=5.0).text("Vitesse flux"));
                                ui.add(egui::Slider::new(&mut params.flow_density, 0.0..=50.0).text("Densité flux"));
                                ui.add(egui::Slider::new(&mut params.viscosity, 0.0..=0.01).text("Viscosité"));
                                ui.separator();
                                ui.add(egui::Slider::new(&mut params.w_drag, 0.0..=5.0).text("Poids traînée"));
                                ui.add(egui::Slider::new(&mut params.w_torque, 0.0..=5.0).text("Poids couple"));

                                ui.separator();
                                ui.label("Champ affiché");
                                ui.radio_value(&mut params.display_mode, DisplayMode::Density, "Densité");
                                ui.radio_value(&mut params.display_mode, DisplayMode::Vorticity, "Vorticité (gradient)");
                                ui.radio_value(&mut params.display_mode, DisplayMode::Pressure, "Pression (gradient)");
                                ui.radio_value(&mut params.display_mode, DisplayMode::Velocity, "Vitesse (gradient)");
                                ui.radio_value(&mut params.display_mode, DisplayMode::Phase, "Phase");
                                if params.display_mode == DisplayMode::Vorticity {
                                    ui.add(egui::Slider::new(&mut params.vorticity_gain, 0.3..=5.0).text("Contraste vorticité"));
                                }
                                if params.display_mode == DisplayMode::Pressure {
                                    ui.add(egui::Slider::new(&mut params.pressure_gain, 0.3..=5.0).text("Contraste pression"));
                                }

                                ui.separator();
                                ui.label("Dessin de murs");
                                ui.add(egui::Slider::new(&mut params.pen_size, 1..=30).text("Taille du stylo"));
                                ui.checkbox(&mut params.straight_line_mode, "Ligne droite (clic → relâcher)");

                                ui.separator();
                                if ui.button(if params.paused { "Reprendre" } else { "Pause" }).clicked() {
                                    params.paused = !params.paused;
                                }
                                ui.add_enabled_ui(params.paused, |ui| {
                                    if ui.button("Step (1 pas)").clicked() {
                                        do_step = true;
                                    }
                                });
                                if ui.button("Screenshot").clicked() {
                                    save_screenshot(pixels.frame(), grid_w, grid_h, step);
                                }
                                if ui.button("Quitter (stop propre)").clicked() {
                                    elwt.exit();
                                }

                                ui.separator();
                                let fps = if last_tick_ms > 0.0 { 1000.0 / last_tick_ms } else { 0.0 };
                                ui.label(format!("Step: {step}  |  t: {:.1}s", start.elapsed().as_secs_f32()));
                                ui.label(format!("Temps/tick: {:.2} ms  |  FPS (calcul): {:.1}", last_tick_ms, fps));
                                egui_plot::Plot::new("tick_ms_plot").height(100.0).show(ui, |plot_ui| {
                                    plot_ui.line(
                                        egui_plot::Line::new(egui_plot::PlotPoints::from(tick_history.points.clone()))
                                            .name("ms/tick"),
                                    );
                                });

                                ui.separator();
                                ui.collapsing("Scénario / Réinitialisation", |ui| {
                                    ui.label("Forme à placer au centre");
                                    egui::ComboBox::from_label("Forme")
                                        .selected_text(match scenario.shape {
                                            ShapeKind::None => "Aucune",
                                            ShapeKind::Triangle => "Triangle",
                                            ShapeKind::Square => "Carré",
                                            ShapeKind::Rectangle => "Rectangle",
                                            ShapeKind::Circle => "Cercle",
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut scenario.shape, ShapeKind::None, "Aucune");
                                            ui.selectable_value(&mut scenario.shape, ShapeKind::Triangle, "Triangle");
                                            ui.selectable_value(&mut scenario.shape, ShapeKind::Square, "Carré");
                                            ui.selectable_value(&mut scenario.shape, ShapeKind::Rectangle, "Rectangle");
                                            ui.selectable_value(&mut scenario.shape, ShapeKind::Circle, "Cercle");
                                        });
                                    ui.add_enabled(
                                        scenario.shape != ShapeKind::None,
                                        egui::Slider::new(&mut scenario.shape_size, 4.0..=(N * 0.5)).text("Taille de la forme"),
                                    );

                                    ui.separator();
                                    ui.label("Configuration du flux");
                                    egui::ComboBox::from_label("Configuration")
                                        .selected_text(match scenario.config {
                                            ConfigKind::ClassicFlow => "Tunnel de vent classique",
                                            ConfigKind::CenterSource => "Source centrale",
                                            ConfigKind::KarmanVortex => "Allée de Von Kármán",
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut scenario.config, ConfigKind::ClassicFlow, "Tunnel de vent classique");
                                            ui.selectable_value(&mut scenario.config, ConfigKind::CenterSource, "Source centrale");
                                            ui.selectable_value(&mut scenario.config, ConfigKind::KarmanVortex, "Allée de Von Kármán");
                                        });
                                    if scenario.config == ConfigKind::CenterSource {
                                        ui.checkbox(&mut scenario.center_source_radial, "Source radiale (sinon tourbillon)");
                                    }

                                    ui.separator();
                                    if ui.button("🔄 Redémarrer la simulation").clicked() {
                                        do_restart = true;
                                    }
                                });

                                ui.separator();
                                ui.collapsing("Générateur de bateau", |ui| {
                                    ui.add(egui::Slider::new(&mut boat_ui.hull_width, 5.0..=(N * 0.9)).text("Largeur coque"));
                                    ui.add(egui::Slider::new(&mut boat_ui.hull_height, 5.0..=(N * 0.9)).text("Hauteur coque"));
                                    ui.add(egui::Slider::new(&mut boat_ui.container_width, 2.0..=100.0).text("Largeur conteneur"));
                                    ui.add(egui::Slider::new(&mut boat_ui.container_height, 2.0..=100.0).text("Hauteur conteneur"));
                                    ui.add(egui::Slider::new(&mut boat_ui.container_gap, 0.0..=40.0).text("Espacement"));

                                    egui::ComboBox::from_label("Organisation")
                                        .selected_text(match boat_ui.kind_index {
                                            0 => "Grille",
                                            1 => "Pyramide",
                                            _ => "Quinconce",
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut boat_ui.kind_index, 0, "Grille");
                                            ui.selectable_value(&mut boat_ui.kind_index, 1, "Pyramide");
                                            ui.selectable_value(&mut boat_ui.kind_index, 2, "Quinconce");
                                        });

                                    ui.add(egui::Slider::new(&mut boat_ui.rows, 1..=25).text("Rangées"));
                                    if boat_ui.kind_index != 1 {
                                        ui.add(egui::Slider::new(&mut boat_ui.cols, 1..=25).text("Colonnes"));
                                    }

                                    ui.separator();
                                    ui.label("Courbure de coque (proue / poupe)");
                                    ui.checkbox(&mut boat_ui.curve_bow, "Proue courbée (avant)");
                                    ui.checkbox(&mut boat_ui.curve_stern, "Poupe courbée (arrière)");
                                    ui.add_enabled(
                                        boat_ui.curve_bow || boat_ui.curve_stern,
                                        egui::Slider::new(&mut boat_ui.hull_curvature, 0.0..=1.0)
                                            .text("Courbure (0 = pointue, 1 = arrondie)"),
                                    );
                                    ui.add_enabled(
                                        boat_ui.curve_bow,
                                        egui::Slider::new(&mut boat_ui.bow_length, 1.0..=(N * 0.3)).text("Longueur proue"),
                                    );
                                    ui.add_enabled(
                                        boat_ui.curve_stern,
                                        egui::Slider::new(&mut boat_ui.stern_length, 1.0..=(N * 0.3)).text("Longueur poupe"),
                                    );

                                    ui.separator();
                                    ui.label("Carénages aéro (gap-flow protectors)");
                                    ui.checkbox(&mut boat_ui.add_bow_fairing, "Carénage avant les conteneurs");
                                    ui.checkbox(&mut boat_ui.add_stern_fairing, "Carénage derrière les conteneurs");
                                    ui.add_enabled(
                                        boat_ui.add_bow_fairing || boat_ui.add_stern_fairing,
                                        egui::Slider::new(&mut boat_ui.fairing_length, 1.0..=(N * 0.2)).text("Longueur carénage"),
                                    );

                                    ui.separator();
                                    if ui.button("Appliquer configuration bateau").clicked() {
                                        apply_boat = true;
                                    }
                                    ui.horizontal(|ui| {
                                        if ui.button("🎲 Configuration aléatoire").clicked() {
                                            apply_random_boat = true;
                                        }
                                        ui.add(egui::Slider::new(&mut boat_ui.random_batch_size, 2..=40).text("N (comparaison console)"));
                                    });
                                    ui.label("« N » lance N configurations aléatoires en tâche de fond \
                                              (headless) et affiche leur classement traînée/couple dans la console, \
                                              à la façon de l'étude jointe sur les carénages d'étrave.");
                                });

                                ui.separator();
                                ui.label("Traînée / Couple par objet détecté");
                                ui.add(egui::Slider::new(&mut params.min_cells_to_plot, 0..=500).text("Taille mini (cellules)"));
                                ui.add(egui::Slider::new(&mut params.min_column_x, 0.0..=N).text("Colonne mini (x)"));
                                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                                    let mut ids: Vec<usize> = history.per_object.keys().cloned().collect();
                                    ids.sort();
                                    for id in ids {
                                        if let Some(h) = history.per_object.get(&id) {
                                            if h.last_cell_count < params.min_cells_to_plot {
                                                continue;
                                            }
                                            if h.last_center_x < params.min_column_x {
                                                continue;
                                            }
                                            ui.label(format!("Objet #{id} ({} cellules)", h.last_cell_count));
                                            egui_plot::Plot::new(("drag_plot", id))
                                                .height(90.0)
                                                .show(ui, |plot_ui| {
                                                    plot_ui.line(
                                                        egui_plot::Line::new(egui_plot::PlotPoints::from(h.drag.clone()))
                                                            .name("Traînée"),
                                                    );
                                                });
                                            egui_plot::Plot::new(("torque_plot", id))
                                                .height(90.0)
                                                .show(ui, |plot_ui| {
                                                    plot_ui.line(
                                                        egui_plot::Line::new(egui_plot::PlotPoints::from(h.torque.clone()))
                                                            .name("Couple"),
                                                    );
                                                });
                                            ui.separator();
                                        }
                                    }
                                });
                            });
                        });
                    });

                    // --- 2. Redémarrage "scénario" (forme + configuration de flux) ---
                    if do_restart {
                        grid = Grid::new();
                        apply_shape(&mut grid, scenario.shape, scenario.shape_size);
                        setup_config(&mut grid, scenario.config, &hole_pos);
                        history.clear();
                        step = 0;
                    }

                    // --- 3. Application de la configuration bateau, si demandée ---
                    let boat_params_from_ui = |boat_ui: &BoatUiState| -> BoatParams {
                        let kind = match boat_ui.kind_index {
                            0 => ContainerLayoutKind::Grid { rows: boat_ui.rows, cols: boat_ui.cols },
                            1 => ContainerLayoutKind::Pyramid { rows: boat_ui.rows },
                            _ => ContainerLayoutKind::Staggered { rows: boat_ui.rows, cols: boat_ui.cols },
                        };
                        BoatParams {
                            center: (N as isize / 2, N as isize / 2),
                            hull_width: boat_ui.hull_width,
                            hull_height: boat_ui.hull_height,
                            container_width: boat_ui.container_width,
                            container_height: boat_ui.container_height,
                            container_gap: boat_ui.container_gap,
                            kind,
                            flow_angle: 0.0,
                            curve_bow: boat_ui.curve_bow,
                            curve_stern: boat_ui.curve_stern,
                            hull_curvature: boat_ui.hull_curvature,
                            bow_length: boat_ui.bow_length,
                            stern_length: boat_ui.stern_length,
                            add_bow_fairing: boat_ui.add_bow_fairing,
                            add_stern_fairing: boat_ui.add_stern_fairing,
                            fairing_length: boat_ui.fairing_length,
                        }
                    };

                    if apply_boat {
                        let boat_params = boat_params_from_ui(&boat_ui);
                        let layout = build_boat_layout("custom", &boat_params);
                        grid = Grid::new();
                        grid.apply_layout(&layout);
                        // La grille vient d'être recréée : il faut reposer les
                        // murs de la configuration active une fois.
                        setup_config(&mut grid, scenario.config, &hole_pos);
                        history.clear();
                        step = 0;
                    }

                    if apply_random_boat {
                        let base_params = boat_params_from_ui(&boat_ui);
                        let mut rng = rand::rng();
                        let layout = random_layout("alea_apercu", &base_params, &mut rng);
                        grid = Grid::new();
                        grid.apply_layout(&layout);
                        setup_config(&mut grid, scenario.config, &hole_pos);
                        history.clear();
                        step = 0;

                        // Comparaison headless lancée en tâche de fond pour ne pas geler l'UI.
                        let n = boat_ui.random_batch_size;
                        let hole_pos_bg = hole_pos.clone();
                        std::thread::spawn(move || {
                            let variants = crate::ship::random_variants(base_params, n);
                            crate::ship::compare_layouts(&variants, 100, &hole_pos_bg);
                        });
                    }

                    // --- 4. Pas de simulation (normal ou manuel via "Step"), avec
                    //        mesure du temps de calcul du tick ---
                    if !params.paused || do_step {
                        let tick_start = Instant::now();

                        inject_for_config(&mut grid, scenario.config, &params, &scenario);
                        grid.vel2_step(params.flow_velocity);
                        step += 1;

                        let objects = grid.compute_object_forces();
                        history.push(&objects);

                        let ms = tick_start.elapsed().as_secs_f64() * 1000.0;
                        last_tick_ms = ms;
                        tick_history.push(ms);
                    }

                    // --- 5. Rendu grille dans le buffer pixels ---
                    draw_grid(&grid, pixels.frame_mut(), grid_w, params.display_mode, params.vorticity_gain, params.pressure_gain);

                    // --- 6. Finalisation UI ---
                    egui_winit.handle_platform_output(&window, full_output.platform_output);
                    let clipped_primitives = egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);

                    // --- 7. Rendu combiné pixels + egui ---
                    let render_result = pixels.render_with(|encoder, render_target, context| {
                        context.scaling_renderer.render(encoder, render_target);

                        let current_size = window.inner_size();
                        let screen_descriptor = ScreenDescriptor {
                            size_in_pixels: [current_size.width, current_size.height],
                            pixels_per_point: full_output.pixels_per_point,
                        };
                        for (id, delta) in &full_output.textures_delta.set {
                            egui_renderer.update_texture(&context.device, &context.queue, *id, delta);
                        }
                        egui_renderer.update_buffers(&context.device, &context.queue, encoder, &clipped_primitives, &screen_descriptor);
                        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("egui"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: render_target,
                                resolve_target: None,
                                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        egui_renderer.render(&mut rpass, &clipped_primitives, &screen_descriptor);
                        Ok(())
                    });

                    if render_result.is_err() {
                        elwt.exit();
                    }
                    window.request_redraw();
                }
            }
            Event::AboutToWait => window.request_redraw(),
            _ => {}
        }
    }).unwrap();

    unreachable!()
}

fn save_screenshot(frame: &[u8], w: usize, h: usize, step: usize) {
    let path = format!("screenshot_step_{step}.png");
    if let Err(e) = image::save_buffer(&path, frame, w as u32, h as u32, image::ColorType::Rgba8) {
        eprintln!("Erreur screenshot: {e}");
    } else {
        println!("Screenshot sauvegardée: {path}");
    }
}

/// Bresenham : renvoie les points d'une ligne entre deux positions de grille.
fn bresenham_line(x0: usize, y0: usize, x1: usize, y1: usize) -> Vec<(usize, usize)> {
    let mut points = Vec::new();
    let dx = (x1 as isize - x0 as isize).abs();
    let dy = (y1 as isize - y0 as isize).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    let mut x = x0 as isize;
    let mut y = y0 as isize;

    loop {
        points.push((x as usize, y as usize));
        if x == x1 as isize && y == y1 as isize { break; }
        let e2 = 2 * err;
        if e2 > -dy { err -= dy; x += sx; }
        if e2 < dx { err += dx; y += sy; }
    }
    points
}
