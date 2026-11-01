//! **Port-only: frame smoothing** (the Port Options' "Frame smoothing", crate::graphics::FrameSmoothing;
//! docs/plan/graphics_options.md). The game ticks at 60 Hz (crate::frame_pace), so a faster display showed each tick's
//! picture two or more times. With the option On and frames coming faster than the ticks, each frame draws the world
//! between the last two ticks at `alpha` = the time since the last tick, in ticks (crate::frame_pace's remainder, the
//! accumulator after the frame's ticks are taken): what is drawn lags the newest tick by `1 − alpha` ticks, at most one
//! tick (16.7 ms), half a tick on average. Off, and On at or below the tick rate, nothing here runs.
//!
//! **Game state is not touched.** Every system of the frame (the ticks, the culls and LOD picks, the moby +0x31
//! write-back, the particles' drawn flag, the camera the next tick's `FastBSphereCheck` and 0x167240 read) sees the
//! tick's values. Only in `Last`, after everything else ([`blend`]), are the render inputs overwritten with the
//! interpolated ones, and `First` ([`restore`]) puts the tick's back before anything runs.
//!
//! * **Camera**: the main camera's `GlobalTransform` and the cameras that copy it (the sky's): translation lerped,
//!   rotation slerped between the transforms of the last two ticks. A camera that moved on a frame without a tick
//!   (the fly camera) is drawn as it is.
//! * **Mobys** (all of them: statics, dynamic slots, Ratchet and his attachments, scene and vendor mobys): every
//!   storage buffer a `MobyMaterial` / `MobyMetalMaterial` draws with, found through the materials. The `MobyInst`
//!   records ([`blend_records`]: model translation lerped, its columns lerped and brought back to their lerped
//!   lengths, the light block lerped) and the joint palettes ([`blend_palette`]: each matrix lerped, i.e. the pose).
//! * **Particles** (crate::particle_render, [`ParticleSmooth`]): each live pool record's position (and a ribbon's
//!   second end) lerped from the previous tick's. Sizes, fades and culls stay the tick's.
//! * **Not interpolated** (they step at the tick rate): the HUD and menus, the moby shadow volumes
//!   (crate::shadow_render) and blob shadows, the sky shells' spin and the stars, water and sea surfaces, shrub sway,
//!   UV scrolls and texture animation, the effect quads (crate::fx_draw: glows, beams, unit quads; the flight's canopy glass is, crate::flight_render::glass_matrix), the entities' sort
//!   keys and Bevy's frustum culls (the tick's view). Docs/plan/gaps.md G-REN-150.
//!
//! **Discontinuities** draw the tick's value: a camera that moves more than [`CAMERA_CUT`] units or turns more than
//! [`CUT_ANGLE`] in one tick (scene cuts, respawns, snaps); a moby record whose palette base or colour mode changed (a
//! new object in a dynamic slot), that moved more than [`MOBY_CUT`] units, or whose axes turned more than
//! [`CUT_ANGLE`] or changed length by half; a joint matrix that turned more than [`CUT_ANGLE`], lost or gained its
//! scale, or comes from or goes to the identity (a moby made visible); every joint of a record that was cut; a
//! particle record dead at the previous tick or moved more than [`PARTICLE_CUT`]; a buffer that changed size or was
//! written on a frame without a tick; everything at a level change (crate::level_switch::LevelGeneration).
//!
//! Frame-exact runs (crate::determinism) interpolate only with `RC_DUMP_TICKS` giving fewer ticks than updates: alpha
//! = updates since the last tick × the pattern's ticks per update. `RC_FRAME_SMOOTH_DEBUG=1` logs every second the
//! frames, ticks, the alpha range and what was skipped, and each camera cut; `=2` also one line per frame (ticks,
//! alpha, the camera drawn, in game units). `RC_FRAME_SMOOTH_HZ=<h>` renders at h frames a second without vsync
//! (crate::frame_pace), to try the option on a 60 Hz screen.

use crate::frame_pace::FrameLimit;
use crate::graphics::FrameSmoothing;
use crate::moby_render::{MobyMaterial, MobyMetalMaterial};
use bevy::prelude::*;
use bevy::render::storage::ShaderBuffer;
use std::collections::{HashMap, HashSet};

