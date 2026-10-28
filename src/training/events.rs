//! 교육 이벤트와 세션 기록. JSON 내보내기·평가는 Phase 4에서 추가한다.

use bevy::prelude::Resource;

/// 교육 기록에 남기는 이벤트.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrainingEvent {
    /// 차체가 고정 장애물에 새로 닿음.
    Collision {
        /// 고정 주기 시뮬레이션 경과 시간(s).
        sim_time_s: f32,
        /// 닿기 직전의 부호 있는 속도(m/s).
        speed_mps: f32,
        /// 멈춘 자세의 후륜축 중심 x 좌표(m).
        x_m: f32,
        /// 멈춘 자세의 후륜축 중심 z 좌표(m).
        z_m: f32,
    },
}

/// 현재 세션의 교육 이벤트 목록(발생 순서).
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct TrainingLog {
    events: Vec<TrainingEvent>,
}

impl TrainingLog {
    /// 이벤트를 기록 끝에 추가한다.
    pub fn push(&mut self, event: TrainingEvent) {
        self.events.push(event);
    }

    /// 기록된 이벤트를 발생 순서대로 반환한다.
    pub fn events(&self) -> &[TrainingEvent] {
        &self.events
    }

    /// 기록된 충돌 횟수를 센다.
    pub fn collision_count(&self) -> usize {
        self.events
            .iter()
            .filter(|event| matches!(event, TrainingEvent::Collision { .. }))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 추가한 순서대로 보관하고 충돌 횟수를 세는지 확인한다.
    #[test]
    fn log_keeps_order_and_counts_collisions() {
        let mut log = TrainingLog::default();
        assert_eq!(log.collision_count(), 0);
        for time in [1.0, 2.5] {
            log.push(TrainingEvent::Collision {
                sim_time_s: time,
                speed_mps: 1.0,
                x_m: 0.0,
                z_m: 0.0,
            });
        }
        assert_eq!(log.collision_count(), 2);
        assert!(matches!(
            log.events()[1],
            TrainingEvent::Collision { sim_time_s, .. } if sim_time_s == 2.5
        ));
    }
}
