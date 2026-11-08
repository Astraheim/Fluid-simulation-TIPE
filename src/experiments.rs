//! Reproductibilité, essais et classement multiobjectif (unités numériques).
use crate::{conditions::*, grid::Grid, ship::WallKind};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Band {
    pub x: usize,
    pub first: usize,
    pub last: usize,
    pub kind: Option<WallKind>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub safety: crate::cargo_safety::Safety,
    pub version: u32,
    pub name: String,
    pub n: usize,
    pub dt: f32,
    #[serde(default)]
    pub source: String,
    pub speed: f32,
    pub density: f32,
    pub wind_height: f32,
    pub realistic_wind: bool,
    pub waterline: f32,
    pub warmup: usize,
    pub measure: usize,
    /// Géométrie discrétisée exacte, y compris les dessins manuels et tirages aléatoires.
    pub bands: Vec<Band>,
    pub ui: serde_json::Value,
    #[serde(default)]
    pub hull_size: Option<(f32, f32)>,
}
impl Config {
    pub fn capture(grid: &Grid, labels: &HashMap<usize, WallKind>) -> Self {
        let mut bands = Vec::new();
        for x in 0..=N as usize + 1 {
            let mut y = 0;
            while y <= N as usize + 1 {
                let idx = grid.to_index(x, y);
                if !grid.cells[idx].wall {
                    y += 1;
                    continue;
                }
                let first = y;
                let kind = labels.get(&idx).copied();
                while y < N as usize + 1
                    && grid.cells[grid.to_index(x, y + 1)].wall
                    && labels.get(&grid.to_index(x, y + 1)).copied() == kind
                {
                    y += 1;
                }
                bands.push(Band {
                    x,
                    first,
                    last: y,
                    kind,
                });
                y += 1;
            }
        }
        Self {
            safety: crate::cargo_safety::Safety::default(),
            version: 1,
            source: source_id(),
            name: "Bateau".into(),
            n: N as usize,
            dt: DT,
            speed: 1.0,
            density: FLOW_DENSITY,
            wind_height: grid.wind_min_height,
            realistic_wind: grid.realistic_wind,
            waterline: WATER_LEVEL,
            warmup: 300,
            measure: 600,
            bands,
            ui: serde_json::Value::Null,
            hull_size: None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if !self.safety.valid() {
            return Err("Paramètres de sécurité invalides".into());
        }
        if self.version != 1 || self.n != N as usize || self.dt != DT {
            return Err("Version, grille ou pas de temps incompatible".into());
        }
        if ![self.speed, self.density, self.wind_height, self.waterline]
            .iter()
            .all(|v| v.is_finite())
            || self.speed < 0.0
            || self.density < 0.0
            || !(0.0..=N).contains(&self.wind_height)
            || !(0.0..=N + 1.0).contains(&self.waterline)
            || self.measure < 2
            || self.warmup.checked_add(self.measure).is_none()
        {
            return Err("Paramètres invalides (au moins deux pas de mesure)".into());
        }
        if self
            .hull_size
            .is_some_and(|(w, h)| !w.is_finite() || !h.is_finite() || w < 0.0 || h < 0.0)
        {
            return Err("Dimensions de coque invalides".into());
        }
        let mut occupied = HashSet::new();
        for b in &self.bands {
            if b.x > N as usize + 1 || b.first > b.last || b.last > N as usize + 1 {
                return Err("Coordonnées de mur invalides".into());
            }
            for y in b.first..=b.last {
                if !occupied.insert((b.x, y)) {
                    return Err("Bandes de murs superposées".into());
                }
            }
        }
        Ok(())
    }
    pub fn text(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }
    pub fn parse(text: &str) -> Result<Self> {
        let c: Self = serde_json::from_str(text)?;
        c.validate()?;
        Ok(c)
    }
    pub fn grid(&self) -> Result<(Grid, HashMap<usize, WallKind>)> {
        self.validate()?;
        let mut g = Grid::new();
        let mut labels = HashMap::new();
        for c in &mut g.cells {
            c.wall = false;
        }
        for b in &self.bands {
            for y in b.first..=b.last {
                let i = g.to_index(b.x, y);
                g.cells[i].wall = true;
                if let Some(k) = b.kind {
                    labels.insert(i, k);
                }
            }
        }
        g.init_water(self.waterline);
        g.wind_min_height = self.wind_height;
        g.realistic_wind = self.realistic_wind;
        Ok((g, labels))
    }
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let mut ids = HashSet::new();
        let (mut ship, mut protection, mut all) = (0, 0, 0);
        for b in &self.bands {
            let n = b.last - b.first + 1;
            all += n;
            if let Some(k) = b.kind {
                if k != WallKind::Water {
                    ship += n;
                }
                if matches!(
                    k,
                    WallKind::BowHull
                        | WallKind::SternHull
                        | WallKind::BowFairing
                        | WallKind::SternFairing
                        | WallKind::Support
                ) {
                    protection += n;
                }
                if let WallKind::Container(id) = k {
                    ids.insert(id);
                }
            }
        }
        (ids.len(), protection, ship, all)
    }
    /// Dimensions nominales du corps central, sans proue/poupe ni position.
    pub fn hull_dimensions(&self) -> (f32, f32) {
        if let Some(size) = self.hull_size {
            return size;
        }
        // Compatibilité avec les anciens fichiers de l'interface.
        let boat = &self.ui["boat"];
        if boat["biblio"].as_bool() == Some(true) {
            let (layout, _) = crate::ship::build_biblio_layout();
            let (_, w, h) = layout.hull_rects[0];
            return (w, h);
        }
        if let (Some(w), Some(h)) = (boat["hull_width"].as_f64(), boat["hull_height"].as_f64()) {
            return (w as f32, h as f32);
        }
        // Anciens fichiers sans curseurs : enveloppe des seules cellules Hull.
        let hull: Vec<_> = self
            .bands
            .iter()
            .filter(|b| b.kind == Some(WallKind::Hull))
            .collect();
        if hull.is_empty() {
            return (0.0, 0.0);
        }
        (
            (hull.iter().map(|b| b.x).max().unwrap() - hull.iter().map(|b| b.x).min().unwrap() + 1)
                as f32,
            (hull.iter().map(|b| b.last).max().unwrap()
                - hull.iter().map(|b| b.first).min().unwrap()
                + 1) as f32,
        )
    }
    /// Seuls la vitesse d'entrée et les dimensions centrales séparent les groupes.
    pub fn protocol(&self) -> String {
        serde_json::to_string(&(self.speed + 0.0, self.hull_dimensions())).unwrap()
    }
    pub fn protocol_label(&self) -> String {
        let (w, h) = self.hull_dimensions();
        format!("Coque {w} × {h} · vitesse {}", self.speed)
    }
}

