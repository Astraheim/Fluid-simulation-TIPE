use crate::conditions::*;
use crate::grid::Grid;
use egui::Context;
use egui_wgpu::{Renderer as EguiRenderer, ScreenDescriptor};
use pixels::{Pixels, SurfaceTexture};
use pixels::wgpu; // réexporté par pixels : garantit la même version que celle utilisée en interne
use std::sync::Arc;
use std::time::Instant;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

/*
Remplace run_simulation() de visualization.rs.

Architecture :
    - winit : fenêtre + boucle d'événements
    - pixels : buffer de pixels GPU-accéléré (même principe que le Vec<u32> de
      minifb, mais le scaling DX/DY se fait sur le GPU -> on peut rendre à la
      résolution de la grille N x N et laisser pixels agrandir, au lieu de
      remplir les blocs DX*DY à la main comme avant)
    - egui + egui-wgpu : UI immédiate par-dessus la texture pixels, dans la
      même passe de rendu wgpu (pixels expose son wgpu::Device/Queue)

Note : les valeurs "réglables en direct" vivent dans SimParams, PAS dans les
const de conditions.rs (celles-ci restent la config de démarrage / la
structure figée de la grille : N, DX, DY, SIZE).
*/

/// Paramètres physiques modifiables sans recréer la grille.
pub struct SimParams {
    pub flow_velocity: f32,
    pub flow_density: f32,
    pub viscosity: f32,
    pub w_drag: f32,
    pub w_torque: f32,
    pub paused: bool,
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
        }
    }
}

/// Historique glissant pour le plot temps réel (traînée / couple)
pub struct ForceHistory {
    pub drag: Vec<[f64; 2]>,   // [step, valeur]
    pub torque: Vec<[f64; 2]>,
    pub step: usize,
    pub max_points: usize,
}

impl ForceHistory {
    fn new(max_points: usize) -> Self {
        Self { drag: Vec::new(), torque: Vec::new(), step: 0, max_points }
    }

