//! Godot 화면에 보낼 사람이 읽는 문구를 한국어로 바꾼다(GD-4, 계약 v1.2).
//!
//! `sim-core` 문구는 Bevy 판(한글 글꼴 없음)과 시험이 함께 쓰므로 영어로 둔다. Godot 경계에서만 알려진 문장을
//! 한국어로 바꾸고, 모르는 문장은 그대로 보낸다. 오류 상세(노드 번호·파일 경로 등)는 원문을 뒤에 붙여 둔다.
//! OSM 출처 표기 문장은 라이선스 표기이므로 바꾸지 않는다.

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
    match notice {
        "Virtual training course from an OSM route (not a surveyed road)" => {
            return "OSM 경로로 만든 가상 교육 코스입니다(실측 도로 아님)".to_owned();
        }
        "Virtual training course from OSM centerline (not a surveyed road)" => {
            return "OSM 중심선으로 만든 가상 교육 코스입니다(실측 도로 아님)".to_owned();
        }
        _ => {}
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

/// 경로 선택 화면 안내·경고·경로 오류 한 줄을 한국어로 바꾼다.
pub fn picker_line(line: &str) -> String {
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
    }

    /// 저장소 여수 경로 코스의 실제 안내가 출처 표기 말고 모두 한국어로 바뀌는지 확인한다.
    #[test]
    fn bundled_route_notices_are_korean() {
        let path = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/maps/yeosu-expo/route-junctions.toml"
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
    }
}