/// The most a camera moves in one tick (units) before it counts as a cut.
pub const CAMERA_CUT: f32 = 8.0;
/// The most a moby moves in one tick (units) before it counts as a teleport.
pub const MOBY_CUT: f32 = 8.0;
/// The most a particle moves in one tick (units) before it counts as a new one.
pub const PARTICLE_CUT: f32 = 4.0;
/// The most a camera, a moby or a joint turns in one tick before it counts as a cut (45°).
pub const CUT_ANGLE: f32 = std::f32::consts::FRAC_PI_4;
/// The tick rate (crate::determinism).
const TICK_HZ: f64 = crate::determinism::TICK_HZ;
/// `MobyInst` (crate::moby_render): 208 bytes = 52 words; model 0..16, light block 16..48, misc 48..52 (palette base,
/// colour mode, CPU colour base, UV scroll).
const RECORD_WORDS: usize = 52;
const MISC: usize = 48;
/// A palette matrix: 16 f32, column i = joint row i (crate::moby_anim).
const JOINT_WORDS: usize = 16;

/// The frame's place between two ticks: the ticks it ran, and while smoothing is active the fraction of a tick since
/// the last one (written in `First` by crate::frame_pace, or by [`frame_exact_phase`]).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct TickPhase {
    pub ticks: u32,
    pub alpha: f32,
    pub active: bool,
}

impl Default for TickPhase {
    fn default() -> Self { TickPhase { ticks: 1, alpha: 1.0, active: false } }
}

/// Alpha from frame_pace's accumulator after the frame's ticks are taken (its floor mode keeps it in [0, 1)).
pub fn alpha_from_acc(acc: f64) -> f32 { acc.clamp(0.0, 1.0) as f32 }

/// Whether frames come faster than the ticks, so that smoothing has something to draw: `RC_FRAME_SMOOTH_HZ` above
/// 60; no limiter (`RC_NOVSYNC=1`, `RC_FRAME_CAP=0`); the FPS limit Unlimited; or a limit of 60 or 120 on a display
/// above 60 Hz (2.5 % margin, as crate::frame_pace's period match). The limit 30 never.
pub fn faster_than_ticks(forced: Option<f64>, limited: bool, fps: FrameLimit, refresh_hz: Option<f64>) -> bool {
    if let Some(h) = forced { return h > TICK_HZ; }
    if !limited { return true; }
    match fps {
        FrameLimit::Fps30 => false,
        FrameLimit::Unlimited => true,
        FrameLimit::Fps60 | FrameLimit::Fps120 => refresh_hz.is_some_and(|r| r > TICK_HZ * 1.025),
    }
}

/// `RC_FRAME_SMOOTH_HZ=<h>` (frames a second, > 0).
pub fn forced_hz() -> Option<f64> {
    std::env::var("RC_FRAME_SMOOTH_HZ").ok().and_then(|v| v.trim().parse::<f64>().ok()).filter(|&h| h > 0.0)
}

/// `RC_FRAME_SMOOTH_DEBUG` (0 off, 1 per second, 2 per frame too).
fn debug_level() -> u8 { std::env::var("RC_FRAME_SMOOTH_DEBUG").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0) }

/// The transform between `a` (t = 0) and `b` (t = 1): translation and scale lerped, rotation slerped.
pub fn lerp_transform(a: &Transform, b: &Transform, t: f32) -> Transform {
    Transform { translation: a.translation.lerp(b.translation, t), rotation: a.rotation.slerp(b.rotation, t), scale: a.scale.lerp(b.scale, t) }
}

/// A camera move from `a` to `b` in one tick that is a cut, not motion.
pub fn camera_cut(a: &Transform, b: &Transform) -> bool {
    a.translation.distance(b.translation) > CAMERA_CUT || a.rotation.angle_between(b.rotation) > CUT_ANGLE
}

/// Columns `a`, `b` turned more than [`CUT_ANGLE`] or changed length by half (one of them zero: a hidden joint shown).
fn column_cut(a: Vec3, b: Vec3) -> bool {
    let (la, lb) = (a.length(), b.length());
    if la == 0.0 && lb == 0.0 { return false; }
    if la.min(lb) < la.max(lb) * 0.5 { return true; }
    a.angle_between(b) > CUT_ANGLE
}

/// Column `a` → `b` at `t`: the lerp, brought back to the lerped length (a turn keeps its scale).
fn column_lerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    let len = a.length() + (b.length() - a.length()) * t;
    a.lerp(b, t).normalize_or_zero() * len
}

fn word(b: &[u8], i: usize) -> [u8; 4] { [b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]] }
fn f(b: &[u8], i: usize) -> f32 { f32::from_le_bytes(word(b, i)) }
fn put(b: &mut [u8], i: usize, v: f32) { b[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes()); }
fn col(b: &[u8], c: usize) -> Vec3 { Vec3::new(f(b, 4 * c), f(b, 4 * c + 1), f(b, 4 * c + 2)) }
fn put_col(b: &mut [u8], c: usize, v: Vec3) { for k in 0..3 { put(b, 4 * c + k, v[k]); } }

