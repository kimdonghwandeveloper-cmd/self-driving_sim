//! 교육 기록. Phase 5 기록·평가의 기초로, 지금은 충돌 이벤트를 모은다.

pub mod events;

use bevy::prelude::*;

use crate::SimSet;
use crate::vehicle::CollisionEvent;

pub use events::{TrainingEvent, TrainingLog};

/// 교육 기록 리소스와 이벤트 수집 시스템을 등록하는 플러그인.
pub struct TrainingPlugin;

impl Plugin for TrainingPlugin {
    /// 교육 기록 리소스를 만들고, 화면 표시 전에 이번 프레임의 충돌 메시지를 기록하게 한다.
    fn build(&self, app: &mut App) {
        app.init_resource::<TrainingLog>().add_systems(
            Update,
            record_collisions
                .before(SimSet::Display)
                .run_if(crate::driving_active),
        );
    }
}

/// 충돌 메시지를 교육 이벤트로 바꿔 기록하고 로그에 남긴다.
pub fn record_collisions(
    mut collisions: MessageReader<CollisionEvent>,
    mut log: ResMut<TrainingLog>,
) {
    for collision in collisions.read() {
        warn!(
            "training event: collision at ({:.2}, {:.2}) m, speed {:+.2} m/s, t={:.2} s",
            collision.x_m, collision.z_m, collision.speed_mps, collision.sim_time_s
        );
        log.push(TrainingEvent::Collision {
            sim_time_s: collision.sim_time_s,
            speed_mps: collision.speed_mps,
            x_m: collision.x_m,
            z_m: collision.z_m,
        });
    }
}