    fn push(&mut self, drag: f32, torque: f32) {
        self.drag.push([self.step as f64, drag as f64]);
        self.torque.push([self.step as f64, torque as f64]);
        if self.drag.len() > self.max_points {
            self.drag.remove(0);
            self.torque.remove(0);
        }
        self.step += 1;
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

/// Rendu de la grille dans le buffer pixels, à résolution N x N (pas DX*DY,
/// c'est pixels/wgpu qui agrandit à l'affichage -> plus de boucle de blocs).
fn draw_grid(grid: &Grid, frame: &mut [u8], grid_w: usize) {
    let n_max = N as usize + 1;
    for j in 1..n_max {
        for i in 1..n_max {
            let idx = grid.to_index(i, j);
            let px = (j * grid_w + i) * 4;
            let color = if grid.cells[idx].wall {
                [0, 0, 0, 255]
            } else {
                density_color(grid.cells[idx].density)
            };
            frame[px..px + 4].copy_from_slice(&color);
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
    let window = Arc::new(window); // Arc possédé (Send+Sync requis par wgpu::WindowHandle) -> Pixels 'static

    let grid_w = N as usize + 2;
    let grid_h = N as usize + 2;

    // IMPORTANT : taille PHYSIQUE réelle de la fenêtre, pas les constantes
    // logiques WINDOW_WIDTH/HEIGHT -- sur un écran avec mise à l'échelle
    // Windows != 100%, les deux diffèrent, et pixels/egui ont besoin de la
    // taille physique pour configurer correctement la surface et les zones
    // de clip d'egui (sinon egui peut se retrouver rendu hors de la zone
    // visible alors que la grille, elle, s'affiche normalement).
    let win_size = window.inner_size();

    // On passe un CLONE possédé de l'Arc (pas une référence) : c'est ce qui donne
    // un Pixels<'static> non lié à la durée de vie de `window`, et permet de
    // déplacer `window` (l'Arc, pas cloné) dans la closure de l'event loop
    // sans conflit d'emprunt.
    let surface_texture = SurfaceTexture::new(win_size.width, win_size.height, Arc::clone(&window));
    let mut pixels = Pixels::new(grid_w as u32, grid_h as u32, surface_texture).unwrap();

    // --- Setup egui ---
    let mut egui_ctx = Context::default();
    let mut egui_winit = egui_winit::State::new(egui_ctx.clone(), egui::ViewportId::ROOT, &window, None, None);
    let mut egui_renderer = EguiRenderer::new(pixels.device(), pixels.render_texture_format(), None, 1);

    let mut params = SimParams::default();
    let mut history = ForceHistory::new(500);
    let mut step: usize = 0;
    let start = Instant::now();

    // État pour le dessin de murs à la souris (repris de l'ancien run_simulation)
    let mut mouse_down = false;
    let mut last_grid_pos: Option<(usize, usize)> = None;

    let hole_pos: Vec<usize> = (1..=N as usize).filter(|&x| x % FLOW_SPACE == 0).collect();

    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { event, .. } => {
                let response = egui_winit.on_window_event(&window, &event);
                if response.consumed {
                    // egui a géré l'événement (clic sur un slider, etc.) : pas de dessin de mur
                } else if let WindowEvent::CloseRequested = event {
                    elwt.exit();
                } else if let WindowEvent::MouseInput { state, button, .. } = event {
                    if button == winit::event::MouseButton::Left {
                        mouse_down = state == winit::event::ElementState::Pressed;
                        if !mouse_down {
                            last_grid_pos = None; // fin du tracé, prochaine pression = nouveau trait
                        }
                    }
                } else if let WindowEvent::CursorMoved { position, .. } = event {
                    if mouse_down {
                        // Conversion position souris (physique) -> coordonnées grille.
                        // Le buffer pixels est rendu à résolution N x N (cf. draw_grid),
                        // donc pixel buffer == coordonnées grille directement.
                        if let Ok((gx, gy)) = pixels.window_pos_to_pixel((position.x as f32, position.y as f32)) {
                            if gx >= 1 && gx <= N as usize && gy >= 1 && gy <= N as usize {
                                let idx = grid.to_index(gx, gy);
                                grid.cells[idx].wall = true;

                                // Trace un trait entre la dernière position et la position
                                // actuelle si la souris a bougé vite (drag), comme avant.
                                if let Some((lx, ly)) = last_grid_pos {
                                    for (x, y) in bresenham_line(lx, ly, gx, gy) {
                                        if x >= 1 && x <= N as usize && y >= 1 && y <= N as usize {
                                            let idx = grid.to_index(x, y);
                                            grid.cells[idx].wall = true;
                                        }
                                    }
                                }
                                last_grid_pos = Some((gx, gy));
                            }
                        }
                    }
                } else if let WindowEvent::RedrawRequested = event {
                    // --- 1. Step physique (si pas en pause) ---
                    if !params.paused {
                        grid.initialize_wind_tunnel(params.flow_density, params.flow_velocity, &hole_pos);
                        grid.vel2_step(params.flow_velocity);
                        step += 1;

                        // Exemple : couple/traînée globaux pour le plot
                        let objects = grid.compute_object_forces();
                        let total_torque: f32 = objects.iter().map(|o| o.torque.abs()).sum();
                        let total_drag: f32 = objects.iter().map(|o| o.total_force.x).sum();
                        history.push(total_drag, total_torque);
                    }

                    // --- 2. Rendu grille dans le buffer pixels ---
                    draw_grid(&grid, pixels.frame_mut(), grid_w);

                    // --- 3. UI egui ---
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
                            if ui.button(if params.paused { "Reprendre" } else { "Pause" }).clicked() {
                                params.paused = !params.paused;
                            }
                            if ui.button("Screenshot").clicked() {
                                save_screenshot(pixels.frame(), grid_w, grid_h, step);
                            }
                            ui.separator();
                            ui.label(format!("Step: {step}  |  t: {:.1}s", start.elapsed().as_secs_f32()));

                            ui.separator();
                            ui.label("Traînée / Couple (temps réel)");
                            egui_plot::Plot::new("forces_plot")
                                .height(200.0)
                                .show(ui, |plot_ui| {
                                    plot_ui.line(egui_plot::Line::new(egui_plot::PlotPoints::from(history.drag.clone())).name("Traînée"));
                                    plot_ui.line(egui_plot::Line::new(egui_plot::PlotPoints::from(history.torque.clone())).name("Couple"));
                                });
                        });
                    });

                    egui_winit.handle_platform_output(&window, full_output.platform_output);
                    let clipped_primitives = egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);

                    // --- 4. Rendu combiné pixels + egui ---
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

/// Reprend bresenham_line de visualization.rs (dessin de murs entre deux points de la souris)
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