/// One `MobyInst` record between `p` (the previous tick's) and `c` (this tick's) into `out` (a copy of `c`); false when
/// it is a discontinuity (module docs) and `out` keeps `c`.
fn blend_record(p: &[u8], c: &[u8], t: f32, out: &mut [u8]) -> bool {
    if p == c { return true; }
    if p[4 * MISC..4 * (MISC + 3)] != c[4 * MISC..4 * (MISC + 3)] { return false; }
    if col(p, 3).distance(col(c, 3)) > MOBY_CUT || (0..3).any(|k| column_cut(col(p, k), col(c, k))) { return false; }
    if (0..MISC).any(|i| !f(p, i).is_finite() || !f(c, i).is_finite()) { return false; }
    for k in 0..3 { put_col(out, k, column_lerp(col(p, k), col(c, k), t)); }
    put_col(out, 3, col(p, 3).lerp(col(c, 3), t));
    for i in 16..MISC { put(out, i, f(p, i) + (f(c, i) - f(p, i)) * t); }
    true
}

/// The `MobyInst` records of a buffer between `prev` and `cur` at `t` (module docs), into `out` (a copy of `cur`).
/// Returns the indices of the records cut.
pub fn blend_records(prev: &[u8], cur: &[u8], t: f32, out: &mut [u8]) -> Vec<usize> {
    let n = RECORD_WORDS * 4;
    let mut cuts = Vec::new();
    for (i, ((p, c), o)) in prev.chunks_exact(n).zip(cur.chunks_exact(n)).zip(out.chunks_exact_mut(n)).enumerate() {
        if !blend_record(p, c, t, o) { cuts.push(i); }
    }
    cuts
}

/// The palette matrix bytes of the identity.
fn identity_joint() -> [u8; JOINT_WORDS * 4] {
    let mut b = [0u8; JOINT_WORDS * 4];
    for k in 0..4 { put(&mut b, 5 * k, 1.0); }
    b
}

/// One joint matrix between `p` and `c` at `t` into `out` (a copy of `c`); false on a discontinuity.
fn blend_joint(p: &[u8], c: &[u8], t: f32, out: &mut [u8], identity: &[u8]) -> bool {
    if p == c { return true; }
    if p == identity || c == identity { return false; }
    if (0..JOINT_WORDS).any(|i| !f(p, i).is_finite() || !f(c, i).is_finite()) { return false; }
    if (0..3).any(|k| column_cut(col(p, k), col(c, k))) { return false; }
    for k in 0..3 { put_col(out, k, column_lerp(col(p, k), col(c, k), t)); }
    for i in 12..JOINT_WORDS { put(out, i, f(p, i) + (f(c, i) - f(p, i)) * t); }
    true
}

/// The joint palette between `prev` and `cur` at `t` into `out` (a copy of `cur`), except the matrices in `cut` (index
/// ranges: the joints of records cut). Returns the matrices cut.
pub fn blend_palette(prev: &[u8], cur: &[u8], t: f32, out: &mut [u8], cut: &[(usize, usize)]) -> usize {
    let (n, id) = (JOINT_WORDS * 4, identity_joint());
    let mut cuts = 0;
    for (j, ((p, c), o)) in prev.chunks_exact(n).zip(cur.chunks_exact(n)).zip(out.chunks_exact_mut(n)).enumerate() {
        if cut.iter().any(|&(a, b)| (a..b).contains(&j)) { cuts += (p != c) as usize; continue; }
        if !blend_joint(p, c, t, o, &id) { cuts += 1; }
    }
    cuts
}

/// The palette ranges of the records `cut` of a record buffer: from each one's palette base to the next higher base
/// of the buffer (the end: past the palette).
pub fn cut_ranges(records: &[u8], cut: &[usize]) -> Vec<(usize, usize)> {
    let n = RECORD_WORDS * 4;
    let base = |i: usize| records.get(i * n..(i + 1) * n).map(|r| u32::from_le_bytes(word(r, MISC)) as usize);
    let mut bases: Vec<usize> = (0..records.len() / n).filter_map(base).collect();
    bases.sort_unstable();
    bases.dedup();
    cut.iter().filter_map(|&i| base(i)).map(|b| (b, bases.iter().copied().find(|&x| x > b).unwrap_or(usize::MAX))).collect()
}

/// A tracked value's last two ticks.
#[derive(Clone, Default)]
struct Hist<T> {
    prev: T,
    cur: T,
    ready: bool,
}

