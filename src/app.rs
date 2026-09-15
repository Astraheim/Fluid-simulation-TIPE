use crate::conditions::*;
use crate::grid::{Grid, ObjectForce};
use crate::ship::{build_boat_layout, BoatParams, ContainerLayoutKind};
use egui::Context;
use egui_wgpu::{Renderer as EguiRenderer, ScreenDescriptor};
use pixels::{Pixels, SurfaceTexture};
use pixels::wgpu; // réexporté par pixels : garantit la même version que celle utilisée en interne
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

/*
Remplace run_simulation() de visualization.rs.

Architecture :
    - winit : fenêtre + boucle d'événements
    - pixels : buffer de pixels GPU-accéléré
    - egui + egui-wgpu : UI immédiate par-dessus la texture pixels

Nouveautés (session du jour) :
    - Affichage vorticité (bouton, réutilise le calcul de visualization.rs)
    - Stylo réglable (taille) + mode "ligne droite" pour dessiner les murs
    - Bouton "Step" pour avancer d'un seul pas quand la simulation est en pause
    - Un graphe traînée + un graphe couple PAR objet détecté (au lieu d'un
      graphe global agrégé)
    - Panneau "Générateur de bateau" : construit une ShipLayout (coque +
      conteneurs) suivant plusieurs organisations, pour tester vite (voir
      ship.rs::build_boat_layout / ContainerLayoutKind)
*/

/// Paramètres modifiables sans recréer la grille.
pub struct SimParams {
    pub flow_velocity: f32,
    pub flow_density: f32,
    pub viscosity: f32,
    pub w_drag: f32,
    pub w_torque: f32,
    pub paused: bool,
    pub paint_vorticity: bool,
    pub pen_size: usize,
    pub straight_line_mode: bool,
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
            paint_vorticity: false,
            pen_size: 1,
            straight_line_mode: false,
        }
    }
}

/// Historique glissant traînée/couple pour UN objet détecté.
pub struct ObjectHistory {
    pub drag: Vec<[f64; 2]>,
    pub torque: Vec<[f64; 2]>,
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
            });
            entry.drag.push([self.step as f64, obj.total_force.x as f64]);
            entry.torque.push([self.step as f64, obj.torque as f64]);
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

/// Couleur associée à une valeur de vorticité (bleu = positif, rouge = négatif)
fn vorticity_color(vort: f32) -> [u8; 4] {
    let iv = ((vort.abs().min(5.0)) * 51.0) as u8;
    if vort > 0.0 {
        [0, iv, 0xFF, 0xFF]
    } else {
        [0xFF, iv, 0, 0xFF]
    }
}