/// Injection commune à l'interface et au mode sans fenêtre : boîte historique conservée.
pub fn inject(grid: &mut Grid, density: f32, speed: f32) {
    let right = 1 + (N as usize / 30).max(2);
    for i in 3..=right.saturating_sub(5) {
        for j in 1..=N as usize {
            if !grid.cells[grid.to_index(i, j)].wall {
                let f = grid.inlet_profile_factor(j);
                grid.cell_init(i, j, speed * f, 0.0, if f > 0.0 { density } else { 0.0 });
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Stats {
    pub n: usize,
    pub mean: f64,
    pub m2: f64,
}
impl Stats {
    pub fn push(&mut self, x: f64) {
        self.n += 1;
        let d = x - self.mean;
        self.mean += d / self.n as f64;
        self.m2 += d * (x - self.mean);
    }
    pub fn sd(&self) -> f64 {
        if self.n < 2 {
            0.0
        } else {
            (self.m2 / (self.n - 1) as f64).max(0.0).sqrt()
        }
    }
}
const METRICS: [&str; 14] = [
    "Fx",
    "Fy",
    "torque",
    "container_drag",
    "pressure_potential_max_abs",
    "speed_max",
    "poisson_residual",
    "poisson_relative_residual",
    "iterations",
    "inlet_flow",
    "outlet_flow",
    "net_outward_flow",
    "step_ms",
    "divergence_max_abs",
];
#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub config: Config,
    pub folder: String,
    pub status: String,
    pub samples: usize,
    pub stats: BTreeMap<String, Stats>,
    pub source: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Weights {
    #[serde(default = "small_bonus")]
    pub shelter: f64,
    #[serde(default = "small_bonus")]
    pub access: f64,
    pub capacity: f64,
    pub drag: f64,
    pub protection: f64,
    pub capacity_ref: f64,
    pub drag_ref: f64,
    pub protection_ref: f64,
}
fn small_bonus() -> f64 {
    0.1
}
impl Default for Weights {
    fn default() -> Self {
        Self {
            shelter: 0.1,
            access: 0.1,
            capacity: 1.0,
            drag: 1.0,
            protection: 1.0,
            capacity_ref: 65.0,
            drag_ref: 1.0,
            protection_ref: 1000.0,
        }
    }
}
impl Weights {
    /// Majorant du score avant simulation : la pénalité de traînée vaut au mieux zéro.
    pub fn optimistic_score(&self, config: &Config) -> f64 {
        let (n, p, _, _) = config.counts();
        self.capacity * n as f64 / self.capacity_ref.max(1.0)
            + self.shelter.max(0.0)
            + self.access * crate::cargo_safety::access(config)
            - self.protection * p as f64 / self.protection_ref.max(1.0)
    }
    pub fn score(&self, e: &Entry) -> f64 {
        let (n, p, _, _) = e.config.counts();
        let sheltered = e
            .stats
            .get("container_load_mean_abs")
            .map_or(0.0, |s| 1.0 / (1.0 + s.mean / self.drag_ref.max(1e-12)));
        self.capacity * n as f64 / self.capacity_ref.max(1.0)
            + self.shelter * sheltered
            + self.access * crate::cargo_safety::access(&e.config)
            - self.drag * e.stats["Fx"].mean.abs() / self.drag_ref.max(1e-12)
            - self.protection * p as f64 / self.protection_ref.max(1.0)
    }
}
pub fn source_id() -> String {
    option_env!("TIPE_SOURCE_ID")
        .unwrap_or("unknown-build")
        .into()
}
pub fn entries(root: &Path) -> Result<Vec<Entry>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for dir in fs::read_dir(root)? {
        let p = dir?.path().join("summary.json");
        if p.is_file() {
            let e: Entry = serde_json::from_slice(&fs::read(&p)?)?;
            if e.status == "completed"
                && e.samples >= 2
                && e.stats.contains_key("Fx")
                && crate::cargo_safety::tiers(&e.config) <= crate::cargo_safety::MAX_TIERS
            {
                out.push(e);
            }
        }
    }
    Ok(out)
}
pub fn save_ranking(root: &Path, entries: &[Entry], weights: &Weights) -> Result<()> {
    fs::create_dir_all(root)?;
    let rows:Vec<_>=entries.iter().map(|e|serde_json::json!({"name":e.config.name,"score":weights.score(e),"containers":e.config.counts().0,"drag":e.stats["Fx"].mean,"protection_cells":e.config.counts().1,"folder":e.folder})).collect();
    fs::write(
        root.join("ranking.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"weights":weights,"rows":rows}))?,
    )?;
    Ok(())
}
pub struct Recorder {
    pub config: Config,
    pub folder: PathBuf,
    pub step: usize,
    stats: BTreeMap<String, Stats>,
    done: bool,
    manifest: String,
    cargo_bounds: BTreeMap<usize, crate::cargo_safety::Bounds>,
    peak_safety_ratio: Option<f64>,
}
impl Recorder {
    pub fn new(config: Config, root: &Path) -> Result<Self> {
        config.validate()?;
        if crate::cargo_safety::tiers(&config) > crate::cargo_safety::MAX_TIERS {
            return Err("Bateau invalide : plus de 10 étages de conteneurs".into());
        }
        fs::create_dir_all(root)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let folder = root.join(format!("run_{stamp}_{}", std::process::id()));
        fs::create_dir(&folder)?;
        fs::write(folder.join("config.json"), config.text()?)?;
        // Une seule copie par contenu technique, partagée par tous les essais.
        let manifest_data = serde_json::to_vec_pretty(&serde_json::json!({
            "source":source_id(), "conditions_rs":include_str!("conditions.rs"),
            "cargo_lock":include_str!("../Cargo.lock"),
            "force_model":"legacy pressure potential + empirical normal drag; not SI",
            "axes":"x right, y down; torque about domain centre, lever arm / N",
            "initial_state":"zero velocities/density, phase from waterline",
            "coefficient_reference":"rho=RHO_AIR; U=input slider; Lref=1 grid domain; Cx,Cy proxies only",
            "pressure_phase":"projection before advection; velocities after complete step"
        }))?;
        let hash = manifest_data.iter().fold(14695981039346656037u64, |h, b| {
            (h ^ *b as u64).wrapping_mul(1099511628211)
        });
        let manifests = root.join("manifests");
        fs::create_dir_all(&manifests)?;
        let filename = format!("build_{hash:016x}.json");
        let target = manifests.join(&filename);
        match File::options().write(true).create_new(true).open(&target) {
            Ok(mut file) => file.write_all(&manifest_data)?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if fs::read(&target)? != manifest_data {
                    return Err("Manifest partagé incompatible".into());
                }
            }
            Err(e) => return Err(e.into()),
        }
        let manifest = format!("../manifests/{filename}");
        let mut r = Self {
            cargo_bounds: crate::cargo_safety::bounds(&config),
            peak_safety_ratio: None,
            config,
            folder,
            step: 0,
            stats: BTreeMap::new(),
            done: false,
            manifest,
        };
        r.summary("running")?;
        Ok(r)
    }
    fn summary(&mut self, status: &str) -> Result<Entry> {
        let e = Entry {
            config: self.config.clone(),
            folder: self.folder.to_string_lossy().into(),
            status: status.into(),
            samples: self.stats.get("Fx").map_or(0, |s| s.n),
            stats: self.stats.clone(),
            source: source_id(),
        };
        let mut document = serde_json::to_value(&e)?;
        document["cargo_assessment"] = serde_json::json!({"max_tiers":crate::cargo_safety::tiers(&self.config),"tier_limit":crate::cargo_safety::MAX_TIERS,"access_proxy":crate::cargo_safety::access(&self.config),"peak_overturning_or_lift_ratio":self.peak_safety_ratio,"force_check":if self.peak_safety_ratio.is_some(){"calibrated_exploratory_model"}else{"not_evaluated_uncalibrated"},"assumptions":"individual unlashed boxes; no ship motions, sliding, support or stack-load validation"});
        document["manifest"] = serde_json::json!(self.manifest);
        document["score"] = serde_json::json!({
            "weights":Weights::default(),
            "value":if e.samples>0 {Some(Weights::default().score(&e))}else{None},
            "formula":"wN*N/Nref - wD*abs(mean(Fx))/Dref - wP*protection_cells/Pref + wS/(1+mean_container_load/Dref) + wA*access_proxy",
            "note":"Default weights; ranking.json records the weights chosen in the menu."
        });
        document["statistics"] = serde_json::to_value(
            self.stats
                .iter()
                .map(|(k, s)| {
                    (
                        k.clone(),
                        serde_json::json!({"mean":s.mean,"std":s.sd(),"samples":s.n}),
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        )?;
        document["steps"] = serde_json::json!(self.step);
        document["time"] = serde_json::json!(self.step as f64 * DT as f64);
        document["counts"] = serde_json::json!({"containers":self.config.counts().0,
            "protection_cells":self.config.counts().1,"ship_wall_cells":self.config.counts().2,
            "total_wall_cells":self.config.counts().3});
        let tmp = self.folder.join("summary.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&document)?)?;
        // Le fichier final n'est jamais lu pendant l'écriture.
        let dest = self.folder.join("summary.json");
        if cfg!(windows) && dest.exists() {
            fs::remove_file(&dest)?;
        }
        fs::rename(tmp, dest)?;
        Ok(e)
    }
    pub fn finish(&mut self, status: &str) -> Result<Entry> {
        let e = self.summary(status)?;
        self.done = true;
        Ok(e)
    }
    pub fn sample(
        &mut self,
        g: &mut Grid,
        labels: &HashMap<usize, WallKind>,
        ms: f64,
    ) -> Result<bool> {
        let forces = g.compute_wall_forces();
        let (cargo_load, ratio) =
            crate::cargo_safety::loads(&self.config, g, labels, &forces, &self.cargo_bounds);
        if !cargo_load.is_finite() || ratio.is_some_and(|r| !r.is_finite()) {
            self.finish("non_finite")?;
            return Err("Effort conteneur non fini".into());
        }
        if let Some(ratio) = ratio {
            self.peak_safety_ratio = Some(self.peak_safety_ratio.unwrap_or(0.0).max(ratio));
            if ratio >= 1.0 {
                self.finish("invalid_cargo_load")?;
                return Err("Bateau invalide : seuil de basculement ou soulèvement atteint (modèle calibré)".into());
            }
        }
        let (mut fx, mut fy, mut torque, mut containers) = (0.0, 0.0, 0.0, 0.0);
        // Ordre déterministe, indépendant de l'ordre aléatoire du HashMap.
        let mut indices: Vec<_> = labels.keys().copied().collect();
        indices.sort_unstable();
        for idx in indices {
            let k = labels[&idx];
            let (x, y) = g.decode_index(idx);
            if k == WallKind::Water || y as f32 >= self.config.waterline {
                continue;
            }
            let f = forces[idx];
            fx += f.x as f64;
            fy += f.y as f64;
            torque +=
                (x as f64 / N as f64 - 0.5) * f.y as f64 - (y as f64 / N as f64 - 0.5) * f.x as f64;
            if matches!(k, WallKind::Container(_)) {
                containers += f.x as f64;
            }
        }
        let (mut pmax, mut vmax, mut dmax) = (0.0_f64, 0.0_f64, 0.0_f64);
        for i in 1..=N as usize {
            for j in 1..=N as usize {
                let c = &g.cells[g.to_index(i, j)];
                if c.wall {
                    continue;
                }
                let v = g.get_velocity_at_center(i, j);
                if ![c.pressure, v.x, v.y].iter().all(|x| x.is_finite()) {
                    self.finish("non_finite")?;
                    return Err("Calcul non fini : essai exclu du classement".into());
                }
                pmax = pmax.max(c.pressure.abs() as f64);
                vmax = vmax.max(v.magnitude() as f64);
                dmax = dmax.max(
                    ((g.get_u(i + 1, j) - g.get_u(i, j) + g.get_v(i, j + 1) - g.get_v(i, j)) * N)
                        .abs() as f64,
                );
            }
        }
        let (mut qi, mut qo, mut qt, mut qb) = (0.0, 0.0, 0.0, 0.0);
        for j in 1..=N as usize {
            qi += g.get_u(1, j) as f64 / N as f64;
            qo += g.get_u(N as usize + 1, j) as f64 / N as f64;
            qt += g.get_v(j, 1) as f64 / N as f64;
            qb += g.get_v(j, N as usize + 1) as f64 / N as f64;
        }
        let values = [
            fx,
            fy,
            torque,
            containers,
            pmax,
            vmax,
            g.projection_residual as f64,
            g.projection_relative_residual as f64,
            g.projection_iterations as f64,
            qi,
            qo,
            qo - qi + qb - qt,
            ms,
            dmax,
        ];
        if !values.iter().all(|v| v.is_finite()) {
            self.finish("non_finite")?;
            return Err("Mesure non finie".into());
        }
        self.step += 1;
        let measure = self.step > self.config.warmup;
        if measure {
            self.stats
                .entry("container_load_mean_abs".into())
                .or_default()
                .push(cargo_load);
        }
        for (k, v) in METRICS.iter().zip(values) {
            if measure {
                self.stats.entry(k.to_string()).or_default().push(v);
            }
        }
        let q = 0.5 * RHO_AIR as f64 * (self.config.speed as f64).powi(2);
        if q > 0.0 {
            if measure {
                self.stats
                    .entry("Cx_proxy".into())
                    .or_default()
                    .push(fx / q);
                self.stats
                    .entry("Cy_proxy".into())
                    .or_default()
                    .push(fy / q);
            }
        }
        if self.step >= self.config.warmup + self.config.measure {
            self.finish("completed")?;
            return Ok(true);
        }
        Ok(false)
    }
}
impl Drop for Recorder {
    fn drop(&mut self) {
        if !self.done {
            if let Err(e) = self.summary("interrupted") {
                eprintln!("Échec sauvegarde essai : {e}");
            }
        }
    }
}

pub fn run(config: Config, root: &Path) -> Result<Entry> {
    let (mut grid, labels) = config.grid()?;
    let mut r = Recorder::new(config, root)?;
    loop {
        let t = Instant::now();
        inject(&mut grid, r.config.density, r.config.speed);
        grid.vel2_step(r.config.speed);
        if r.sample(&mut grid, &labels, t.elapsed().as_secs_f64() * 1000.0)? {
            break;
        }
    }
    r.finish("completed")
}
#[derive(Serialize, Deserialize)]
pub struct Campaign {
    pub output: String,
    pub speeds: Vec<f32>,
    pub boats: Vec<Config>,
}
pub fn default_campaign() -> Campaign {
    let (layout, profile) = crate::ship::build_biblio_layout();
    let (center, width, height) = layout.hull_rects[0];
    let waterline = center.1 as f32 - height / 2.0 + height * profile.freeboard_ratio;
    let mut boats = Vec::new();
    for protected in [false, true] {
        for (name, fraction) in [
            ("Biblio complet", 1.0),
            ("Biblio 2 tiers", 0.66),
            ("Biblio 1 tiers", 0.33),
        ] {
            let mut l = layout.clone();
            let mut ids = l.containers.clone();
            ids.sort_by_key(|c| std::cmp::Reverse(c.center.1));
            let keep = (ids.len() as f32 * fraction).round() as usize;
            let wanted: HashSet<_> = ids.iter().take(keep).map(|c| c.id).collect();
            l.containers.retain(|c| wanted.contains(&c.id));
            if protected {
                let first = l
                    .containers
                    .iter()
                    .map(|c| c.center.0 - (c.width / 2.0) as isize)
                    .min()
                    .unwrap();
                let deck = center.1 - (height / 2.0) as isize;
                l.fairings.push(crate::ship::Fairing {
                    position: (first - 25, deck),
                    length: 25.0,
                    half_height: 30.0,
                    side: crate::ship::FairingSide::Bow,
                    profile: crate::ship::FairingProfile::Triangular,
                    height_scale: 1.0,
                });
            }
            let mut grid = Grid::new();
            let labels = grid.apply_layout_with_profile(&l, &profile);
            grid.setup_wind_tunnel_walls(
                &(1..=N as usize)
                    .filter(|j| j % FLOW_SPACE == 0)
                    .collect::<Vec<_>>(),
            );
            let mut c = Config::capture(&grid, &labels);
            c.hull_size = Some((width, height));
            c.name = format!("{}{}", name, if protected { " + protection" } else { "" });
            c.waterline = waterline;
            c.wind_height = 260.0;
            c.realistic_wind = false;
            boats.push(c);
        }
    }
    Campaign {
        output: "results".into(),
        speeds: vec![0.6, 1.0],
        boats,
    }
}
pub fn cli() -> bool {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 2 {
        return false;
    }
    let work = || -> Result<()> {
        match args[1].as_str(){
        "--init-campaign"=>{let p=args.get(2).ok_or("Chemin JSON manquant")?;let mut f=File::options().write(true).create_new(true).open(p)?;f.write_all(&serde_json::to_vec_pretty(&default_campaign())?)?;},
        "--run"=>{let c=Config::parse(&fs::read_to_string(args.get(2).ok_or("Configuration manquante")?)?)?;let e=run(c,Path::new(args.get(3).map_or("results",String::as_str)))?;println!("{} : {}",e.config.name,e.folder);},
        "--batch"=>{let c:Campaign=serde_json::from_slice(&fs::read(args.get(2).ok_or("Campagne manquante")?)?)?;for b in &c.boats{b.validate()?;}for speed in c.speeds{if !speed.is_finite()||speed<0.0{return Err("Vitesse invalide".into());}for b in &c.boats{let mut b=b.clone();b.speed=speed;println!("Essai {} ; vitesse {}",b.name,speed);let e=run(b,Path::new(&c.output))?;println!("Enregistré : {}",e.folder);}}},
        _=>return Err("Utilisation : --run config.json [results] | --batch campagne.json | --init-campaign campagne.json".into())
    }
        Ok(())
    };
    if let Err(e) = work() {
        eprintln!("{e}");
        std::process::exit(1);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stats_known() {
        let mut s = Stats::default();
        for x in [1., 2., 3.] {
            s.push(x);
        }
        assert_eq!(s.mean, 2.);
        assert_eq!(s.sd(), 1.);
    }
    #[test]
    fn config_roundtrip() {
        let c = default_campaign().boats.remove(0);
        let d = Config::parse(&c.text().unwrap()).unwrap();
        let (g, l) = d.grid().unwrap();
        assert_eq!(Config::capture(&g, &l).bands, c.bands);
        assert!(c.counts().0 > 0);
    }
    #[test]
    fn invalid_import() {
        let mut c = default_campaign().boats.remove(0);
        c.bands[0].last = N as usize + 2;
        assert!(c.validate().is_err());
    }
    #[test]
    fn campaign_same_hull() {
        let c = default_campaign();
        assert_eq!(c.boats[0].protocol(), c.boats[1].protocol());
        assert!(c.boats[0].counts().0 > c.boats[1].counts().0);
    }
    #[test]
    fn results_and_replay() {
        let root = std::env::temp_dir().join(format!(
            "tipe-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut c = Config::capture(&Grid::new(), &HashMap::new());
        c.speed = 0.0;
        c.warmup = 1;
        c.measure = 2;
        let a = run(c.clone(), &root).unwrap();
        let b = run(c.clone(), &root).unwrap();
        assert_eq!(a.samples, 2);
        assert_eq!(fs::read_dir(root.join("manifests")).unwrap().count(), 1);
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(Path::new(&a.folder).join("summary.json")).unwrap())
                .unwrap();
        assert!(
            Path::new(&a.folder)
                .join(saved["manifest"].as_str().unwrap())
                .is_file()
        );
        let files: HashSet<_> = fs::read_dir(&a.folder)
            .unwrap()
            .map(|p| p.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(
            files,
            ["config.json", "summary.json"]
                .into_iter()
                .map(String::from)
                .collect()
        );
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(Path::new(&a.folder).join("summary.json")).unwrap())
                .unwrap();
        assert_eq!(
            document["score"]["value"].as_f64(),
            Some(Weights::default().score(&a))
        );
        assert_eq!(
            document["statistics"]["Fx"]["std"].as_f64(),
            Some(a.stats["Fx"].sd())
        );
        // Les anciennes synthèses sans champ score restent chargeables.
        let legacy = serde_json::to_value(&a).unwrap();
        assert!(serde_json::from_value::<Entry>(legacy).is_ok());
        assert!(!a.stats.contains_key("Cx_proxy"));
        for (k, v) in &a.stats {
            if k != "step_ms" {
                assert_eq!(v.mean, b.stats[k].mean);
                assert_eq!(v.m2, b.stats[k].m2);
            }
        }
        {
            let mut r = Recorder::new(c, &root).unwrap();
            r.finish("interrupted").unwrap();
        }
        assert_eq!(entries(&root).unwrap().len(), 2);
        let w = Weights::default();
        let mut worse = a.clone();
        worse.stats.get_mut("Fx").unwrap().mean = 1.0;
        assert!(w.score(&a) > w.score(&worse));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn hull_ends_count_as_protections_without_splitting_rankings() {
        let (mut layout, profile) = crate::ship::build_biblio_layout();
        let mut g = Grid::new();
        let labels = g.apply_layout_with_profile(&layout, &profile);
        let a = Config::capture(&g, &labels);
        layout.hull_curve.as_mut().unwrap().bow_length *= 0.5;
        layout.hull_curve.as_mut().unwrap().stern_length *= 0.5;
        let mut g = Grid::new();
        let labels = g.apply_layout_with_profile(&layout, &profile);
        let b = Config::capture(&g, &labels);
        assert_eq!(a.protocol(), b.protocol());
        assert!(a.counts().1 > b.counts().1);
        let ends: usize = a
            .bands
            .iter()
            .filter(|b| matches!(b.kind, Some(WallKind::BowHull | WallKind::SternHull)))
            .map(|b| b.last - b.first + 1)
            .sum();
        assert_eq!(a.counts().1, ends);
    }
    #[test]
    fn ranking_uses_only_speed_and_central_dimensions() {
        let mut a = default_campaign().boats.remove(0);
        a.hull_size = Some((250.0, 30.0));
        let mut b = a.clone();
        b.source = "updated-dependencies-and-compiler".into();
        b.n += 1;
        b.dt *= 2.0;
        b.density *= 2.0;
        b.wind_height += 10.0;
        b.realistic_wind = !b.realistic_wind;
        b.waterline += 1.0;
        b.warmup += 100;
        b.measure += 100;
        b.ui = serde_json::json!({"boat":{"container_width":99,"container_height":99}});
        b.bands.clear();
        assert_eq!(a.protocol(), b.protocol());
        b.speed += 0.1;
        assert_ne!(a.protocol(), b.protocol());
        b.speed = a.speed;
        for dimensions in [(251.0, 30.0), (250.0, 31.0)] {
            b.hull_size = Some(dimensions);
            assert_ne!(a.protocol(), b.protocol());
        }
    }
    #[test]
    fn legacy_configs_keep_their_group_without_rewriting_files() {
        let mut c = default_campaign().boats.remove(0);
        c.hull_size = Some((250.0, 30.0));
        c.ui = serde_json::json!({"boat":{"hull_width":250,"hull_height":30,"biblio":false}});
        let mut old = serde_json::to_value(&c).unwrap();
        old.as_object_mut().unwrap().remove("hull_size");
        let loaded = Config::parse(&old.to_string()).unwrap();
        assert_eq!(c.protocol(), loaded.protocol());
        assert_eq!(c.bands, loaded.bands);
    }
    #[test]
    fn random_filter_uses_score_bound_and_current_group() {
        let mut winner = default_campaign().boats.remove(0);
        winner.hull_size = Some((250.0, 30.0));
        let mut stats = BTreeMap::new();
        let mut fx = Stats::default();
        fx.push(0.0);
        fx.push(0.0);
        stats.insert("Fx".into(), fx);
        let mut panel = Panel::default();
        panel.entries = vec![Entry {
            config: winner.clone(),
            folder: String::new(),
            status: "completed".into(),
            samples: 2,
            stats,
            source: "old-build".into(),
        }];
        let mut bad = winner.clone();
        bad.bands
            .retain(|b| !matches!(b.kind, Some(WallKind::Container(_))));
        assert!(!panel.candidate_can_win(&bad));
        bad.speed += 1.0;
        assert!(panel.candidate_can_win(&bad)); // aucune référence pour ce vent
        let mut promising = winner.clone();
        promising
            .bands
            .retain(|b| !matches!(b.kind, Some(WallKind::BowHull | WallKind::SternHull)));
        assert!(panel.candidate_can_win(&promising));
        assert!(panel.weights.optimistic_score(&winner) >= panel.weights.score(&panel.entries[0]));
        // Une forte traînée mesurée laisse sa chance à un bateau moins chargé.
        panel.entries[0].stats.get_mut("Fx").unwrap().mean = 100.0;
        bad.speed = winner.speed;
        assert!(panel.candidate_can_win(&bad));
    }
    #[test]
    fn zero_residual() {
        let mut g = Grid::new();
        g.project();
        assert_eq!(g.projection_residual, 0.0);
        assert_eq!(g.projection_iterations, 1);
    }
}

pub enum Action {
    Export,
    Save,
    Import,
    Start,
    RandomStart,
    Stop,
    Load(Config),
}
pub struct Panel {
    pub text: String,
    pub code_expanded: bool,
    pub rankings_open: bool,
    pub name: String,
    pub status: String,
    pub root: String,
    pub warmup: usize,
    pub measure: usize,
    pub weights: Weights,
    pub safety: crate::cargo_safety::Safety,
    pub entries: Vec<Entry>,
    pub protocol: String,
    pub recorder: Option<Recorder>,
    pub saved: Vec<(String, Config)>,
}
impl Default for Panel {
    fn default() -> Self {
        let mut p = Self {
            text: String::new(),
            code_expanded: false,
            rankings_open: false,
            name: "Mon bateau".into(),
            status: String::new(),
            root: "results".into(),
            warmup: 300,
            measure: 600,
            weights: Weights::default(),
            safety: crate::cargo_safety::Safety::default(),
            entries: Vec::new(),
            protocol: String::new(),
            recorder: None,
            saved: Vec::new(),
        };
        if let Ok(data) = fs::read("results/ranking.json") {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&data) {
                if let Ok(w) = serde_json::from_value(v["weights"].clone()) {
                    p.weights = w;
                }
            }
        }
        p.refresh();
        p
    }
}
impl Panel {
    pub fn candidate_can_win(&self, c: &Config) -> bool {
        // Les poids négatifs rendraient ce majorant invalide.
        if self.weights.drag < 0.0 {
            return true;
        }
        let key = c.protocol();
        let best = self
            .entries
            .iter()
            .filter(|e| e.config.protocol() == key)
            .map(|e| self.weights.score(e))
            .reduce(f64::max);
        best.is_none_or(|score| self.weights.optimistic_score(c) > score)
    }
    pub fn refresh(&mut self) {
        match entries(Path::new(&self.root)) {
            Ok(e) => {
                self.entries = e;
                self.status = format!("{} essais terminés", self.entries.len());
            }
            Err(e) => self.status = e.to_string(),
        }
        self.saved.clear();
        if let Ok(dirs) = fs::read_dir(Path::new(&self.root).join("boats")) {
            for d in dirs.flatten() {
                if let Ok(s) = fs::read_to_string(d.path()) {
                    if let Ok(c) = Config::parse(&s) {
                        self.saved.push((c.name.clone(), c));
                    }
                }
            }
        }
        self.saved.sort_by(|a, b| a.0.cmp(&b.0));
    }
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        egui::CollapsingHeader::new("Bateaux sauvegardés / classement").default_open(true).show(ui,|ui|{
            ui.horizontal(|ui|{ui.label("Nom");ui.text_edit_singleline(&mut self.name);});
            ui.horizontal(|ui|{if ui.button("Copier caractéristiques").clicked(){action=Some(Action::Export);}if ui.button("Sauvegarder bateau").clicked(){action=Some(Action::Save);}});
            ui.toggle_value(&mut self.code_expanded, "Afficher / masquer le code du bateau");
            if self.code_expanded {
                ui.label("Colle ici le code d’un bateau, puis Charger :");
                egui::ScrollArea::vertical().id_source("boat_code_scroll").max_height(120.0).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut self.text).desired_rows(3).desired_width(f32::INFINITY));
                });
                if ui.button("Charger le code (depuis zéro)").clicked(){action=Some(Action::Import);}
            }
            egui::ComboBox::from_id_source("saved_boats").selected_text("Charger un bateau sauvegardé…").show_ui(ui,|ui|{for(name,c)in &self.saved{if ui.selectable_label(false,name).clicked(){action=Some(Action::Load(c.clone()));}}});
            ui.separator();ui.label("Essai reproductible — tunnel de vent");
            egui::CollapsingHeader::new("Conteneurs : stabilité et hypothèses").show(ui,|ui| {
                ui.label("10 étages maximum. Modèle individuel sans arrimage, glissement ni mouvements du navire.");
                ui.add_enabled_ui(self.recorder.is_none(), |ui| {
                    ui.add(egui::DragValue::new(&mut self.safety.mass_kg).clamp_range(1.0..=100000.0).prefix("Masse par conteneur (kg) : "));
                    ui.add(egui::DragValue::new(&mut self.safety.base_m).clamp_range(0.1..=30.0).prefix("Base dans le sens du vent (m) : "));
                    ui.add(egui::DragValue::new(&mut self.safety.height_m).clamp_range(0.1..=10.0).prefix("Hauteur (m) : "));
                    ui.add(egui::DragValue::new(&mut self.safety.newtons_per_unit).clamp_range(0.0..=1e12).prefix("Calibration N/unité (0 = inconnue) : "));
                });
                ui.small("15 t est une hypothèse de chargement, pas une masse moyenne mesurée. Base 6,058 m : vent longitudinal sur un 20 pieds.");
                ui.label(format!("Seuil théorique, force horizontale à mi-hauteur : {:.0} N",self.safety.centre_force_limit()));
                if self.safety.newtons_per_unit==0.0 {ui.colored_label(egui::Color32::YELLOW,"Basculement non évalué : forces numériques non calibrées.");}
            });
            ui.add(egui::DragValue::new(&mut self.warmup).clamp_range(0..=1_000_000).prefix("Stabilisation : ").suffix(" pas"));
            ui.add(egui::DragValue::new(&mut self.measure).clamp_range(2..=1_000_000).prefix("Mesure : ").suffix(" pas"));
            if let Some(r)=&self.recorder{ui.label(format!("Essai : {} / {} pas",r.step,r.config.warmup+r.config.measure));if ui.button("Arrêter (hors classement)").clicked(){action=Some(Action::Stop);}}
            else {
                if ui.button("Mesurer et classer (redémarre à zéro)").clicked(){action=Some(Action::Start);}
                if ui.button("🎲 Tester et classer un bateau aléatoire").clicked(){action=Some(Action::RandomStart);}
            }
            ui.small("Essai aléatoire : au moins 50 conteneurs ; jusqu’à 64 tirages pour éviter un candidat incapable de battre le meilleur score, même sans traînée.");
            ui.small("Le calcul conserve le solveur actuel. Score exploratoire, pas une validation physique.");
            if ui.button("🏆 Classements").clicked() { self.refresh(); self.rankings_open = true; }
            ui.label(&self.status);
        });
        let mut open = self.rankings_open;
        egui::Window::new("Classements des bateaux").open(&mut open)
            .default_width(620.0).default_height(500.0).vscroll(true).show(ui.ctx(), |ui| {
            ui.separator();
            ui.label("Score = capacité − traînée − protections + abri + accès");
            for(label,v)in [("Poids capacité",&mut self.weights.capacity),("Poids traînée",&mut self.weights.drag),("Poids protections",&mut self.weights.protection),("Bonus abri",&mut self.weights.shelter),("Bonus accès",&mut self.weights.access)]{ui.add(egui::Slider::new(v,0.0..=10.0).text(label));}
            egui::CollapsingHeader::new("Échelles du score / dossier").show(ui,|ui|{
                ui.add(egui::DragValue::new(&mut self.weights.capacity_ref).clamp_range(1.0..=1e6).prefix("Conteneurs de référence : "));
                ui.add(egui::DragValue::new(&mut self.weights.drag_ref).clamp_range(1e-6..=1e6).speed(0.01).prefix("Traînée de référence : "));
                ui.add(egui::DragValue::new(&mut self.weights.protection_ref).clamp_range(1.0..=1e6).prefix("Cellules de protection de référence : "));
                ui.text_edit_singleline(&mut self.root);
            });
            ui.small("Coût estimé = proue + poupe + carénages + supports (surface 2D), pas un prix en euros. Couple conservé dans les résultats.");
            if ui.button("Actualiser les résultats").clicked(){self.refresh();}
            let mut protocols:Vec<(String,String)>=Vec::new();
            for e in &self.entries {let key=e.config.protocol();if !protocols.iter().any(|(k,_)|*k==key){protocols.push((key,e.config.protocol_label()));}}
            protocols.sort_by(|a,b| a.1.cmp(&b.1));
            if protocols.is_empty() { ui.label("Aucun essai terminé. Lance une mesure pour créer un classement."); }
            if !protocols.iter().any(|(k,_)|*k==self.protocol){self.protocol=protocols.first().map_or(String::new(),|p|p.0.clone());}
            egui::CollapsingHeader::new("Tous les classements").default_open(true).show(ui, |ui| {
                for (key, label) in &protocols {
                    let count = self.entries.iter().filter(|e| e.config.protocol() == *key).count();
                    ui.selectable_value(&mut self.protocol, key.clone(), format!("{label} · {count} bateaux"));
                }
            });
            ui.small("Groupes séparés uniquement par dimensions de coque et vitesse d’entrée. Plus haut = meilleur.");
            let mut ranked:Vec<_>=self.entries.iter().filter(|e|e.config.protocol()==self.protocol).collect();
            ranked.sort_by(|a,b|self.weights.score(b).total_cmp(&self.weights.score(a)));
            if ui.button("Enregistrer ce classement / ces poids").clicked(){let rows:Vec<_>=ranked.iter().map(|e|(*e).clone()).collect();self.status=match save_ranking(Path::new(&self.root),&rows,&self.weights){Ok(())=>"Classement enregistré dans ranking.json".into(),Err(e)=>e.to_string()};}
            egui::ScrollArea::horizontal().show(ui, |ui| {
                egui::Grid::new("ranking_table").striped(true).show(ui, |ui| {
                    for title in ["Rang", "Bateau", "Score", "Conteneurs", "Traînée", "Protections", "Stabilité", "Actions"] { ui.strong(title); }
                    ui.end_row();
                    for (rank,e) in ranked.iter().enumerate() {
                        let (n,p,_,_) = e.config.counts();
                        let drag = &e.stats["Fx"];
                        ui.label(format!("#{}",rank+1));
                        ui.label(&e.config.name).on_hover_text(&e.folder);
                        ui.label(format!("{:.4}",self.weights.score(e)));
                        ui.label(n.to_string());
                        ui.label(format!("{:.5}",drag.mean)).on_hover_text(format!("Écart type : {:.5}",drag.sd()));
                        ui.label(p.to_string());
                        ui.label(if e.config.safety.newtons_per_unit>0.0 && e.stats.contains_key("container_load_mean_abs") {"Vérifiée (modèle)"}else{"Non évaluée"});
                        ui.push_id(rank, |ui| { ui.horizontal(|ui| {
                            if ui.button("Charger").clicked() { action=Some(Action::Load(e.config.clone())); }
                            if ui.button("Copier").clicked() {
                                if let Ok(t)=e.config.text() { self.text=t.clone();self.code_expanded=false;ui.output_mut(|o|o.copied_text=t); }
                            }
                        }); });
                        ui.end_row();
                    }
                });
            });
            });
        self.rankings_open = open;
        action
    }
    pub fn save(&mut self, c: &Config) -> Result<()> {
        let root = Path::new(&self.root).join("boats");
        fs::create_dir_all(&root)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let p = root.join(format!("boat_{stamp}.json"));
        fs::write(&p, c.text()?)?;
        self.refresh();
        self.status = format!("Sauvegardé : {}", p.display());
        Ok(())
    }
}