impl<T: Clone + PartialEq> Hist<T> {
    /// This frame's true value: a tick shifts it in; a change without a tick (written per frame) restarts the history.
    /// False when it restarted (nothing to interpolate).
    fn step(&mut self, now: &T, ticked: bool) -> bool {
        if !self.ready {
            (self.prev, self.cur, self.ready) = (now.clone(), now.clone(), true);
            return false;
        }
        if ticked {
            self.prev = std::mem::replace(&mut self.cur, now.clone());
        } else if *now != self.cur {
            (self.prev, self.cur) = (now.clone(), now.clone());
            return false;
        }
        true
    }

    fn moving(&self) -> bool { self.ready && self.prev != self.cur }
}

/// A storage buffer's history and whether the asset holds the blend (restored in `First`).
struct BufHist {
    handle: Handle<ShaderBuffer>,
    hist: Hist<Vec<u8>>,
    shown: bool,
}

/// What [`blend`] tracks and what [`restore`] puts back.
#[derive(Resource, Default)]
struct Smooth {
    camera: Option<(Entity, Hist<Transform>)>,
    records: HashMap<AssetId<ShaderBuffer>, BufHist>,
    palettes: HashMap<AssetId<ShaderBuffer>, BufHist>,
    /// The cameras whose `GlobalTransform` holds the blend, with the tick's.
    cameras_shown: Vec<(Entity, GlobalTransform)>,
    generation: u64,
    stats: Stats,
}

/// `RC_FRAME_SMOOTH_DEBUG`'s counts since its last line.
#[derive(Default)]
struct Stats {
    frames: u32,
    ticks: u32,
    alpha: Option<(f32, f32)>,
    alpha_sum: f32,
    blended: u32,
    camera_cuts: u32,
    record_cuts: u32,
    joint_cuts: u32,
    restarts: u32,
    since: Option<std::time::Instant>,
}

/// Particle positions at the last two ticks, per pool record (crate::particle_render's `build_sprites`).
#[derive(Resource, Default)]
pub struct ParticleSmooth {
    prev: Vec<Option<[Vec3; 2]>>,
    cur: Vec<Option<[Vec3; 2]>>,
    alpha: f32,
    on: bool,
    pub cuts: u32,
}

impl ParticleSmooth {
    /// Before the frame's sprites: on a frame with ticks, `pool(i)` (record i's position and second end while alive)
    /// becomes the current tick's.
    pub fn begin(&mut self, phase: &TickPhase, n: usize, pool: impl Fn(usize) -> Option<([f32; 3], [f32; 3])>) {
        self.on = phase.active;
        self.alpha = phase.alpha;
        if !self.on {
            if !self.cur.is_empty() { *self = ParticleSmooth::default(); }
            return;
        }
        if phase.ticks == 0 && self.cur.len() == n { return; }
        let now = (0..n).map(|i| pool(i).map(|(a, b)| [Vec3::from_array(a), Vec3::from_array(b)])).collect();
        if self.cur.len() == n {
            self.prev = std::mem::replace(&mut self.cur, now);
        } else {
            (self.prev, self.cur) = (vec![None; n], now);
        }
    }

    /// Record `i`'s point `end` (0: the position, 1: a ribbon's second end) to draw, `p` being this tick's.
    pub fn pos(&mut self, i: usize, end: usize, p: [f32; 3]) -> [f32; 3] {
        if !self.on { return p; }
        let Some(a) = self.prev.get(i).copied().flatten().map(|e| e[end]) else { return p };
        let b = Vec3::from_array(p);
        if !a.is_finite() || !b.is_finite() || a.distance(b) > PARTICLE_CUT {
            self.cuts += (a != b) as u32;
            return p;
        }
        a.lerp(b, self.alpha).to_array()
    }
}

pub struct FrameSmoothPlugin;

impl Plugin for FrameSmoothPlugin {
    fn build(&self, app: &mut App) {
        let s = FrameSmoothing::startup();
        if s.on() { println!("frame smoothing: on (settings file or RC_GFX_FRAME_SMOOTHING; interpolates when frames come faster than the 60 Hz ticks)"); }
        app.insert_resource(s)
            .init_resource::<TickPhase>()
            .init_resource::<ParticleSmooth>()
            .init_resource::<Smooth>()
            .add_systems(First, restore)
            .add_systems(Last, blend.after(crate::moby_render::update_extra_metal).after(crate::level_switch::apply_switch));
        if crate::determinism::deterministic() && crate::determinism::tick_pattern().is_some() {
            app.add_systems(First, frame_exact_phase.after(bevy::time::TimeSystems));
        }
    }
}

