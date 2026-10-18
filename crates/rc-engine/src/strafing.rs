//! **Port-only: Strafing** (the Port Options' "Strafing" row, `rc_game::menus::pause::port::Setting::Strafing`; the
//! rules: `rc_game::hero::strafe`, docs/plan/modern_strafing.md). The value is kept in the port settings file
//! (`strafing = classic|modern`, crate::render_settings; frame-exact runs read it only from an explicit
//! `RC_SETTINGS_FILE`) and copied into the game's options before every tick (`GameOptions::strafing`).

use bevy::prelude::*;
use rc_game::hero::strafe::Strafing;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StrafingSetting(pub Strafing);

impl StrafingSetting {
    const KEY: &str = "strafing";

    /// The start value: the port settings file's, else Classic.
    pub fn startup() -> Self { StrafingSetting(crate::render_settings::load_key(Self::KEY).as_deref().and_then(Strafing::parse).unwrap_or_default()) }

    /// Writes the setting into the port settings file.
    pub fn save(self) { crate::render_settings::save_key(Self::KEY, self.0.key()); }
}

pub struct StrafingPlugin;

impl Plugin for StrafingPlugin {
    fn build(&self, app: &mut App) {
        let s = StrafingSetting::startup();
        if s != StrafingSetting::default() { println!("strafing: {} (settings file)", s.0.key()); }
        app.insert_resource(s).add_systems(FixedUpdate, apply.before(crate::gameplay::GameTick));
    }
}

/// The setting into the game's options (the tick keeps it when it rebuilds them from the saved game).
fn apply(s: Res<StrafingSetting>, play: Option<ResMut<crate::gameplay::Play>>) {
    let Some(mut p) = play else { return };
    if p.game.options.strafing != s.0 { p.game.options.strafing = s.0; }
}
