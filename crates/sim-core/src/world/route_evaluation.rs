//! 지도 입력 형식에 독립적인 경로 편차·거리·목표 도달 평가.
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// 경로 코스 주행 성과 및 경로 이탈 오차(Cross-Track Error) 평가 메트릭.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteEvaluationMetrics {
    /// 목표 지점 도달 성공 여부.
    pub goal_reached: bool,
    /// 총 주행 시간(초).
    pub elapsed_time_s: f32,
    /// 주행 물리 틱 수.
    pub tick_count: usize,
    /// 누적 주행 거리(m).
    pub distance_m: f32,
    /// 평균 경로 이탈 오차(Cross-Track Error, m).
    pub avg_cte_m: f32,
    /// 최대 경로 이탈 오차(Max Cross-Track Error, m).
    pub max_cte_m: f32,
    /// 장애물 충돌 횟수.
    pub impact_count: usize,
    /// 최종 위치에서 목표 지점까지의 직선 잔여 거리(m).
    pub remaining_to_goal_m: f32,
}

impl RouteEvaluationMetrics {
    /// 평가 메트릭을 사람이 읽기 편한 한국어 요약 문자열로 서식화한다.
    pub fn summary_korean(&self) -> String {
        let status = if self.goal_reached {
            "목표 도달 성공"
        } else {
            "목표 미도달"
        };
        format!(
            "[{status}] 주행시간: {:.2}초, 주행거리: {:.1}m, 평균 CTE: {:.3}m, 최대 CTE: {:.3}m, 충돌: {}회, 목표잔여: {:.1}m",
            self.elapsed_time_s,
            self.distance_m,
            self.avg_cte_m,
            self.max_cte_m,
            self.impact_count,
            self.remaining_to_goal_m
        )
    }
}

/// 매 물리 틱마다 차량 위치를 안내선과 비교하여 실시간 경로 이탈 오차(CTE)를 누적하고 목표 도달을 판정하는 평가기.
#[derive(Debug, Clone)]
pub struct RouteDriveEvaluator {
    /// 코스 가이드 중심선.
    guide: Vec<Vec2>,
    /// 목표 지점 좌표(x, z).
    goal_position: Vec2,
    /// 목표 지점 도달 판정 반경(m).
    goal_threshold_m: f32,
    /// 최근 탐색 선분 색인 (되돌아감 방지용).
    last_segment: usize,
    /// 총 진행된 물리 틱 수.
    tick_count: usize,
    /// 장애물 충돌 횟수.
    impact_count: usize,
    /// 누적 CTE 합산값(m).
    sum_cte_m: f32,
    /// 최대 CTE(m).
    max_cte_m: f32,
    /// 최근 차량 위치(거리 누적용).
    last_pos: Option<Vec2>,
    /// 누적 이동 거리(m).
    distance_travelled_m: f32,
    /// 최근 측정된 순간 CTE(m).
    current_cte_m: f32,
    /// 목표 도달 성공 여부.
    goal_reached: bool,
}

impl RouteDriveEvaluator {
    /// 가이드 중심선과 목표 지점, 도달 반경을 받아 평가기를 생성한다.
    pub fn new(guide: &[Vec2], goal_position: Vec2, goal_threshold_m: f32) -> Result<Self, String> {
        if guide.len() < 2 {
            return Err("guide line must have at least 2 points".to_owned());
        }

        Ok(Self {
            guide: guide.to_vec(),
            goal_position,
            goal_threshold_m: goal_threshold_m.max(1.0),
            last_segment: 0,
            tick_count: 0,
            impact_count: 0,
            sum_cte_m: 0.0,
            max_cte_m: 0.0,
            last_pos: None,
            distance_travelled_m: 0.0,
            current_cte_m: 0.0,
            goal_reached: false,
        })
    }

    /// 공통 코스의 목표 자세와 명시 안내선으로 기본 평가기를 구성한다.
    pub fn from_course(course: &super::course::CourseDef, guide: &[Vec2]) -> Result<Self, String> {
        let goal = course.goal.ok_or("course has no goal pose")?;
        Self::new(guide, Vec2::new(goal.x_m, goal.z_m), 5.0)
    }

    /// OSM 저장 경로 또는 블록 저장 경로의 안내선을 읽어 공통 평가기를 만든다.
    pub fn load_map_file(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let table: toml::Table =
            toml::from_str(&text).map_err(|e| format!("invalid course: {e}"))?;
        let base = path.parent().unwrap_or(std::path::Path::new("."));
        match table.get("format").and_then(toml::Value::as_str) {
            Some(super::block_route_file::BLOCK_ROUTE_FORMAT) => {
                let result = super::block_route_file::load_block_route_file(path)?;
                Self::from_course(&result.course, &result.guide)
            }
            Some(super::route_plan::ROUTE_FORMAT) => {
                let file: super::route_plan::RouteFile =
                    toml::from_str(&text).map_err(|e| format!("invalid route: {e}"))?;
                let result = super::route_plan::load_route_course_full_with_patch_dir(
                    &file,
                    base,
                    std::path::Path::new(super::map_patch::PATCH_DIR),
                    super::map_patch::PatchAudience::Distribution,
                )?;
                Self::from_course(&result.course, &result.guide)
            }
            _ => Err("course format has no supported route guide".into()),
        }
    }

