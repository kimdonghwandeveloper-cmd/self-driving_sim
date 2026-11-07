//! 화면 UI(HUD).

pub mod hud;

use bevy::prelude::*;

use crate::SimSet;

/// HUD 생성·갱신 시스템을 등록하는 플러그인.
pub struct HudPlugin;

impl Plugin for HudPlugin {
    /// HUD 시스템을 앱에 등록한다.
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, hud::spawn_hud.run_if(crate::no_app_state))
            .add_systems(
                OnEnter(crate::AppMode::Driving),
                hud::spawn_hud.in_set(crate::SessionSet::Spawn),
            )
            .add_systems(Update, hud::update_hud.in_set(SimSet::Display));
    }
}
