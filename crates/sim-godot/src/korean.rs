//! Godot 화면에 보낼 사람이 읽는 문구를 한국어로 바꾼다(GD-4, 계약 v1.2).
//!
//! `sim-core` 문구는 시험과 개발 명령(`sim-tools`)이 함께 쓰므로 영어로 둔다. Godot 경계에서만 알려진 문장을
//! 한국어로 바꾸고, 모르는 문장은 그대로 보낸다. 오류 상세(노드 번호·파일 경로 등)는 원문을 뒤에 붙여 둔다.
//! OSM 출처 표기 문장은 라이선스 표기이므로 바꾸지 않는다.

use sim_core::scenario::run::{Outcome, Phase};
use sim_core::vehicle::calibration::{ActiveTrial, CalibrationReadout};

/// 상태 머신·장치 명령 거부 사유를 한국어로 바꾼다.
pub fn reject_reason(reason: &str) -> String {
    let fixed = match reason {
        "engine is off" => Some("시동이 꺼져 있습니다"),
        "emergency stop is active" => Some("비상 정지 중입니다"),
        "vehicle must be fully stopped" | "vehicle must be stopped" => {
            Some("차량이 완전히 멈춰야 합니다")
        }
        "emergency stop is not active" => Some("비상 정지 상태가 아닙니다"),
        "speed value is invalid" => Some("속도 값이 올바르지 않습니다"),
        _ => None,
    };
    if let Some(text) = fixed {
        return text.to_owned();
    }
    if let Some(gears) = between(reason, "simultaneous gear keys (", ")") {
        return format!("기어 키를 동시에 눌렀습니다({gears})");
    }
    reason.to_owned()
}

/// 코스 안내(임시 폭·가상 코스)를 한국어로 바꾼다. 출처 표기는 그대로 둔다.
pub fn course_notice(notice: &str) -> String {
    if let Some(translated) = map_patch_line(notice) {
        return translated;
    }
    match notice {
        "Virtual training course from an OSM route (not a surveyed road)" => {
            return "OSM 경로로 만든 가상 교육 코스입니다(실측 도로 아님)".to_owned();
        }
        "Virtual training course from OSM centerline (not a surveyed road)" => {
            return "OSM 중심선으로 만든 가상 교육 코스입니다(실측 도로 아님)".to_owned();
        }
        "One-way and divided roads are drawn as a single carriageway (not the real lane layout)" => {
            return "일방통행·왕복 분리 도로는 한 줄 도로로 그립니다(실제 차로 구성 아님)"
                .to_owned();
        }
        _ => {}
    }
    if let Some(count) = between(
        notice,
        "Simplified ",
        " intersection(s): side roads with unsupported shapes are not drawn",
    ) {
        return format!("교차로 {count}곳 단순화: 형상을 지원하지 않는 곁가지는 그리지 않습니다");
    }
    if let Some(rest) = notice.strip_prefix("Road width ") {
        if let Some((width, source)) = split_once_between(rest, " m is a temporary value (", ")") {
            return format!("도로 폭 {width} m는 임시값입니다({source})");
        }
        if let Some(width) = rest.strip_suffix(" m is a temporary training value") {
            return format!("도로 폭 {width} m는 훈련용 임시값입니다");
        }
        if let Some(width) = rest.strip_suffix(" m from the OSM width tag") {
            return format!("도로 폭 {width} m는 OSM 폭 태그 값입니다");
        }
    }
    notice.to_owned()
}

/// 전국 장소 검색(4-10) 오류를 한국어로 바꾼다. 모르는 오류는 앞에 "전국 검색 실패:"를 붙여 그대로 둔다.
pub fn search_error(error: &str) -> String {
    if error == "this search is not saved and the network is off" {
        return "전국 검색: 네트워크가 꺼져 있고 저장한 결과도 없습니다".to_owned();
    }
    if error == "empty search text" {
        return "검색어를 입력하세요".to_owned();
    }
    if let Some(status) = error.strip_prefix("the place search server answered ") {
        return format!("전국 검색 서버가 응답 코드 {status}로 거절했습니다(잠시 뒤 다시 시도)");
    }
    format!("전국 검색 실패: {error}")
}