/// Frame-exact runs with `RC_DUMP_TICKS` (module docs): this update's ticks and alpha.
fn frame_exact_phase(s: Res<FrameSmoothing>, mut phase: ResMut<TickPhase>, mut k: Local<u64>, mut since: Local<u32>) {
    let Some(p) = crate::determinism::tick_pattern() else { return };
    let rate = p.iter().sum::<u32>() as f32 / p.len() as f32;
    let ticks = p[(*k % p.len() as u64) as usize];
    *k += 1;
    *since = if ticks > 0 { 0 } else { *since + 1 };
    *phase = TickPhase { ticks, alpha: (*since as f32 * rate).min(1.0), active: s.on() && rate < 1.0 };
}

/// `First`: the tick's values back into everything [`blend`] overwrote.
fn restore(mut st: ResMut<Smooth>, mut buffers: ResMut<Assets<ShaderBuffer>>, mut gts: Query<&mut GlobalTransform>) {
    let st = &mut *st;
    for (e, gt) in st.cameras_shown.drain(..) {
        if let Ok(mut g) = gts.get_mut(e) { *g = gt; }
    }
    for b in st.records.values_mut().chain(st.palettes.values_mut()) {
        if std::mem::take(&mut b.shown) { crate::asset_write::set_buffer(&mut buffers, &b.handle, &b.hist.cur); }
    }
}

