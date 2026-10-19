//! **Port-only: the new level's pipelines compiled behind the level change's black screen.** Bevy compiles a render
//! pipeline the first time something needs it, in the background, and leaves that thing out of the frame until it is
//! ready: a level's first sight of each material (a moby, a shader variant) shows as a hitch or a missing object for
//! a few frames. The PS2 hid its disc reads behind the transition's black screen after the level swap
//! (crate::travel_render, `Step::Enter`); this reuses that screen for the compiles.
//!
//! After a level change ([`LevelGeneration`]) whose transition still shows the black screen
//! (`Travel::blanked`), the game holds: no ticks ([`hold`], `TimeUpdateStrategy::FixedTimesteps(0)` after
//! crate::frame_pace's), so the transition stays on its black frame and the level's `entry` waits. Meanwhile the world
//! and sky cameras draw the new level again (`travel_render::view_layers`) under the HUD's opaque black rectangle,
//! with CPU frustum culling off ([`NoCpuCulling`] on the [`WorldCamera`]s) and every hidden mesh shown for the
//! visibility check ([`show_all`] / [`restore`]), so each mesh's pipeline is queued. The hold ends once the set-up
//! built the play state, [`MIN_FRAMES`] have passed and the pipeline cache had nothing waiting and no new pipeline
//! for [`STABLE_FRAMES`] frames, or after `RC_PIPELINE_WARM_MAX` seconds (default [`DEFAULT_MAX`]).
//!
//! It logs `pipelines: level NN warmed ...` with the time it took, and afterwards every pipeline created during play
//! (`pipelines: late ...`, its label): what the hold missed. Not in the frame-exact mode (crate::determinism) or a
//! replay (crate::session: the recorded ticks drive it); `RC_PIPELINE_WARM=0` turns it off. Not at the boot: its
//! level shows behind no black screen.

use crate::level_switch::LevelGeneration;
use crate::render_settings::WorldCamera;
use bevy::camera::visibility::{NoCpuCulling, VisibilitySystems};
use bevy::prelude::*;
use bevy::render::render_resource::{PipelineCache, PipelineDescriptor};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::time::TimeUpdateStrategy;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// The hold's shortest length in frames: the set-up spawns over the first frames after the swap.
const MIN_FRAMES: u32 = 8;
/// Frames in a row with nothing waiting and no new pipeline that end the hold.
const STABLE_FRAMES: u32 = 3;
/// The hold's longest length in seconds (`RC_PIPELINE_WARM_MAX`).
const DEFAULT_MAX: f64 = 20.0;

/// The render world's pipeline cache, as of its last frame ([`count`]).
#[derive(Resource, Clone, Default)]
struct Counts(Arc<CountsInner>);

#[derive(Default)]
struct CountsInner {
    total: AtomicUsize,
    waiting: AtomicUsize,
    /// Labels of the pipelines created during play are kept (`late`), not those of the boot or a hold.
    report: AtomicBool,
    late: Mutex<Vec<String>>,
}

/// The hold of a level change (module docs).
#[derive(Resource, Default)]
pub(crate) struct PipelineWarm {
    hold: Option<Hold>,
    max: f64,
}

struct Hold {
    level: u32,
    start: Instant,
    frames: u32,
    stable: u32,
    first_total: usize,
    last_total: usize,
}

impl PipelineWarm {
    /// The level change is holding: the world cameras draw the new level under the black screen.
    pub(crate) fn active(&self) -> bool { self.hold.is_some() }
}

pub struct PipelineWarmPlugin;

impl Plugin for PipelineWarmPlugin {
    fn build(&self, app: &mut App) {
        if !crate::gameplay::enabled() || crate::determinism::deterministic() || crate::session::replay_steps(0).is_some() {
            return;
        }
        if std::env::var("RC_PIPELINE_WARM").is_ok_and(|v| v.trim() == "0") { return; }
        let max = std::env::var("RC_PIPELINE_WARM_MAX").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(DEFAULT_MAX);
        let counts = Counts::default();
        app.insert_resource(PipelineWarm { hold: None, max })
            .insert_resource(counts.clone())
            .init_resource::<Shown>()
            .add_systems(First, hold.after(crate::frame_pace::PaceSteps).before(bevy::time::TimeSystems))
            .add_systems(
                PostUpdate,
                (
                    show_all.after(VisibilitySystems::VisibilityPropagate).before(VisibilitySystems::CheckVisibility),
                    restore.after(VisibilitySystems::CheckVisibility),
                ),
            )
            .add_systems(Last, late);
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.insert_resource(counts).add_systems(Render, count.in_set(RenderSystems::Cleanup));
        }
    }
}