/// 전국 자료에서 지역 열기(4-11) 오류를 한국어로 바꾼다. 모르는 오류는 앞에 "지역을 열 수 없습니다:"를 붙인다.
pub fn region_error(error: &str) -> String {
    if error.starts_with("cannot read") && error.contains("manifest.toml") {
        return "전국 지도 자료가 없습니다(data/korea). README의 \"전국 지도 자료 만들기\"대로 만드세요"
            .to_owned();
    }
    if error.contains("rebuild the data") || error.contains("does not match manifest.toml") {
        return "전국 지도 자료가 이 프로그램과 맞지 않습니다. 다시 만드세요".to_owned();
    }
    if error == "no roads in this area" {
        return "이 지역에는 도로가 없습니다".to_owned();
    }
    if error.starts_with("invalid point") {
        return "지도 밖입니다".to_owned();
    }
    format!("지역을 열 수 없습니다: {error}")
}

/// 경로 선택 화면 안내·경고·경로 오류 한 줄을 한국어로 바꾼다.
pub fn picker_line(line: &str) -> String {
    if let Some(translated) = map_patch_line(line) {
        return translated;
    }
    if let Some(count) = line.strip_prefix("Points: ") {
        return format!("찍은 점: {count}개");
    }
    if line == "Road data: no boundary connectivity record (Overpass snapshot)" {
        return "도로 자료: 경계 연결 기록 없음(Overpass 스냅샷)".to_owned();
    }
    if let Some(count) = between(
        line,
        "Road data: Geofabrik extract, boundary connections checked (",
        " incomplete node(s) near the edge)",
    ) {
        return format!(
            "도로 자료: Geofabrik 추출, 경계 연결 확인됨(가장자리 미완성 노드 {count}개)"
        );
    }
    if line == "Turn restrictions: none in this snapshot" {
        return "회전 제한: 이 스냅샷에 없음".to_owned();
    }
    if let Some(count) = between(line, "Turn restrictions: ", " applied") {
        return format!("회전 제한: {count}개 반영");
    }
    if let Some(count) = between(
        line,
        "Detour: avoided ",
        " intersection(s) that cannot be built",
    ) {
        return format!("우회: 만들 수 없는 교차로 {count}곳을 피해 감");
    }
    if line.starts_with("Simplified ") {
        return course_notice(line);
    }
    if line == "Places: not loaded" {
        return "건물·장소: 받지 않음(\"건물·장소 받기\")".to_owned();
    }
    if let Some(count) = between(line, "Places: ", " (click a building or search by name)") {
        return format!("건물·장소: {count}곳(건물 클릭 또는 이름 검색)");
    }
    for (english, korean) in [
        ("Start: ", "출발: "),
        ("End: ", "도착: "),
        ("Via: ", "경유: "),
    ] {
        if let Some(name) = line.strip_prefix(english) {
            return format!("{korean}{name}");
        }
    }
    if line == "Click at least 2 points on the roads (start, end)" {
        return "도로 위에 점을 2개 이상 찍으세요(출발, 도착)".to_owned();
    }
    if let Some((length, junctions)) = split_once_between(
        line.strip_prefix("Route: ").unwrap_or(""),
        " m, ",
        " intersection(s)",
    ) {
        return format!("경로: {length} m, 교차로 {junctions}개");
    }
    if let Some(rest) = line.strip_prefix("Width: ") {
        return format!("도로 폭: {rest}");
    }
    if line == "Note: width is a temporary value, not from an OSM width tag" {
        return "참고: 폭은 OSM 폭 태그가 아닌 임시값입니다".to_owned();
    }
    if let Some(detail) = between(line, "Warning: narrow road (", ")") {
        return format!("경고: 좁은 도로({detail})");
    }
    for (english, korean) in [
        ("Turn the vehicle cannot make: ", "차량이 돌 수 없는 꺾임: "),
        ("Intersection rejected: ", "교차로 거부: "),
        ("Roads overlap: ", "도로 겹침: "),
        ("Route error: ", "경로 오류: "),
    ] {
        if let Some(detail) = line.strip_prefix(english) {
            return format!("{korean}{detail}");
        }
    }
    line.to_owned()
}

