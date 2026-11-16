//! Lanelet2 지도 전체를 열고 출발·도착 차선으로 검증된 코스 설정을 만든다.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::lanelet2_course::{Lanelet2CourseFile, LaneletCourse, build_lanelet_course};
use super::lanelet2_geometry::{LaneletGeometryMap, build_lanelet_geometry};
use super::{lanelet2, map::LatLon};

/// 원본 설정과 전체 차선 형상을 보존하는 선택 모델. 미리보기는 파일을 변경하지 않는다.
pub struct LaneletSelection {
    pub file: Lanelet2CourseFile,
    pub geometry: LaneletGeometryMap,
    source: PathBuf,
}

impl LaneletSelection {
    /// 코스 설정에서 전체 차선 형상을 읽는다. 기본 경로가 막혀 있어도 지도는 열 수 있다.
    pub fn open(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let file: Lanelet2CourseFile = toml::from_str(&text).map_err(|e| e.to_string())?;
        file.validate()?;
        let source = path
            .parent()
            .unwrap_or(Path::new("."))
            .join(&file.osm)
            .canonicalize()
            .map_err(|e| format!("cannot read Lanelet2 OSM: {e}"))?;
        let bytes = std::fs::read(&source).map_err(|e| e.to_string())?;
        let parsed = lanelet2::parse_lanelet2(&bytes)?;
        let geometry = build_lanelet_geometry(
            &parsed,
            LatLon {
                lat: file.origin[0],
                lon: file.origin[1],
            },
        )?;
        Ok(Self {
            file,
            geometry,
            source,
        })
    }

    /// 선택한 차선 쌍의 통행 방향·연결·충돌 형상을 기존 코스 생성기로 검증한다.
    pub fn preview(&self, start: i64, goal: i64) -> Result<LaneletCourse, String> {
        let mut notices = Vec::new();
        notices.extend(self.file.attribution.iter().cloned());
        notices.extend(self.file.terms.iter().cloned());
        notices.extend(
            self.file
                .notices
                .iter()
                .filter(|s| !s.trim().is_empty())
                .cloned(),
        );
        notices.retain(|s| !s.trim().is_empty());
        build_lanelet_course(&self.geometry, &self.file.name, start, goal, notices)
    }

    /// 검증한 선택을 새 TOML로 저장한다. 기존 파일을 덮어쓰지 않고 원본 출처·원점은 유지한다.
    pub fn save(&self, start: i64, goal: i64, path: &Path) -> Result<(), String> {
        self.preview(start, goal)?;
        let parent = path.parent().ok_or("route path has no parent")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let parent = parent.canonicalize().map_err(|e| e.to_string())?;
        let mut file = self.file.clone();
        file.start_lanelet = start;
        file.goal_lanelet = goal;
        file.osm = relative_source(&parent, &self.source)
            .to_string_lossy()
            .into_owned();
        let text = toml::to_string_pretty(&file).map_err(|e| e.to_string())?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| format!("cannot create route (existing files are preserved): {e}"))?;
        output.write_all(text.as_bytes()).map_err(|e| e.to_string())
    }
}

/// 같은 볼륨에서는 상대 경로로 저장하고 다른 볼륨이면 절대 원본 경로를 보존한다.
fn relative_source(parent: &Path, source: &Path) -> PathBuf {
    for (depth, ancestor) in parent.ancestors().enumerate() {
        if let Ok(tail) = source.strip_prefix(ancestor) {
            return (0..depth)
                .fold(PathBuf::new(), |p, _| p.join(".."))
                .join(tail);
        }
    }
    source.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 실제 OSM부터 선택·저장·재로드하고 역방향 실패와 덮어쓰기 방지를 검사한다.
    #[test]
    fn selection_round_trip_and_invalid_route() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model =
            LaneletSelection::open(&root.join("assets/maps/lanelet2-demo/course.toml")).unwrap();
        assert_eq!(model.geometry.lanelets.len(), 2);
        let expected = model.preview(301, 302).unwrap();
        assert!(model.preview(302, 301).is_err());
        assert!(model.preview(9999, 302).is_err());
        let dir = std::env::temp_dir().join(format!("lanelet-selection-{}", std::process::id()));
        let path = dir.join("selected.toml");
        model.save(301, 302, &path).unwrap();
        assert!(model.save(301, 302, &path).is_err());
        let loaded = super::super::lanelet2_course::load_lanelet2_course_file(&path).unwrap();
        assert_eq!(loaded, expected);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
