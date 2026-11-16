//! Lanelet2 차선 선택 모델의 Godot 연결. 경로 검증은 sim-core에 맡긴다.
use crate::paths::{repo_path, resolve_user_path};
use godot::prelude::*;
use sim_core::world::lanelet2_selection::LaneletSelection;

/// 지도 전체 형상과 선택 경로 미리보기를 제공하는 독립 세션.
#[derive(GodotClass)]
#[class(init, base=RefCounted)]
pub struct SimLaneletPicker {
    model: Option<LaneletSelection>,
}

/// 지면 점 배열을 지도 화면의 x·z 좌표로 옮긴다.
fn points(values: &[glam::Vec2]) -> PackedVector2Array {
    values.iter().map(|p| Vector2::new(p.x, p.y)).collect()
}

/// 실패 시 이전 지도나 경로를 성공으로 표시하지 않도록 명시적 결과를 만든다.
fn failure(error: &str) -> VarDictionary {
    vdict! { "ok" => false, "error" => error }
}

#[godot_api]
impl SimLaneletPicker {
    /// 설정 파일에서 전체 차선 목록과 출처·경고를 읽는다. 실패하면 이전 모델을 폐기한다.
    #[func]
    fn open(&mut self, path: GString) -> VarDictionary {
        self.model = None;
        match LaneletSelection::open(&resolve_user_path(&path.to_string())) {
            Ok(model) => {
                let lanes: VarArray = model.geometry.lanelets.values().map(|lane| {
                    vdict! { "id" => lane.id, "left" => points(&lane.surface.left),
                        "right" => points(&lane.surface.right), "center" => points(&lane.centerline) }.to_variant()
                }).collect();
                let mut notices = model.file.notices.clone();
                notices.extend(model.file.attribution.iter().cloned());
                notices.extend(model.file.terms.iter().cloned());
                notices.extend(model.geometry.warnings.iter().cloned());
                let notices: PackedStringArray =
                    notices.iter().map(|s| GString::from(s.as_str())).collect();
                let result = vdict! { "ok" => true, "lanes" => lanes, "notices" => notices };
                self.model = Some(model);
                result
            }
            Err(error) => failure(&error),
        }
    }

    /// 선택한 출발·도착 사이의 유효한 경로와 실제 시작·목표 자세를 반환한다.
    #[func]
    fn preview(&self, start: i64, goal: i64) -> VarDictionary {
        let Some(model) = &self.model else {
            return failure("먼저 Lanelet2 지도를 여세요");
        };
        match model.preview(start, goal) {
            Ok(course) => {
                let ids: PackedInt64Array = course.traversal.iter().map(|s| s.id).collect();
                let start = course.course.start;
                let goal = course.course.goal.expect("validated course has goal");
                vdict! { "ok" => true, "ids" => ids, "guide" => points(&course.guide),
                "start" => Vector2::new(start.x_m, start.z_m), "goal" => Vector2::new(goal.x_m, goal.z_m) }
            }
            Err(error) => failure(&error),
        }
    }

    /// 검증된 선택을 충돌하지 않는 캐시 파일에 저장해 기존 주행 로더용 식별자를 반환한다.
    #[func]
    fn save(&self, start: i64, goal: i64) -> VarDictionary {
        let Some(model) = &self.model else {
            return failure("먼저 Lanelet2 지도를 여세요");
        };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let relative = format!(
            "cache/lanelet2-routes/selected-{}-{stamp}.toml",
            std::process::id()
        );
        match model.save(start, goal, &repo_path(&relative)) {
            Ok(()) => vdict! { "ok" => true, "course" => format!("map:{relative}").as_str() },
            Err(error) => failure(&error),
        }
    }
}