/// 지역 보정의 자료 출처·이름·확인 상태·실측 설명·경고를 한국어 안내로 바꾼다.
fn map_patch_line(line: &str) -> Option<String> {
    if line == "Map source: OSM" {
        return Some("지도 자료: OSM".to_owned());
    }
    if line == "Map source: OSM + local patches" {
        return Some("지도 자료: OSM + 지역 보정".to_owned());
    }
    if let Some(detail) = line.strip_prefix("Map patch: ") {
        let detail = detail
            .replace("status: verified", "확인 상태: 확인됨")
            .replace("status: provisional", "확인 상태: 임시")
            .replace("status: synthetic", "확인 상태: 합성 시험")
            .replace("geometry accuracy: ", "형상 정확도·실측 여부: ");
        return Some(format!("지역 보정: {detail}"));
    }
    if let Some(detail) = line.strip_prefix("Map patch notice: ") {
        return Some(format!("지역 보정 안내: {detail}"));
    }
    if let Some(detail) = line.strip_prefix("Map patch warning: ") {
        return Some(format!("지역 보정 경고: {detail}"));
    }
    None
}

/// 보정 표시값을 HUD용 한국어 줄로 만든다(2-5, 지운 Bevy 판 보정 줄과 같은 내용). 모두 시뮬레이터 임시값이다.
pub fn calibration_lines(readout: &CalibrationReadout) -> Vec<String> {
    let turn = match readout.turn_radius_m {
        Some(radius) => format!("{radius:.2} m"),
        None => "직진".to_owned(),
    };
    let active = match readout.active {
        Some(ActiveTrial::Braking { distance_m, time_s }) => {
            format!("제동 {distance_m:.2} m / {time_s:.2} 초")
        }
        Some(ActiveTrial::Accelerating { distance_m, time_s }) => {
            format!("가속 {distance_m:.2} m / {time_s:.2} 초")
        }
        None => "-".to_owned(),
    };
    let (stops, accels) = readout.trials;
    let last_stop = match readout.last_stop {
        Some(stop) => format!(
            "{stops}회차 {:.2} m/s에서 {:.2} m, {:.2} 초 ({})",
            stop.from_speed_mps,
            stop.distance_m,
            stop.time_s,
            if stop.emergency {
                "비상 정지"
            } else {
                "제동"
            }
        ),
        None => "-".to_owned(),
    };
    let last_accel = match readout.last_accel {
        Some(accel) => format!(
            "{accels}회차 0 → {:.2} m/s, {:.2} 초, {:.2} m ({})",
            accel.to_speed_mps,
            accel.time_s,
            accel.distance_m,
            if accel.reached_limit {
                "최고 속도"
            } else {
                "가속 뗌"
            }
        ),
        None => "-".to_owned(),
    };
    vec![
        format!(
            "회전반경(후륜축): {turn}    최대 조향 최소 반경(저속): {:.2} m",
            readout.min_turn_radius_m
        ),
        format!("최대 제동 예상 정지거리: {:.2} m", readout.brake_estimate_m),
        format!("진행 중 시험: {active}"),
        format!("마지막 제동 시험: {last_stop}"),
        format!("마지막 가속 시험: {last_accel}"),
    ]
}

/// 시나리오 실행 단계(끝났으면 끝난 방식)를 HUD용 한국어로 바꾼다(5-1).
pub fn scenario_phase(phase: Phase) -> &'static str {
    match phase {
        Phase::Briefing => "시작 전",
        Phase::Running => "진행 중",
        Phase::Paused => "일시정지",
        Phase::Finished(Outcome::Success) => "성공",
        Phase::Finished(Outcome::TimeLimit) => "실패 · 시간 초과",
        Phase::Finished(Outcome::Collisions) => "실패 · 허용 충돌 횟수 초과",
        Phase::Finished(Outcome::Aborted) => "중단",
    }
}

/// `text`가 `prefix`로 시작하고 `suffix`로 끝나면 그 사이를 돌려준다.
fn between<'a>(text: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    text.strip_prefix(prefix)?.strip_suffix(suffix)
}