/// Render world: the cache's pipelines and waiting ones; the labels of new ones while reporting.
fn count(cache: Res<PipelineCache>, counts: Res<Counts>, mut seen: Local<usize>) {
    let total = cache.pipelines().count();
    if total > *seen && counts.0.report.load(Ordering::Relaxed) {
        let mut late = counts.0.late.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for p in cache.pipelines().skip(*seen) {
            let label = match &p.descriptor {
                PipelineDescriptor::RenderPipelineDescriptor(d) => d.label.as_deref(),
                PipelineDescriptor::ComputePipelineDescriptor(d) => d.label.as_deref(),
            };
            late.push(label.unwrap_or("(no label)").to_string());
        }
    }
    *seen = total;
    counts.0.total.store(total, Ordering::Relaxed);
    counts.0.waiting.store(cache.waiting_pipelines().count(), Ordering::Relaxed);
}

/// Starts, runs and ends the hold; while it runs, this update has no ticks.
#[allow(clippy::too_many_arguments)]
fn hold(
    mut warm: ResMut<PipelineWarm>,
    generation: Res<LevelGeneration>,
    travel: Res<crate::travel_render::Travel>,
    play: Option<Res<crate::gameplay::Play>>,
    counts: Res<Counts>,
    mut strategy: ResMut<TimeUpdateStrategy>,
    cams: Query<Entity, With<WorldCamera>>,
    mut commands: Commands,
) {
    let total = counts.0.total.load(Ordering::Relaxed);
    if generation.is_changed() && !generation.is_added() && travel.blanked() && !crate::saves::front_end_active() {
        counts.0.report.store(false, Ordering::Relaxed);
        let level = crate::level_load::level_index();
        warm.hold = Some(Hold { level, start: Instant::now(), frames: 0, stable: 0, first_total: total, last_total: total });
        for e in &cams { commands.entity(e).insert(NoCpuCulling); }
    }
    let max = warm.max;
    let Some(h) = warm.hold.as_mut() else { return };
    h.frames += 1;
    let waiting = counts.0.waiting.load(Ordering::Relaxed);
    if play.is_some() && h.frames > MIN_FRAMES && waiting == 0 && total == h.last_total { h.stable += 1; } else { h.stable = 0; }
    h.last_total = total;
    let secs = h.start.elapsed().as_secs_f64();
    if h.stable < STABLE_FRAMES && secs < max {
        *strategy = TimeUpdateStrategy::FixedTimesteps(0);
        return;
    }
    let capped = if h.stable < STABLE_FRAMES { format!(" (cut at {max} s, {waiting} still compiling)") } else { String::new() };
    println!(
        "pipelines: level {:02} warmed in {secs:.2} s ({} frames): {total} (+{} new){capped}",
        h.level,
        h.frames,
        total.saturating_sub(h.first_total)
    );
    warm.hold = None;
    for e in &cams { commands.entity(e).remove::<NoCpuCulling>(); }
    counts.0.report.store(true, Ordering::Relaxed);
}

/// While holding: every mesh hidden by its visibility counts as visible for this frame's visibility check (bypassing
/// change detection, so the propagation does not see it; [`restore`] puts it back).
fn show_all(warm: Res<PipelineWarm>, mut meshes: Query<(Entity, &mut InheritedVisibility), With<Mesh3d>>, mut shown: ResMut<Shown>) {
    if !warm.active() { return; }
    for (e, mut v) in &mut meshes {
        if !v.get() {
            *v.bypass_change_detection() = InheritedVisibility::VISIBLE;
            shown.0.push(e);
        }
    }
}

/// The meshes [`show_all`] showed, hidden again after the visibility check.
#[derive(Resource, Default)]
struct Shown(Vec<Entity>);

fn restore(mut shown: ResMut<Shown>, mut meshes: Query<&mut InheritedVisibility>) {
    for e in shown.0.drain(..) {
        if let Ok(mut v) = meshes.get_mut(e) { *v.bypass_change_detection() = InheritedVisibility::HIDDEN; }
    }
}

/// The pipelines created during play since the last frame: what a hold missed.
fn late(counts: Res<Counts>, frame: Res<bevy::diagnostic::FrameCount>) {
    let mut late = counts.0.late.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if late.is_empty() { return; }
    println!("pipelines: late at frame {}: {}", frame.0, late.join(", "));
    late.clear();
}