/// Rendu de la grille dans le buffer pixels, à résolution N x N.
fn draw_grid(grid: &Grid, frame: &mut [u8], grid_w: usize, paint_vorticity: bool) {
    let n_max = N as usize + 1;
    for j in 1..n_max {
        for i in 1..n_max {
            let idx = grid.to_index(i, j);
            let px = (j * grid_w + i) * 4;
            let color = if grid.cells[idx].wall {
                [0, 0, 0, 255]
            } else if paint_vorticity {
                vorticity_color(vorticity_at(grid, i, j))
            } else {
                density_color(grid.cells[idx].density)
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

pub fn run_app(mut grid: Grid) -> ! {
    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_title("Simulation - pixels + egui")
        .with_inner_size(winit::dpi::LogicalSize::new(WINDOW_WIDTH as f64, WINDOW_HEIGHT as f64))
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
    let mut boat_ui = BoatUiState::default();
    let mut step: usize = 0;
    let start = Instant::now();

    // État pour le dessin de murs à la souris
    let mut mouse_down = false;
    let mut last_grid_pos: Option<(usize, usize)> = None; // pour le mode "libre"
    let mut line_anchor: Option<(usize, usize)> = None;   // pour le mode "ligne droite"
    let mut last_cursor_pixel: Option<(usize, usize)> = None; // dernière position connue (grille)

    let hole_pos: Vec<usize> = (1..=N as usize).filter(|&x| x % FLOW_SPACE == 0).collect();

    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { event, .. } => {
                let response = egui_winit.on_window_event(&window, &event);
                if response.consumed {
                    // egui a géré l'événement (clic sur un slider, etc.)
                } else if let WindowEvent::CloseRequested = event {
                    elwt.exit();
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
                    //        que le bouton "Step" et "Appliquer bateau" agissent
                    //        dès cette frame) ---
                    let mut do_step = false;
                    let mut apply_boat = false;

                    let raw_input = egui_winit.take_egui_input(&window);
                    let full_output = egui_ctx.run(raw_input, |ctx| {
                        egui::SidePanel::right("controls").show(ctx, |ui| {
                            ui.heading("Paramètres");
                            ui.add(egui::Slider::new(&mut params.flow_velocity, 0.0..=5.0).text("Vitesse flux"));
                            ui.add(egui::Slider::new(&mut params.flow_density, 0.0..=50.0).text("Densité flux"));
                            ui.add(egui::Slider::new(&mut params.viscosity, 0.0..=0.01).text("Viscosité"));
                            ui.separator();
                            ui.add(egui::Slider::new(&mut params.w_drag, 0.0..=5.0).text("Poids traînée"));
                            ui.add(egui::Slider::new(&mut params.w_torque, 0.0..=5.0).text("Poids couple"));

                            ui.separator();
                            ui.checkbox(&mut params.paint_vorticity, "Afficher la vorticité");

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
                            ui.separator();
                            ui.label(format!("Step: {step}  |  t: {:.1}s", start.elapsed().as_secs_f32()));

                            ui.separator();
                            ui.collapsing("Générateur de bateau", |ui| {
                                ui.add(egui::Slider::new(&mut boat_ui.hull_width, 5.0..=100.0).text("Largeur coque"));
                                ui.add(egui::Slider::new(&mut boat_ui.hull_height, 5.0..=200.0).text("Hauteur coque"));
                                ui.add(egui::Slider::new(&mut boat_ui.container_width, 2.0..=40.0).text("Largeur conteneur"));
                                ui.add(egui::Slider::new(&mut boat_ui.container_height, 2.0..=40.0).text("Hauteur conteneur"));
                                ui.add(egui::Slider::new(&mut boat_ui.container_gap, 0.0..=10.0).text("Espacement"));

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

                                ui.add(egui::Slider::new(&mut boat_ui.rows, 1..=8).text("Rangées"));
                                if boat_ui.kind_index != 1 {
                                    ui.add(egui::Slider::new(&mut boat_ui.cols, 1..=8).text("Colonnes"));
                                }

                                if ui.button("Appliquer configuration bateau").clicked() {
                                    apply_boat = true;
                                }
                            });

                            ui.separator();
                            ui.label("Traînée / Couple par objet détecté");
                            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                                let mut ids: Vec<usize> = history.per_object.keys().cloned().collect();
                                ids.sort();
                                for id in ids {
                                    if let Some(h) = history.per_object.get(&id) {
                                        ui.label(format!("Objet #{id}"));
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

                    // --- 2. Application de la configuration bateau, si demandée ---
                    if apply_boat {
                        let kind = match boat_ui.kind_index {
                            0 => ContainerLayoutKind::Grid { rows: boat_ui.rows, cols: boat_ui.cols },
                            1 => ContainerLayoutKind::Pyramid { rows: boat_ui.rows },
                            _ => ContainerLayoutKind::Staggered { rows: boat_ui.rows, cols: boat_ui.cols },
                        };
                        let boat_params = BoatParams {
                            center: (N as isize / 2, N as isize / 2),
                            hull_width: boat_ui.hull_width,
                            hull_height: boat_ui.hull_height,
                            container_width: boat_ui.container_width,
                            container_height: boat_ui.container_height,
                            container_gap: boat_ui.container_gap,
                            kind,
                            flow_angle: 0.0,
                        };
                        let layout = build_boat_layout("custom", &boat_params);
                        grid = Grid::new();
                        grid.apply_layout(&layout);
                        history.clear();
                        step = 0;
                    }

                    // --- 3. Pas de simulation (normal ou manuel via "Step") ---
                    if !params.paused || do_step {
                        grid.initialize_wind_tunnel(params.flow_density, params.flow_velocity, &hole_pos);
                        grid.vel2_step(params.flow_velocity);
                        step += 1;

                        let objects = grid.compute_object_forces();
                        history.push(&objects);
                    }

                    // --- 4. Rendu grille dans le buffer pixels ---
                    draw_grid(&grid, pixels.frame_mut(), grid_w, params.paint_vorticity);

                    // --- 5. Finalisation UI ---
                    egui_winit.handle_platform_output(&window, full_output.platform_output);
                    let clipped_primitives = egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);

                    // --- 6. Rendu combiné pixels + egui ---
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