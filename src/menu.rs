//! 메인 화면(Phase 4-5a).
//!
//! 인자 없이 실행하면 이 화면에서 시작한다. 메뉴:
//! - 주행하기: 개발 코스(training·calibration) 또는 저장된 경로 파일(`cache/routes/*.toml`, 최근 것부터).
//! - 맵 생성하기: 경로 선택 화면(전국 배경 지도)으로 간다.
//! - 종료.
//!
//! 주행·경로 선택 화면에서 `Esc`를 두 번(3초 안에) 누르면 메인 화면으로 돌아온다. 화면 엔티티는 상태 수명
//! (`DespawnOnExit`)에 묶여 있고, 주행에 다시 들어갈 때 [`crate::reset_driving_session`]이 이전 상태를 지운다.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bevy::prelude::*;

use crate::AppMode;
use crate::picker::{self, Picker};
use crate::world::course::{CourseDef, CourseId};
use crate::world::{ActiveCourse, CourseSelection, map};

/// 메뉴에 보일 저장 경로 파일 수.
const MAX_ROUTES: usize = 8;
/// `Esc`를 두 번 눌러야 하는 시간(s).
const ESC_CONFIRM_S: f32 = 3.0;

/// 메인 화면 플러그인.
pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    /// 메뉴 화면 구성(상태 진입 시)과 버튼 처리, 다른 화면에서의 `Esc` 복귀를 등록한다.
    fn build(&self, app: &mut App) {
        app.init_resource::<EscConfirm>()
            .add_systems(
                OnEnter(AppMode::MainMenu),
                (clear_buttons, spawn_menu).chain(),
            )
            .add_systems(
                Update,
                handle_menu_buttons.run_if(in_state(AppMode::MainMenu)),
            )
            .add_systems(
                Update,
                return_to_menu
                    .run_if(in_state(AppMode::Driving).or_else(in_state(AppMode::RoutePicking))),
            )
            .add_systems(OnExit(AppMode::Driving), clear_esc)
            .add_systems(OnExit(AppMode::RoutePicking), clear_esc);
    }
}

/// 메뉴 항목이 하는 일.
#[derive(Component, Debug, Clone, PartialEq)]
pub enum MenuAction {
    /// 개발 코스로 주행.
    DriveCourse(CourseSelection),
    /// 저장된 경로 파일로 주행.
    DriveRoute(PathBuf),
    /// 경로 선택 화면(맵 생성하기).
    CreateMap,
    /// 종료.
    Quit,
}

/// 메뉴 알림 문구(코스를 읽지 못한 경우 등).
#[derive(Component)]
struct MenuMessage;

/// `Esc` 복귀 확인 상태: 첫 `Esc`를 누른 시각.
#[derive(Resource, Debug, Default)]
pub struct EscConfirm {
    /// 첫 `Esc` 시각(s). 없으면 확인 대기 아님.
    pub first_press_s: Option<f32>,
}

/// 저장된 경로 파일 목록(최근 수정 순). 파일 이름과 경로.
pub fn saved_routes(dir: &Path, limit: usize) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(SystemTime, String, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .map(|path| {
            let time = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            (time, name, path)
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    files
        .into_iter()
        .take(limit)
        .map(|(_, name, path)| (name, path))
        .collect()
}

/// 메뉴 항목의 코스를 만든다. 읽지 못하면 오류 문구.
pub fn course_for(action: &MenuAction) -> Option<Result<CourseDef, String>> {
    match action {
        MenuAction::DriveCourse(selection) => {
            let (course, fallback) = selection.resolve();
            Some(fallback.map_or(Ok(course), Err))
        }
        MenuAction::DriveRoute(path) => Some(map::load_map_course(path)),
        MenuAction::CreateMap | MenuAction::Quit => None,
    }
}

/// 화면을 바꿀 때 눌린 키·버튼을 비운다.
fn clear_buttons(world: &mut World) {
    crate::clear_pressed_buttons(world);
}

/// 메뉴 화면을 만든다: 카메라, 제목, 버튼(개발 코스 둘, 저장 경로, 맵 생성하기, 종료), 알림 줄.
fn spawn_menu(mut commands: Commands) {
    let scoped = DespawnOnExit(AppMode::MainMenu);
    commands.spawn((Camera2d, scoped.clone()));
    let button = |label: String, action: MenuAction, color: Color| {
        (
            Button,
            Node {
                width: Val::Px(520.0),
                padding: UiRect::axes(Val::Px(16.0), Val::Px(8.0)),
                margin: UiRect::vertical(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(color),
            action,
            children![(
                Text::new(label),
                TextFont::from_font_size(18.0),
                TextColor(Color::WHITE),
            )],
        )
    };
    let routes = saved_routes(Path::new(picker::ROUTE_DIR), MAX_ROUTES);
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.12, 0.15)),
            scoped,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("Shuttle Training Simulator"),
                TextFont::from_font_size(30.0),
                TextColor(Color::WHITE),
                Node {
                    margin: UiRect::bottom(Val::Px(18.0)),
                    ..default()
                },
            ));
            let drive = Color::srgb(0.15, 0.35, 0.55);
            parent.spawn(button(
                "Drive: training course".to_owned(),
                MenuAction::DriveCourse(CourseSelection::Builtin(CourseId::Training)),
                drive,
            ));
            parent.spawn(button(
                "Drive: calibration course".to_owned(),
                MenuAction::DriveCourse(CourseSelection::Builtin(CourseId::Calibration)),
                drive,
            ));
            if routes.is_empty() {
                parent.spawn((
                    Text::new("No saved routes yet (cache/routes)"),
                    TextFont::from_font_size(15.0),
                    TextColor(Color::srgb(0.7, 0.7, 0.7)),
                ));
            }
            for (name, path) in routes {
                parent.spawn(button(
                    format!("Drive saved route: {name}"),
                    MenuAction::DriveRoute(path),
                    Color::srgb(0.20, 0.30, 0.40),
                ));
            }
            parent.spawn(button(
                "Create map route (nationwide map)".to_owned(),
                MenuAction::CreateMap,
                Color::srgb(0.15, 0.45, 0.2),
            ));
            parent.spawn(button(
                "Quit".to_owned(),
                MenuAction::Quit,
                Color::srgb(0.45, 0.15, 0.15),
            ));
            parent.spawn((
                Text::new("While driving or picking a route, press Esc twice to come back here."),
                TextFont::from_font_size(14.0),
                TextColor(Color::srgb(0.75, 0.75, 0.75)),
                Node {
                    margin: UiRect::top(Val::Px(14.0)),
                    ..default()
                },
            ));
            parent.spawn((
                Text::new(""),
                TextFont::from_font_size(15.0),
                TextColor(Color::srgb(1.0, 0.6, 0.5)),
                MenuMessage,
            ));
        });
}

