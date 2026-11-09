//! 주행할 수 있는 지도 코스 파일 목록(GD-3·GD-4 메뉴).
//!
//! 저장소에 들어 있는 지도 코스(`assets/maps/<지역>/*.toml`)와 경로 선택 화면이 저장한 경로(`cache/routes/*.toml`)를
//! 찾아 메뉴에 보일 이름과 코스 선택 문자열(`map:<저장소 루트 기준 경로>`)로 돌려준다. 파일 내용은 읽지 않는다.
//! 열 수 있는지는 코스를 만들 때([`super::map::load_map_course`]) 검사한다.

use std::path::Path;
use std::time::SystemTime;

/// 저장소에 들어 있는 지도 코스 폴더(저장소 루트 기준).
pub const BUNDLED_MAP_DIR: &str = "assets/maps";
/// 경로 선택 화면이 경로를 저장하는 폴더(저장소 루트 기준, [`crate::picker::region::ROUTE_DIR`]와 같다).
pub const SAVED_ROUTE_DIR: &str = crate::picker::region::ROUTE_DIR;

/// 메뉴에 보일 지도 코스 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseFile {
    /// 사람이 읽는 이름. 예: `"yeosu-expo / route-junctions"`, `"저장 경로 / route-20261107T050906Z"`.
    pub label: String,
    /// 코스 선택 문자열. 예: `"map:assets/maps/yeosu-expo/route-junctions.toml"`.
    pub course: String,
    /// 경로 선택 화면이 저장한 경로이면 `true`, 저장소에 들어 있는 코스이면 `false`.
    pub saved: bool,
}

/// `root`(저장소 루트) 아래의 지도 코스 파일을 찾는다. 저장소 코스(폴더·파일 이름 순) 다음에 저장 경로(최근 저장 순).
///
/// 폴더가 없거나 읽지 못하면 그 부분만 빈 목록이다.
pub fn list_course_files(root: &Path) -> Vec<CourseFile> {
    let mut files = Vec::new();
    let mut regions = dir_entries(&root.join(BUNDLED_MAP_DIR))
        .into_iter()
        .filter(|(path, _)| path.is_dir())
        .collect::<Vec<_>>();
    regions.sort_by(|a, b| a.0.cmp(&b.0));
    for (region, _) in regions {
        let region_name = file_name(&region);
        let mut tomls = toml_files(&region);
        tomls.sort_by(|a, b| a.0.cmp(&b.0));
        for (path, _) in tomls {
            files.push(CourseFile {
                label: format!("{region_name} / {}", file_stem(&path)),
                course: format!("map:{BUNDLED_MAP_DIR}/{region_name}/{}", file_name(&path)),
                saved: false,
            });
        }
    }
    let mut saved = toml_files(&root.join(SAVED_ROUTE_DIR));
    // 최근에 저장한 것이 위로 온다. 시각이 같으면 이름 역순(타임스탬프 이름이면 최근 순)이다.
    saved.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    for (path, _) in saved {
        files.push(CourseFile {
            label: format!("저장 경로 / {}", file_stem(&path)),
            course: format!("map:{SAVED_ROUTE_DIR}/{}", file_name(&path)),
            saved: true,
        });
    }
    files
}

/// 폴더의 항목과 수정 시각을 읽는다. 읽지 못하면 빈 목록이다.
fn dir_entries(dir: &Path) -> Vec<(std::path::PathBuf, SystemTime)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| {
            let modified = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (entry.path(), modified)
        })
        .collect()
}

/// 폴더 안의 `.toml` 파일과 수정 시각을 읽는다.
fn toml_files(dir: &Path) -> Vec<(std::path::PathBuf, SystemTime)> {
    dir_entries(dir)
        .into_iter()
        .filter(|(path, _)| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "toml")
        })
        .collect()
}

/// 경로의 마지막 이름(UTF-8이 아니면 손실 변환).
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 확장자를 뺀 파일 이름.
fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 저장소 코스는 폴더·이름 순, 저장 경로는 그 뒤에 오고 `.toml`이 아닌 파일은 빠지는지 확인한다.
    #[test]
    fn lists_bundled_then_saved_courses() {
        let root = std::env::temp_dir().join(format!("sds-course-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in [
            "assets/maps/b-region",
            "assets/maps/a-region",
            "cache/routes",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in [
            "assets/maps/b-region/course.toml",
            "assets/maps/a-region/z.toml",
            "assets/maps/a-region/m.toml",
            "assets/maps/a-region/README.md",
            "cache/routes/route-1.toml",
        ] {
            std::fs::write(root.join(file), "").unwrap();
        }
        let files = list_course_files(&root);
        let courses: Vec<&str> = files.iter().map(|file| file.course.as_str()).collect();
        assert_eq!(
            courses,
            [
                "map:assets/maps/a-region/m.toml",
                "map:assets/maps/a-region/z.toml",
                "map:assets/maps/b-region/course.toml",
                "map:cache/routes/route-1.toml",
            ]
        );
        assert_eq!(files[0].label, "a-region / m");
        assert!(!files[0].saved);
        assert_eq!(files[3].label, "저장 경로 / route-1");
        assert!(files[3].saved);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 폴더가 없으면 빈 목록인지 확인한다.
    #[test]
    fn missing_folders_give_empty_list() {
        let root = std::env::temp_dir().join("sds-course-files-missing-folder");
        assert!(list_course_files(&root).is_empty());
    }

    /// 저장소에 들어 있는 여수 경로 코스가 목록에 있고 실제로 코스를 만들 수 있는지 확인한다.
    #[test]
    fn bundled_yeosu_route_is_listed_and_loads() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let files = list_course_files(root);
        let route = files
            .iter()
            .find(|file| file.course == "map:assets/maps/yeosu-expo/route-junctions.toml")
            .expect("bundled route listed");
        let path = root.join(route.course.strip_prefix("map:").unwrap());
        let course = super::super::map::load_map_course(&path).unwrap();
        course.validate().unwrap();
        assert!(!course.areas.is_empty(), "route course has junction areas");
        assert!(
            course
                .notices
                .iter()
                .any(|notice| notice.contains("OpenStreetMap"))
        );
    }
}