/// `text`가 `<앞>{middle}<뒤>{suffix}` 꼴이면 (앞, 뒤)를 돌려준다.
fn split_once_between<'a>(text: &'a str, middle: &str, suffix: &str) -> Option<(&'a str, &'a str)> {
    let (head, tail) = text.split_once(middle)?;
    Some((head, tail.strip_suffix(suffix)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::world::map::OSM_ATTRIBUTION;

    /// 보정 줄이 직진·진행 중 시험 없음과 끝난 결과(회차·비상 정지·최고 속도)를 한국어로 쓰는지 확인한다.
    #[test]
    fn calibration_lines_translate() {
        use sim_core::vehicle::calibration::{AccelResult, StopResult};
        let empty = calibration_lines(&CalibrationReadout::default());
        assert_eq!(empty.len(), 5);
        assert!(empty[0].starts_with("회전반경(후륜축): 직진"), "{empty:?}");
        assert_eq!(empty[2], "진행 중 시험: -");
        let lines = calibration_lines(&CalibrationReadout {
            turn_radius_m: Some(9.5),
            min_turn_radius_m: 4.85,
            brake_estimate_m: 1.25,
            active: Some(ActiveTrial::Braking {
                distance_m: 0.5,
                time_s: 0.25,
            }),
            last_stop: Some(StopResult {
                from_speed_mps: 2.0,
                distance_m: 0.6,
                time_s: 0.6,
                emergency: true,
            }),
            last_accel: Some(AccelResult {
                to_speed_mps: 5.0,
                distance_m: 12.5,
                time_s: 5.0,
                reached_limit: true,
            }),
            trials: (3, 2),
        });
        assert_eq!(
            lines[0],
            "회전반경(후륜축): 9.50 m    최대 조향 최소 반경(저속): 4.85 m"
        );
        assert_eq!(lines[1], "최대 제동 예상 정지거리: 1.25 m");
        assert_eq!(lines[2], "진행 중 시험: 제동 0.50 m / 0.25 초");
        assert_eq!(
            lines[3],
            "마지막 제동 시험: 3회차 2.00 m/s에서 0.60 m, 0.60 초 (비상 정지)"
        );
        assert_eq!(
            lines[4],
            "마지막 가속 시험: 2회차 0 → 5.00 m/s, 5.00 초, 12.50 m (최고 속도)"
        );
    }

    /// 상태 머신 거부 사유와 장치 기어 충돌을 바꾸고, 모르는 사유는 그대로 두는지 확인한다.
    #[test]
    fn reject_reasons_translate() {
        assert_eq!(reject_reason("engine is off"), "시동이 꺼져 있습니다");
        assert_eq!(
            reject_reason(sim_core::session::RESET_NOT_STOPPED),
            "차량이 완전히 멈춰야 합니다"
        );
        assert_eq!(
            reject_reason("simultaneous gear keys (R+D)"),
            "기어 키를 동시에 눌렀습니다(R+D)"
        );
        assert_eq!(reject_reason("something new"), "something new");
    }

    /// `sim-core` 상태 머신의 모든 거부 사유가 번역표에 있는지 확인한다(문구가 바뀌면 여기서 잡힌다).
    #[test]
    fn every_transition_error_has_korean() {
        use sim_core::vehicle::state::TransitionError;
        for error in [
            TransitionError::EngineOff,
            TransitionError::EmergencyStopActive,
            TransitionError::NotStopped,
            TransitionError::EmergencyStopNotActive,
            TransitionError::InvalidSpeed,
        ] {
            let english = error.to_string();
            assert_ne!(reject_reason(&english), english, "{english}");
        }
    }

    /// 코스 안내를 바꾸되 출처 표기는 그대로 두는지 확인한다.
    #[test]
    fn course_notices_translate_except_attribution() {
        assert_eq!(course_notice(OSM_ATTRIBUTION), OSM_ATTRIBUTION);
        assert_eq!(
            course_notice("Road width 4.0 m is a temporary value (default)"),
            "도로 폭 4.0 m는 임시값입니다(default)"
        );
        assert_eq!(
            course_notice("Road width 5.5 m from the OSM width tag"),
            "도로 폭 5.5 m는 OSM 폭 태그 값입니다"
        );
        assert_eq!(
            course_notice("Road width 4.0 m is a temporary training value"),
            "도로 폭 4.0 m는 훈련용 임시값입니다"
        );
        assert!(
            course_notice("Virtual training course from an OSM route (not a surveyed road)")
                .contains("가상 교육 코스")
        );
        assert_eq!(
            course_notice("Map source: OSM + local patches"),
            "지도 자료: OSM + 지역 보정"
        );
        assert!(
            course_notice("Map patch: 연결 통로 (status: verified, geometry accuracy: 실측 아님)")
                .contains("확인 상태: 확인됨")
        );
    }

    /// 저장소 여수 경로 코스의 실제 안내가 출처 표기 말고 모두 한국어로 바뀌는지 확인한다.
    #[test]
    fn bundled_route_notices_are_korean() {
        let path = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../testdata/yeosu-expo/route-junctions.toml"
        ));
        let course = sim_core::world::map::load_map_course(path).unwrap();
        for notice in &course.notices {
            if notice != OSM_ATTRIBUTION {
                assert_ne!(&course_notice(notice), notice, "{notice}");
            }
        }
    }

    /// 경로 선택 화면 안내·경고·오류 줄을 바꾸는지 확인한다.
    #[test]
    fn picker_lines_translate() {
        assert_eq!(picker_line("Points: 3"), "찍은 점: 3개");
        assert_eq!(
            picker_line("Route: 261 m, 2 intersection(s)"),
            "경로: 261 m, 교차로 2개"
        );
        assert_eq!(
            picker_line("Width: 4.0 m (default)"),
            "도로 폭: 4.0 m (default)"
        );
        assert_eq!(
            picker_line("Warning: narrow road (2.8 m < 3 m)"),
            "경고: 좁은 도로(2.8 m < 3 m)"
        );
        assert_eq!(
            picker_line("Intersection rejected: node 5 has 6 arms"),
            "교차로 거부: node 5 has 6 arms"
        );
        assert_eq!(
            picker_line(
                "Road data: Geofabrik extract, boundary connections checked (4 incomplete node(s) near the edge)"
            ),
            "도로 자료: Geofabrik 추출, 경계 연결 확인됨(가장자리 미완성 노드 4개)"
        );
        assert_eq!(
            picker_line("지역 도로를 열었습니다"),
            "지역 도로를 열었습니다"
        );
        // 4-9 건물·장소 안내.
        assert_eq!(
            picker_line("Places: 3 (click a building or search by name)"),
            "건물·장소: 3곳(건물 클릭 또는 이름 검색)"
        );
        assert_eq!(picker_line("Start: 도서관"), "출발: 도서관");
        assert_eq!(picker_line("End: way/5"), "도착: way/5");
        // 4-8 자동 경로 안내.
        assert_eq!(
            picker_line("Turn restrictions: 3 applied"),
            "회전 제한: 3개 반영"
        );
        assert_eq!(
            picker_line("Turn restrictions: none in this snapshot"),
            "회전 제한: 이 스냅샷에 없음"
        );
        assert_eq!(
            picker_line("Detour: avoided 2 intersection(s) that cannot be built"),
            "우회: 만들 수 없는 교차로 2곳을 피해 감"
        );
        assert_eq!(
            picker_line(
                "Simplified 1 intersection(s): side roads with unsupported shapes are not drawn"
            ),
            "교차로 1곳 단순화: 형상을 지원하지 않는 곁가지는 그리지 않습니다"
        );
        assert_eq!(
            picker_line("Map source: OSM + local patches"),
            "지도 자료: OSM + 지역 보정"
        );
        assert_eq!(
            picker_line("Map patch notice: 실제 도로 아님"),
            "지역 보정 안내: 실제 도로 아님"
        );
    }

    /// 전국 검색 오류는 아는 것만 한국어로 바꾸고 나머지는 앞말을 붙인다.
    #[test]
    fn search_errors_translate() {
        assert!(
            search_error("this search is not saved and the network is off").contains("네트워크")
        );
        assert!(search_error("the place search server answered 429").contains("429"));
        assert_eq!(search_error("boom"), "전국 검색 실패: boom");
    }

    /// 지역 열기 오류: 자료 없음·형식 다름·도로 없음은 한국어 안내, 나머지는 앞말을 붙인다.
    #[test]
    fn region_errors_translate() {
        assert!(region_error("cannot read data/korea/manifest.toml: not found").contains("README"));
        assert!(
            region_error("x has format `y`; this program reads `z` (rebuild the data)")
                .contains("다시 만드세요")
        );
        assert_eq!(
            region_error("no roads in this area"),
            "이 지역에는 도로가 없습니다"
        );
        assert_eq!(region_error("boom"), "지역을 열 수 없습니다: boom");
    }

    /// 시나리오 단계·끝난 방식이 모두 한국어로 바뀌는지 확인한다.
    #[test]
    fn scenario_phase_translates() {
        assert_eq!(scenario_phase(Phase::Briefing), "시작 전");
        assert_eq!(scenario_phase(Phase::Paused), "일시정지");
        assert_eq!(scenario_phase(Phase::Finished(Outcome::Success)), "성공");
        assert_eq!(
            scenario_phase(Phase::Finished(Outcome::TimeLimit)),
            "실패 · 시간 초과"
        );
    }
}