/// 눌린 메뉴 버튼을 처리한다: 코스를 정해 주행에 들어가거나, 경로 선택 화면으로 가거나, 종료한다.
/// 코스를 읽지 못하면 알림만 보이고 메뉴에 머문다.
fn handle_menu_buttons(
    buttons: Query<(&Interaction, &MenuAction), Changed<Interaction>>,
    mut course: ResMut<ActiveCourse>,
    mut next: ResMut<NextState<AppMode>>,
    mut picker: ResMut<Picker>,
    mut messages: Query<&mut Text, With<MenuMessage>>,
    mut exit: MessageWriter<AppExit>,
) {
    for (interaction, action) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            MenuAction::CreateMap => {
                picker.prepare_browse();
                next.set(AppMode::RoutePicking);
            }
            MenuAction::Quit => {
                exit.write(AppExit::Success);
            }
            action => match course_for(action) {
                Some(Ok(def)) => {
                    info!("course from menu: {}", def.name);
                    course.0 = def;
                    next.set(AppMode::Driving);
                }
                Some(Err(error)) => {
                    for mut text in &mut messages {
                        text.0 = format!("Cannot open this course: {error}");
                    }
                }
                None => {}
            },
        }
    }
}

/// 주행·경로 선택 화면에서 `Esc`를 [`ESC_CONFIRM_S`]초 안에 두 번 누르면 메인 화면으로 간다.
fn return_to_menu(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut confirm: ResMut<EscConfirm>,
    mut next: ResMut<NextState<AppMode>>,
) {
    let now = time.elapsed_secs();
    if confirm
        .first_press_s
        .is_some_and(|first| now - first > ESC_CONFIRM_S)
    {
        confirm.first_press_s = None;
    }
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    match confirm.first_press_s {
        Some(_) => {
            confirm.first_press_s = None;
            next.set(AppMode::MainMenu);
        }
        None => confirm.first_press_s = Some(now),
    }
}

/// 화면을 나갈 때 `Esc` 확인 상태를 지운다.
fn clear_esc(mut confirm: ResMut<EscConfirm>) {
    confirm.first_press_s = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 저장 경로 목록: toml만, 최근 수정 순, 개수 제한, 폴더가 없으면 빈 목록.
    #[test]
    fn saved_routes_are_listed_newest_first() {
        let dir = std::env::temp_dir().join(format!("sds-menu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(saved_routes(&dir, 8).is_empty());
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.toml", "b.toml", "note.txt"] {
            std::fs::write(dir.join(name), "x").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let names: Vec<String> = saved_routes(&dir, 8).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["b.toml", "a.toml"]);
        assert_eq!(saved_routes(&dir, 1).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 메뉴 항목의 코스: 개발 코스는 열리고, 없는 경로 파일은 오류, 맵 생성·종료는 코스가 없다.
    #[test]
    fn menu_actions_resolve_courses() {
        assert!(matches!(
            course_for(&MenuAction::DriveCourse(CourseSelection::Builtin(
                CourseId::Training
            ))),
            Some(Ok(_))
        ));
        assert!(matches!(
            course_for(&MenuAction::DriveRoute(PathBuf::from("missing-route.toml"))),
            Some(Err(_))
        ));
        assert_eq!(course_for(&MenuAction::CreateMap), None);
    }
}
