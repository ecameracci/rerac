//! **Full-screen tints from class draw callbacks** (G-REN-030): `rc_game::moby_update::classes::draw_callbacks::
//! screen_tints` (Hoven's turret 1267's red screen, Quartu / the Fleet's alarm 408, Giant Clank's beam flash) as one
//! full-screen GS-blend pass on the main camera ([`crate::gs_post`]): each tint in order, `Cv = ((A − B)·As >> 7) + D` on
//! the display bytes with A / B / D from the low byte of its ALPHA_1 (0x44: Cs, Cd, Cd; 0x42: 0, Cs, Cd),
//! `assets/shaders/screen_tint.wgsl`.
//!
//! [L] The game draws them inside the world pass (the list-2 callbacks after the ties, the beam's sprite after the
//! shrubs), so the mobys, particles and effects drawn later are not tinted; the pass runs after the 3-D passes and before
//! the HUD, so they are.
//!
//! Port-only, Quick Select: Modern (docs/plan/quick_select_pause.md): while the world is held (`Play::qs_hold`) one more
//! tint, black with ALPHA_1 0x44, dims the world to half under the HUD's ring, eased in and out over [`DIM_SECONDS`].

use bevy::core_pipeline::fullscreen_material::FullscreenMaterial;
use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::BoxedSystem;
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::ShaderType;
use bevy::shader::ShaderRef;

use crate::fly_cam::FlyCam;

/// The frame's tints (at most four; GS RGBA bytes, A = 0x80 → 1.0) and their ALPHA_1 low bytes.
#[derive(Component, ExtractComponent, Clone, Copy, Default, PartialEq, ShaderType)]
pub struct ScreenTint {
    pub rgba: [UVec4; 4],
    pub alpha: UVec4,
    pub count: UVec4,
}

impl FullscreenMaterial for ScreenTint {
    fn fragment_shader() -> ShaderRef { "shaders/screen_tint.wgsl".into() }

    /// After the 3-D passes, before the HUD.
    fn schedule_configs(system: ScheduleConfigs<BoxedSystem>) -> ScheduleConfigs<BoxedSystem> {
        system.after(bevy::core_pipeline::Core3dSystems::PostProcess).before(bevy::ui_render::ui_pass)
    }
}

pub struct ScreenTintPlugin;

impl Plugin for ScreenTintPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(crate::gs_post::GsPostPlugin::<ScreenTint>::default()).add_systems(PostUpdate, update);
    }
}

/// The held world's dim: the tint's As at full dim (0x40 = half), and the seconds its ease in or out takes.
pub const DIM_ALPHA: f32 = 64.0;
pub const DIM_SECONDS: f32 = 0.1;

fn update(
    mut commands: Commands,
    play: Option<Res<crate::gameplay::Play>>,
    cams: Query<(Entity, Option<&ScreenTint>), With<FlyCam>>,
    time: Res<Time>,
    mut dim: Local<f32>,
) {
    let Some((entity, have)) = cams.iter().next() else { return };
    let held = play.as_deref().is_some_and(|p| p.qs_hold);
    let step = time.delta_secs() / DIM_SECONDS;
    *dim = if held { (*dim + step).min(1.0) } else { (*dim - step).max(0.0) };
    let tints = play.as_deref().map(|p| rc_game::moby_update::classes::draw_callbacks::screen_tints(&p.game.mobys, &p.svc, p.game.counter)).unwrap_or_default();
    let mut list: Vec<(UVec4, u32)> = tints.iter().map(|s| (UVec4::new(s.rgba & 0xff, (s.rgba >> 8) & 0xff, (s.rgba >> 16) & 0xff, s.rgba >> 24), s.alpha as u32)).collect();
    let dim_a = (*dim * DIM_ALPHA).round() as u32;
    if dim_a != 0 { list.push((UVec4::new(0, 0, 0, dim_a), 0x44)); }
    let want = (!list.is_empty()).then(|| {
        let mut t = ScreenTint::default();
        // The dim is last, so it stays when four class tints fill the pass.
        let from = list.len().saturating_sub(4);
        for (k, (rgba, alpha)) in list[from..].iter().enumerate() {
            t.rgba[k] = *rgba;
            t.alpha[k] = *alpha;
        }
        t.count.x = list.len().min(4) as u32;
        t
    });
    match (want, have) {
        (Some(w), Some(h)) if *h == w => {}
        (Some(w), _) => { commands.entity(entity).insert(w); }
        (None, Some(_)) => { commands.entity(entity).remove::<ScreenTint>(); }
        (None, None) => {}
    }
}
