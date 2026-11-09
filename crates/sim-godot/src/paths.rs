//! 저장소 루트 기준 경로 찾기.
//!
//! Bevy 앱은 저장소 루트에서 실행해 `config/`·`cache/` 같은 상대 경로를 그대로 쓴다. Godot은 실행 폴더가
//! 일정하지 않으므로, Godot 프로젝트(`res://`)의 부모 폴더인 저장소 루트를 기준으로 바꾼다.

use std::path::PathBuf;

use godot::classes::ProjectSettings;
use godot::obj::Singleton;

/// 저장소 루트(`res://..`)의 실제 경로를 반환한다.
pub fn repo_root() -> PathBuf {
    PathBuf::from(
        ProjectSettings::singleton()
            .globalize_path("res://..")
            .to_string(),
    )
}

/// 저장소 루트 기준 상대 경로(예: `cache/tiles`)를 실제 경로로 바꾼다.
pub fn repo_path(relative: &str) -> PathBuf {
    repo_root().join(relative)
}

/// 사용자가 준 경로를 실제 경로로 바꾼다. `res://`·`user://`는 Godot이 바꾸고, 상대 경로는 저장소 루트 기준이다.
pub fn resolve_user_path(path: &str) -> PathBuf {
    if path.starts_with("res://") || path.starts_with("user://") {
        PathBuf::from(
            ProjectSettings::singleton()
                .globalize_path(path)
                .to_string(),
        )
    } else {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            repo_root().join(path)
        }
    }
}