type MainCam<'w, 's> = Query<'w, 's, (Entity, &'static Transform, &'static mut GlobalTransform), With<crate::fly_cam::FlyCam>>;
type OtherCams<'w, 's> = Query<'w, 's, (Entity, &'static mut GlobalTransform), (With<Camera>, Without<crate::fly_cam::FlyCam>)>;
/// Palette buffer → (its handle, the record buffers drawn with it).
type Pairs = HashMap<AssetId<ShaderBuffer>, (Handle<ShaderBuffer>, HashSet<AssetId<ShaderBuffer>>)>;

/// `Last`: the interpolated camera and moby buffers into the render inputs (module docs).
#[allow(clippy::too_many_arguments)]
fn blend(
    mut st: ResMut<Smooth>,
    phase: Res<TickPhase>,
    generation: Res<crate::level_switch::LevelGeneration>,
    mut main: MainCam,
    mut others: OtherCams,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mats: Res<Assets<MobyMaterial>>,
    metal: Res<Assets<MobyMetalMaterial>>,
    mut particles: ResMut<ParticleSmooth>,
    mut debug: Local<Option<u8>>,
) {
    let debug = *debug.get_or_insert_with(debug_level);
    let st = &mut *st;
    if !phase.active || st.generation != generation.0 {
        if st.camera.is_some() || !st.records.is_empty() || !st.palettes.is_empty() {
            let (shown, stats) = (std::mem::take(&mut st.cameras_shown), std::mem::take(&mut st.stats));
            *st = Smooth { cameras_shown: shown, stats, ..default() };
        }
        st.generation = generation.0;
        return;
    }
    let (t, ticked) = (phase.alpha, phase.ticks > 0);
    let s = &mut st.stats;
    s.frames += 1;
    s.ticks += phase.ticks;
    s.alpha = Some(s.alpha.map_or((t, t), |(lo, hi)| (lo.min(t), hi.max(t))));
    s.alpha_sum += t;

    // The camera.
    let mut drawn_eye = None;
    if let Some((e, tf, mut gt)) = main.iter_mut().next() {
        if st.camera.as_ref().is_none_or(|(c, _)| *c != e) { st.camera = Some((e, Hist::default())); }
        let h = &mut st.camera.as_mut().unwrap().1;
        if !h.step(tf, ticked) { s.restarts += 1; }
        if ticked && h.moving() && camera_cut(&h.prev, &h.cur) {
            s.camera_cuts += 1;
            if debug > 0 {
                println!(
                    "frame smooth: camera cut ({:.2} units, {:.1}°): drawn as the tick's",
                    h.prev.translation.distance(h.cur.translation), h.prev.rotation.angle_between(h.cur.rotation).to_degrees()
                );
            }
            h.prev = h.cur;
        }
        if h.moving() {
            let truth = *gt;
            let b = GlobalTransform::from(lerp_transform(&h.prev, &h.cur, t));
            for (oe, mut ogt) in &mut others {
                if *ogt == truth {
                    st.cameras_shown.push((oe, *ogt));
                    *ogt = b;
                }
            }
            st.cameras_shown.push((e, truth));
            *gt = b;
        }
        drawn_eye = Some(crate::game_camera::game_eye(&gt.compute_transform()));
    }

    // The moby buffers the materials draw with: records → the palettes paired with them.
    let mut pairs = Pairs::new();
    let mut recs: HashMap<AssetId<ShaderBuffer>, Handle<ShaderBuffer>> = HashMap::new();
    let mut add = |i: &Handle<ShaderBuffer>, p: &Handle<ShaderBuffer>| {
        recs.entry(i.id()).or_insert_with(|| i.clone());
        pairs.entry(p.id()).or_insert_with(|| (p.clone(), HashSet::new())).1.insert(i.id());
    };
    for (_, m) in mats.iter() { add(&m.instances, &m.palette); }
    for (_, m) in metal.iter() { add(&m.instances, &m.palette); }
    st.records.retain(|id, _| recs.contains_key(id));
    st.palettes.retain(|id, _| pairs.contains_key(id));

    let mut ranges: HashMap<AssetId<ShaderBuffer>, Vec<(usize, usize)>> = HashMap::new();
    for (id, h) in recs {
        let Some(now) = buffers.get(&h).and_then(|b| b.data.as_deref()) else { continue };
        let b = st.records.entry(id).or_insert_with(|| BufHist { handle: h.clone(), hist: Hist::default(), shown: false });
        if !step_buffer(&mut b.hist, now, ticked, &mut st.stats) || !b.hist.moving() { continue; }
        let mut out = b.hist.cur.clone();
        let cuts = blend_records(&b.hist.prev, &b.hist.cur, t, &mut out);
        st.stats.record_cuts += cuts.len() as u32;
        if !cuts.is_empty() { ranges.insert(id, cut_ranges(&b.hist.cur, &cuts)); }
        st.stats.blended += 1;
        b.shown = crate::asset_write::set_buffer(&mut buffers, &b.handle, &out);
    }
    for (id, (h, with)) in pairs {
        let Some(now) = buffers.get(&h).and_then(|b| b.data.as_deref()) else { continue };
        let b = st.palettes.entry(id).or_insert_with(|| BufHist { handle: h.clone(), hist: Hist::default(), shown: false });
        if !step_buffer(&mut b.hist, now, ticked, &mut st.stats) || !b.hist.moving() { continue; }
        let cut: Vec<(usize, usize)> = with.iter().filter_map(|r| ranges.get(r)).flatten().copied().collect();
        let mut out = b.hist.cur.clone();
        st.stats.joint_cuts += blend_palette(&b.hist.prev, &b.hist.cur, t, &mut out, &cut) as u32;
        st.stats.blended += 1;
        b.shown = crate::asset_write::set_buffer(&mut buffers, &b.handle, &out);
    }

    if debug >= 2 {
        println!("frame smooth: frame ticks {} alpha {t:.3} camera {:?}", phase.ticks, drawn_eye.map(|e| [e.x, e.y, e.z]));
    }
    let s = &mut st.stats;
    let now = std::time::Instant::now();
    let since = *s.since.get_or_insert(now);
    if debug > 0 && (now - since).as_secs_f32() >= 1.0 {
        let secs = (now - since).as_secs_f32();
        let (lo, hi) = s.alpha.unwrap_or_default();
        println!(
            "frame smooth: {:.1} frames/s, {:.1} ticks/s, alpha {lo:.3}..{hi:.3} (mean {:.3}); {} buffer blends; skipped: {} camera cuts, \
             {} moby records, {} joints, {} particles, {} restarts (written without a tick)",
            s.frames as f32 / secs, s.ticks as f32 / secs, s.alpha_sum / s.frames.max(1) as f32, s.blended,
            s.camera_cuts, s.record_cuts, s.joint_cuts, particles.cuts, s.restarts
        );
        *s = Stats { since: Some(now), ..default() };
        particles.cuts = 0;
    }
}

/// A buffer's bytes this frame into its history ([`Hist::step`] without reallocating); false when it restarted (a new
/// buffer, another size, a write without a tick).
fn step_buffer(h: &mut Hist<Vec<u8>>, now: &[u8], ticked: bool, s: &mut Stats) -> bool {
    if !h.ready || now.len() != h.cur.len() {
        *h = Hist { prev: now.to_vec(), cur: now.to_vec(), ready: true };
        return false;
    }
    if ticked {
        std::mem::swap(&mut h.prev, &mut h.cur);
        h.cur.copy_from_slice(now);
    } else if h.cur != now {
        h.prev.copy_from_slice(now);
        h.cur.copy_from_slice(now);
        s.restarts += 1;
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pos: [f32; 3], yaw: f32, base: u32) -> Vec<u8> {
        let mut r = vec![0u8; RECORD_WORDS * 4];
        let m = Mat4::from_scale_rotation_translation(Vec3::splat(0.5 / 1024.0), Quat::from_rotation_y(yaw), Vec3::from_array(pos));
        for (i, v) in m.to_cols_array().iter().enumerate() { put(&mut r, i, *v); }
        for i in 16..MISC { put(&mut r, i, i as f32); }
        r[4 * MISC..4 * MISC + 4].copy_from_slice(&base.to_le_bytes());
        r
    }

    fn model(r: &[u8]) -> Mat4 { Mat4::from_cols_array(&std::array::from_fn(|i| f(r, i))) }

    #[test]
    fn alpha_is_the_accumulator_remainder() {
        assert_eq!(alpha_from_acc(0.0), 0.0);
        assert_eq!(alpha_from_acc(0.4), 0.4);
        assert_eq!(alpha_from_acc(-0.2), 0.0);
        assert_eq!(alpha_from_acc(1.7), 1.0);
    }

    #[test]
    fn active_only_above_the_tick_rate() {
        use FrameLimit::*;
        // At or below 60 Hz the option does nothing (On == Off).
        for fps in [Fps30, Fps60, Fps120] {
            assert!(!faster_than_ticks(None, true, fps, Some(60.0)));
            assert!(!faster_than_ticks(None, true, fps, Some(59.94)));
            assert!(!faster_than_ticks(None, true, fps, None));
        }
        assert!(!faster_than_ticks(Some(60.0), true, Fps60, Some(144.0)));
        assert!(!faster_than_ticks(Some(50.0), false, Unlimited, None));
        assert!(!faster_than_ticks(None, true, Fps30, Some(144.0)));
        // Above it: the display's rate, a forced rate, no vsync.
        assert!(faster_than_ticks(None, true, Fps60, Some(144.0)));
        assert!(faster_than_ticks(None, true, Fps120, Some(120.0)));
        assert!(faster_than_ticks(Some(144.0), true, Fps60, Some(60.0)));
        assert!(faster_than_ticks(None, false, Fps60, Some(60.0)));
        assert!(faster_than_ticks(None, true, Unlimited, Some(60.0)));
    }

    #[test]
    fn transforms_lerp_and_slerp() {
        let a = Transform::from_xyz(0.0, 0.0, 0.0);
        let b = Transform::from_xyz(2.0, 0.0, -4.0).with_rotation(Quat::from_rotation_y(0.4));
        let m = lerp_transform(&a, &b, 0.5);
        assert!((m.translation - Vec3::new(1.0, 0.0, -2.0)).length() < 1e-6);
        assert!((m.rotation.angle_between(Quat::IDENTITY) - 0.2).abs() < 1e-5);
        for (k, want) in [(0.0, a), (1.0, b)] {
            let e = lerp_transform(&a, &b, k);
            assert!((e.translation - want.translation).length() < 1e-6 && e.rotation.angle_between(want.rotation) < 1e-3);
        }
        assert!(!camera_cut(&a, &b));
        assert!(camera_cut(&a, &Transform::from_xyz(9.0, 0.0, 0.0)));
        assert!(camera_cut(&a, &Transform::from_rotation(Quat::from_rotation_y(1.0))));
    }

    #[test]
    fn records_blend_between_ticks() {
        let (p, c) = (record([0.0, 1.0, 0.0], 0.0, 4), record([1.0, 1.0, 2.0], 0.3, 4));
        let mut out = c.clone();
        assert!(blend_records(&p, &c, 0.5, &mut out).is_empty());
        let m = model(&out);
        assert!((m.w_axis.truncate() - Vec3::new(0.5, 1.0, 1.0)).length() < 1e-6);
        // The axes keep their scale, turned half-way.
        let x = m.x_axis.truncate();
        assert!((x.length() - 0.5 / 1024.0).abs() < 1e-9);
        assert!((x.angle_between(Vec3::X) - 0.15).abs() < 1e-4);
        // The light block is lerped, the misc words are this tick's.
        assert_eq!(f(&out, 20), 20.0);
        assert_eq!(out[4 * MISC..], c[4 * MISC..]);
        let mut same = c.clone();
        blend_records(&p, &c, 1.0, &mut same);
        assert!((model(&same).w_axis - model(&c).w_axis).length() < 1e-6);
    }

    #[test]
    fn discontinuities_are_not_blended() {
        let p = record([0.0, 0.0, 0.0], 0.0, 4);
        for c in [record([30.0, 0.0, 0.0], 0.0, 4), record([0.1, 0.0, 0.0], 1.2, 4), record([0.1, 0.0, 0.0], 0.0, 9)] {
            let mut out = c.clone();
            assert_eq!(blend_records(&p, &c, 0.5, &mut out), vec![0]);
            assert_eq!(out, c);
        }
        // Two records: only the teleported one is cut; its joints (base 9 up to the next base 20) with it.
        let (mut prev, mut cur) = (record([0.0; 3], 0.0, 9), record([50.0, 0.0, 0.0], 0.0, 9));
        prev.extend(record([0.0; 3], 0.0, 20));
        cur.extend(record([0.2, 0.0, 0.0], 0.0, 20));
        let mut out = cur.clone();
        let cuts = blend_records(&prev, &cur, 0.5, &mut out);
        assert_eq!(cuts, vec![0]);
        assert_eq!(cut_ranges(&cur, &cuts), vec![(9, 20)]);
        assert!((model(&out[RECORD_WORDS * 4..]).w_axis.x - 0.1).abs() < 1e-6);
        // Camera cuts.
        assert!(camera_cut(&Transform::IDENTITY, &Transform::from_xyz(0.0, 0.0, CAMERA_CUT + 0.1)));
    }

    fn joint(yaw: f32, t: [f32; 3]) -> Vec<u8> {
        let m = Mat4::from_rotation_translation(Quat::from_rotation_z(yaw), Vec3::from_array(t));
        let mut b = vec![0u8; JOINT_WORDS * 4];
        for (i, v) in m.to_cols_array().iter().enumerate() { put(&mut b, i, *v); }
        b
    }

    #[test]
    fn palettes_blend_poses_and_skip_cuts() {
        let id = identity_joint().to_vec();
        let prev = [joint(0.2, [0.0, 10.0, 0.0]), id.clone(), joint(0.05, [0.0; 3]), joint(0.0, [1.0; 3])].concat();
        let cur = [joint(0.4, [0.0, 20.0, 0.0]), joint(0.3, [5.0; 3]), joint(1.5, [0.0; 3]), joint(0.1, [2.0; 3])].concat();
        let mut out = cur.clone();
        // Joint 1 leaves the identity (made visible), 2 turns 1.5 rad: both cut; 3 is in a cut record's range.
        assert_eq!(blend_palette(&prev, &cur, 0.5, &mut out, &[(3, 4)]), 3);
        let m = Mat4::from_cols_array(&std::array::from_fn(|i| f(&out, i)));
        assert!((m.w_axis.y - 15.0).abs() < 1e-5);
        assert!((m.x_axis.truncate().angle_between(Vec3::X) - 0.3).abs() < 1e-4);
        assert_eq!(out[64..], cur[64..]);
    }

    #[test]
    fn history_follows_ticks_and_restarts_on_frame_writes() {
        let mut h = Hist::default();
        assert!(!h.step(&1, true));
        assert!(!h.moving());
        assert!(h.step(&2, true) && h.moving() && (h.prev, h.cur) == (1, 2));
        // A frame without a tick and no write: still between 1 and 2.
        assert!(h.step(&2, false) && h.moving());
        // A write without a tick (a per-frame writer): drawn as it is.
        assert!(!h.step(&5, false) && !h.moving());
    }

    #[test]
    fn particles_lerp_from_the_previous_tick() {
        let mut ps = ParticleSmooth::default();
        let on = |ticks, alpha| TickPhase { ticks, alpha, active: true };
        let pool = |x: f32| move |i: usize| (i == 0 || i == 2).then_some(([x, 0.0, 0.0], [x, 1.0, 0.0]));
        ps.begin(&on(1, 0.0), 3, pool(0.0));
        ps.begin(&on(1, 0.25), 3, pool(1.0));
        assert_eq!(ps.pos(0, 0, [1.0, 0.0, 0.0]), [0.25, 0.0, 0.0]);
        assert_eq!(ps.pos(2, 1, [1.0, 1.0, 0.0]), [0.25, 1.0, 0.0]);
        // Dead at the previous tick, or a jump: this tick's.
        assert_eq!(ps.pos(1, 0, [7.0, 0.0, 0.0]), [7.0, 0.0, 0.0]);
        assert_eq!(ps.pos(0, 0, [9.0, 0.0, 0.0]), [9.0, 0.0, 0.0]);
        // A frame without a tick keeps the two ticks.
        ps.begin(&on(0, 0.75), 3, pool(5.0));
        assert_eq!(ps.pos(0, 0, [1.0, 0.0, 0.0]), [0.75, 0.0, 0.0]);
        // Inactive (Off, or at or below the tick rate): exactly the tick's.
        ps.begin(&TickPhase::default(), 3, pool(2.0));
        assert_eq!(ps.pos(0, 0, [2.0, 0.0, 0.0]), [2.0, 0.0, 0.0]);
    }
}
