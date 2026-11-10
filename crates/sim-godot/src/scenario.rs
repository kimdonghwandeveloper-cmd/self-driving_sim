//! 시나리오 파일 목록을 메뉴에 주는 `SimScenario`(계약 v1.4, Phase 5-1).
//!
//! 실행은 `SimVehicle.scenario` 속성으로 한다. 여기서는 차량 없이 필요한 정적 함수만 둔다.

use godot::prelude::*;
use sim_core::scenario;

use crate::paths::repo_root;

/// 차량 없이 쓰는 시나리오 정적 함수 모음.
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct SimScenario {}

#[godot_api]
impl SimScenario {
    /// 메뉴에 보일 시나리오 목록 `[{label, path}]`(`assets/scenarios/*.toml`, 파일 이름 순).
    /// `path`는 저장소 루트 기준 경로이며 `SimVehicle.scenario`에 그대로 넣는다. 읽지 못한 파일은 이름 뒤에 `(오류)`가 붙는다.
    #[func]
    fn scenario_files() -> VarArray {
        scenario::list_scenario_files(&repo_root())
            .into_iter()
            .map(|file| {
                vdict! {
                    "label" => file.label.as_str(),
                    "path" => file.path.as_str(),
                }
                .to_variant()
            })
            .collect()
    }
}