    /// 틱 단위로 차량 위치와 충돌 여부를 반영하여 실시간 CTE를 갱신하고 반환한다.
    pub fn update(&mut self, vehicle_pos: Vec2, collided: bool) -> f32 {
        self.tick_count += 1;
        if collided {
            self.impact_count += 1;
        }
        if let Some(prev) = self.last_pos {
            self.distance_travelled_m += prev.distance(vehicle_pos);
        }
        self.last_pos = Some(vehicle_pos);

        // 이전 구간부터 전방 탐색 창 내에서 가장 가까운 선분 투영 거리를 계산
        let window_end = (self.last_segment + 50).min(self.guide.len() - 1);
        let (best_segment, cte) = (self.last_segment..window_end)
            .map(|k| {
                let (a, b) = (self.guide[k], self.guide[k + 1]);
                let seg_len = a.distance(b);
                let offset = if seg_len < 1e-4 {
                    vehicle_pos.distance(a)
                } else {
                    let t = ((vehicle_pos - a).dot(b - a) / (seg_len * seg_len)).clamp(0.0, 1.0);
                    vehicle_pos.distance(a.lerp(b, t))
                };
                (k, offset)
            })
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .unwrap_or((self.last_segment, 0.0));

        self.last_segment = best_segment;
        self.current_cte_m = cte;
        self.sum_cte_m += cte;
        self.max_cte_m = self.max_cte_m.max(cte);

        // 목표 지점과의 거리 검사
        let dist_to_goal = vehicle_pos.distance(self.goal_position);
        if dist_to_goal <= self.goal_threshold_m {
            self.goal_reached = true;
        }

        cte
    }

    /// 현재 순간의 경로 이탈 오차(CTE, m)를 반환한다.
    pub fn current_cte(&self) -> f32 {
        self.current_cte_m
    }

    /// 목표 지점 도달 여부를 반환한다.
    pub fn is_goal_reached(&self) -> bool {
        self.goal_reached
    }

    /// 현재까지의 주행 메트릭 종합 보고서를 산출한다.
    pub fn finalize(&self, vehicle_pos: Vec2, dt_s: f32) -> RouteEvaluationMetrics {
        let avg_cte_m = if self.tick_count > 0 {
            self.sum_cte_m / self.tick_count as f32
        } else {
            0.0
        };
        let remaining_to_goal_m = vehicle_pos.distance(self.goal_position);

        RouteEvaluationMetrics {
            goal_reached: self.goal_reached,
            elapsed_time_s: self.tick_count as f32 * dt_s,
            tick_count: self.tick_count,
            distance_m: self.distance_travelled_m,
            avg_cte_m,
            max_cte_m: self.max_cte_m,
            impact_count: self.impact_count,
            remaining_to_goal_m,
        }
    }

    /// 상태를 초기화하여 재주행을 준비한다.
    pub fn reset(&mut self) {
        self.last_segment = 0;
        self.tick_count = 0;
        self.impact_count = 0;
        self.sum_cte_m = 0.0;
        self.max_cte_m = 0.0;
        self.last_pos = None;
        self.distance_travelled_m = 0.0;
        self.current_cte_m = 0.0;
        self.goal_reached = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 주행 평가기가 실시간 CTE 오차를 정확히 누적하고 목표 도달을 판정하는지 검증한다.
    #[test]
    fn evaluates_cross_track_error_and_goal_reach() {
        let guide = vec![Vec2::new(0.0, 0.0), Vec2::new(0.0, 100.0)];
        let goal_pos = Vec2::new(0.0, 100.0);
        let mut evaluator = RouteDriveEvaluator::new(&guide, goal_pos, 5.0).unwrap();

        assert!(!evaluator.is_goal_reached());

        // x = 0.5m 오프셋으로 주행 시뮬레이션
        let dt = 1.0 / 64.0;
        for i in 0..100 {
            let z = i as f32 * 0.95; // 0m -> 94.05m
            let pos = Vec2::new(0.5, z);
            let cte = evaluator.update(pos, false);
            assert!((cte - 0.5).abs() < 1e-3, "cte={cte}");
        }

        // 아직 목표(100m)에 도달 전 (z=94.05m, 잔여 5.95m > 5.0m threshold)
        assert!(!evaluator.is_goal_reached());

        // 목표 지점 진입 (z = 97.0m, 잔여 3.0m <= 5.0m)
        evaluator.update(Vec2::new(0.5, 97.0), false);
        assert!(evaluator.is_goal_reached());

        let metrics = evaluator.finalize(Vec2::new(0.5, 97.0), dt);
        assert!(metrics.goal_reached);
        assert!((metrics.avg_cte_m - 0.5).abs() < 1e-3);
        assert!((metrics.max_cte_m - 0.5).abs() < 1e-3);
        assert_eq!(metrics.impact_count, 0);
        assert!(metrics.summary_korean().contains("목표 도달 성공"));
    }
    /// OSM 경로의 안내선을 읽어 평가하고 초기화 후 누적값이 지워지는지 확인한다.
    #[test]
    fn osm_route_evaluation_loads_and_resets() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/yeosu-expo/route-junctions.toml");
        let mut evaluator = RouteDriveEvaluator::load_map_file(&path).unwrap();
        let p = evaluator.guide[0];
        evaluator.update(p, false);
        evaluator.update(p + Vec2::X, true);
        assert!(evaluator.finalize(p, 1.0 / 64.0).distance_m > 0.0);
        evaluator.reset();
        let m = evaluator.finalize(p, 1.0 / 64.0);
        assert_eq!(m.tick_count, 0);
        assert_eq!(m.impact_count, 0);
        assert_eq!(m.distance_m, 0.0);
    }
}
