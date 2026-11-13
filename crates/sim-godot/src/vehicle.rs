//! 차량 노드 `SimVehicle`(Godot 인터페이스 계약 v1 2절·5-1·6절).
//!
//! 주행 계산은 `sim_core::session::DriveSession`이 하고, 이 노드는 Godot 물리 틱마다 세션을 한 번 돌린 뒤
//! 자기 `transform`을 갱신하고 틱 결과를 시그널로 보낸다. 장치 읽기는 `devices` 모듈이 한다.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use godot::classes::notify::Node3DNotification;
use godot::classes::{Engine, INode3D, Node3D, ProjectSettings};
use godot::prelude::*;
use sim_core::camera::{OperatorView, ViewDirection};
use sim_core::config::{
    CAMERA_CONFIG_PATH, ConfigError, VEHICLE_CONFIG_PATH, VehicleConfig, parse_operator_view,
    parse_vehicle_config, read_config,
};
use sim_core::scenario::load_scenario;
use sim_core::scenario::run::{Control, Phase, ScenarioRun, ScenarioUpdate};
use sim_core::session::{DriveCommand, DriveSession, QueueFull, TickReport};
use sim_core::vehicle::state::{Gear, VehicleInput};

use crate::course::{
    build_course, build_course_with_patch_dir, course_dict, failure_dict, panic_message,
};
use crate::devices::{Devices, load_bindings};
use crate::paths::resolve_user_path;

/// 기본 설정 폴더(`res://` 기준). 저장소 루트의 `config/`다.
const DEFAULT_CONFIG_DIR: &str = "res://../config";
/// 계약이 기대하는 물리 틱 수(초당).
const EXPECTED_TICKS_PER_SECOND: i32 = 64;

/// 기어의 계약 표시 이름(`"P"` 등)을 반환한다.
pub fn gear_name(gear: Gear) -> &'static str {
    match gear {
        Gear::Park => "P",
        Gear::Reverse => "R",
        Gear::Neutral => "N",
        Gear::Drive => "D",
    }
}

/// 시점 방향의 계약 이름(`"front"` 등)을 반환한다.
fn view_name(view: ViewDirection) -> &'static str {
    match view {
        ViewDirection::Front => "front",
        ViewDirection::Rear => "rear",
        ViewDirection::Left => "left",
        ViewDirection::Right => "right",
    }
}

/// `config_dir` 속성 값을 실제 폴더 경로로 바꾼다. 비어 있으면 `res://../config`.
pub fn resolve_config_dir(config_dir: &str) -> PathBuf {
    let dir = if config_dir.is_empty() {
        DEFAULT_CONFIG_DIR
    } else {
        config_dir
    };
    if dir.starts_with("res://") || dir.starts_with("user://") {
        PathBuf::from(ProjectSettings::singleton().globalize_path(dir).to_string())
    } else {
        PathBuf::from(dir)
    }
}

/// 차량 설정 파일을 읽는다. 없거나 잘못되면 기본값을 쓰고 경고문을 `warnings`에 더한다(계약 6절).
pub fn load_vehicle_config(config_dir: &Path, warnings: &mut Vec<String>) -> VehicleConfig {
    if !config_dir.is_dir() {
        warnings.push(format!(
            "설정 폴더가 없어 모든 설정을 기본값으로 씁니다: {}",
            config_dir.display()
        ));
        return VehicleConfig::default();
    }
    let path = config_dir.join(
        Path::new(VEHICLE_CONFIG_PATH)
            .file_name()
            .expect("config path has a file name"),
    );
    match read_config(&path, parse_vehicle_config) {
        Ok(Some(config)) => config,
        Ok(None) => {
            warnings.push(format!(
                "설정 파일이 없어 기본값을 씁니다: {}",
                path.display()
            ));
            VehicleConfig::default()
        }
        Err(error) => {
            warnings.push(config_warning(&path, &error));
            VehicleConfig::default()
        }
    }
}

/// 카메라 설정 파일을 읽는다. 없거나 잘못되면 기본값을 쓰고 경고문을 `warnings`에 더한다.
/// 설정 폴더가 없으면(경고는 차량 설정에서 이미 남김) 기본값만 쓴다.
pub fn load_operator_view(config_dir: &Path, warnings: &mut Vec<String>) -> OperatorView {
    if !config_dir.is_dir() {
        return OperatorView::default();
    }
    let path = config_dir.join(
        Path::new(CAMERA_CONFIG_PATH)
            .file_name()
            .expect("config path has a file name"),
    );
    match read_config(&path, parse_operator_view) {
        Ok(Some(view)) => view,
        Ok(None) => {
            warnings.push(format!(
                "설정 파일이 없어 기본값을 씁니다: {}",
                path.display()
            ));
            OperatorView::default()
        }
        Err(error) => {
            warnings.push(config_warning(&path, &error));
            OperatorView::default()
        }
    }
}

/// 설정 파일 오류를 화면 표시용 경고문으로 만든다.
fn config_warning(path: &Path, error: &ConfigError) -> String {
    format!("설정 파일 {} {error}; 기본값을 씁니다", path.display())
}

/// 시나리오 파일을 읽고 그 코스를 만들어 실행 상태와 시작 조건을 맞춘 주행 세션을 돌려준다.
/// 실패하면 사람이 읽는 한국어 사유를 돌려준다(패닉도 사유로 바꾼다).
fn start_scenario(
    path: &str,
    config: &VehicleConfig,
) -> Result<(ScenarioRun, DriveSession), String> {
    let def = load_scenario(&resolve_user_path(path)).map_err(|error| error.to_string())?;
    let course = build_course(&def.course)?;
    catch_unwind(AssertUnwindSafe(|| ScenarioRun::new(def, course, config)))
        .map_err(|payload| format!("내부 오류: {}", panic_message(payload.as_ref())))?
        .map_err(|error| error.to_string())
}

/// 차량 한 대. Rust가 입력·상태 머신·운동 모델·충돌을 처리하고 매 물리 틱 끝에 자기 자세를 갱신한다.
#[derive(GodotClass)]
#[class(init, base = Node3D)]
pub struct SimVehicle {
    /// 코스 이름(`"training"`, `"calibration"`). `_ready`에서 한 번만 읽는다.
    #[export]
    #[init(val = GString::from("training"))]
    course: GString,
    /// 지도 코스의 지역 보정 폴더. 비우면 `data/map-patches`; 격리된 시험·개발 자료에서만 바꾼다.
    #[export]
    patch_dir: GString,
    /// 설정 폴더. 비우면 `res://../config`. `_ready`에서 한 번만 읽는다.
    #[export]
    config_dir: GString,
    /// `false`면 장치 입력을 읽지 않고 `set_manual_input`/`send_command`만 쓴다.
    #[export]
    #[init(val = true)]
    read_input: bool,
    /// 시나리오 파일(계약 v1.4, 5-1). 비우면 시나리오 없이 `course`를 주행한다. 있으면 `course` 대신 파일의
    /// 코스를 쓴다. 상대 경로는 저장소 루트 기준. `_ready`에서 한 번만 읽는다.
    #[export]
    scenario: GString,
    /// 주행 세션. 코스 실패·패닉이면 `None`.
    session: Option<DriveSession>,
    /// 시나리오 실행 상태. 시나리오가 없으면 `None`.
    run: Option<ScenarioRun>,
    last_error: String,
    warnings: Vec<String>,
    /// 키보드·게임패드 상태. `_ready`에서 만든다.
    devices: Option<Devices>,
    /// 직전 틱의 `read_input`. 바뀌면 수동 입력과 장치 이력을 비운다.
    last_read_input: Option<bool>,
    operator_view: OperatorView,
    view: ViewDirection,
    debug_camera: bool,
    /// 틱 간격 경고를 이미 남겼는지.
    tick_rate_warned: bool,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for SimVehicle {
    /// 설정을 읽고 코스를 만들어 시작 자세에 놓는다. 실패하면 `course_failed`를 한 번 보낸다.
    fn ready(&mut self) {
        let mut warnings = Vec::new();
        let config_dir = resolve_config_dir(&self.config_dir.to_string());
        let config = load_vehicle_config(&config_dir, &mut warnings);
        let input_dir = config_dir.is_dir().then_some(config_dir.as_path());
        let (keys, pad) = load_bindings(input_dir, &mut warnings);
        self.devices = Some(Devices::new(keys, pad));
        self.operator_view = load_operator_view(&config_dir, &mut warnings);
        for warning in &warnings {
            godot_warn!("{warning}");
        }
        self.warnings = warnings;
        let scenario = self.scenario.to_string();
        if !scenario.trim().is_empty() {
            match start_scenario(scenario.trim(), &config) {
                Ok((run, session)) => {
                    self.run = Some(run);
                    self.session = Some(session);
                    self.sync_transform();
                }
                Err(error) => self.fail(error),
            }
            return;
        }
        let built = if self.patch_dir.to_string().trim().is_empty() {
            build_course(&self.course.to_string())
        } else {
            build_course_with_patch_dir(
                &self.course.to_string(),
                &resolve_user_path(&self.patch_dir.to_string()),
            )
        };
        match built {
            Ok(course) => {
                self.session = Some(DriveSession::new(course, &config));
                self.sync_transform();
            }
            Err(error) => self.fail(error),
        }
    }

    /// 물리 틱 하나를 처리한다(계약 5-1). Godot이 준 `delta`는 쓰지 않고 항상 `DT`로 계산한다.
    fn physics_process(&mut self, _delta: f64) {
        if self.session.is_none() {
            return;
        }
        if !self.tick_rate_warned {
            self.tick_rate_warned = true;
            let rate = Engine::singleton().get_physics_ticks_per_second();
            if rate != EXPECTED_TICKS_PER_SECOND {
                godot_warn!(
                    "physics_ticks_per_second = {rate}; SimVehicle은 항상 DT = 1/64 s로 계산합니다"
                );
            }
        }
        let (view_before, debug_before) = (self.view, self.debug_camera);
        let read_input = self.read_input;
        let session = self.session.as_mut().expect("checked above");
        let devices = self.devices.as_mut().expect("created in ready");
        if self.last_read_input.is_some_and(|last| last != read_input) {
            session.set_manual_input(VehicleInput::default());
            devices.reset_history();
        }
        self.last_read_input = Some(read_input);
        // 시나리오가 진행 중이 아니면(시작 전·일시정지·끝) 틱을 돌리지 않는다. 그동안 누른 키가
        // 계속할 때 새 입력으로 잡히지 않도록 장치 이력을 비운다.
        if self.run.as_ref().is_some_and(|run| !run.is_running()) {
            devices.reset_history();
            return;
        }
        let run = &mut self.run;
        let ticked = catch_unwind(AssertUnwindSafe(|| {
            let read = read_input.then(|| devices.read(session.state().is_stopped()));
            let device = read
                .as_ref()
                .map(|read| read.tick.clone())
                .unwrap_or_default();
            let (report, updates) = match run.as_mut() {
                Some(run) => run
                    .tick(session, &device)
                    .expect("scenario checked running above"),
                None => (session.tick(&device), Vec::new()),
            };
            (report, updates, read)
        }));
        match ticked {
            Ok((report, updates, read)) => {
                if report.course_reset {
                    self.view = ViewDirection::Front;
                }
                if let Some(read) = read {
                    self.view = read.next_view(self.view);
                    self.debug_camera ^= read.debug_toggle;
                }
                self.sync_transform();
                self.emit_report(&report, &updates, view_before, debug_before);
            }
            Err(payload) => {
                self.fail(format!("내부 오류: {}", panic_message(payload.as_ref())));
            }
        }
    }

    /// 창 포커스를 잃으면 다시 얻을 때까지 장치 입력을 0으로 본다(계약 5-1 2단계 신규 규칙).
    fn on_notification(&mut self, what: Node3DNotification) {
        let focused = match what {
            Node3DNotification::APPLICATION_FOCUS_OUT => false,
            Node3DNotification::APPLICATION_FOCUS_IN => true,
            _ => return,
        };
        if let Some(devices) = self.devices.as_mut() {
            devices.set_focused(focused);
        }
    }

    /// 트리에서 빠지면 처리를 멈추고 대기열을 버린다(계약 2절 화면 이탈).
    fn exit_tree(&mut self) {
        if let Some(session) = self.session.as_mut() {
            session.clear_queue();
        }
        self.base_mut().set_physics_process(false);
    }
}

#[godot_api]
impl SimVehicle {
    /// 명령 결과(성공). 명령마다 처리 순서대로 한 번.
    #[signal]
    fn command_applied(command: GString, source: GString);
    /// 명령 결과(거부)와 사유.
    #[signal]
    fn command_rejected(command: GString, source: GString, reason: GString);
    /// 틱 시작과 끝 기어가 다를 때 끝 값.
    #[signal]
    fn gear_changed(gear: GString);
    /// 틱 시작과 끝 시동이 다를 때 끝 값.
    #[signal]
    fn engine_changed(on: bool);
    /// 틱 시작과 끝 비상 정지가 다를 때 끝 값.
    #[signal]
    fn emergency_changed(active: bool);
    /// 이 틱에 시작 위치 복귀가 성공함.
    #[signal]
    fn course_reset();
    /// 접촉이 시작된 틱에 한 번.
    #[signal]
    fn collided();
    /// 틱 시작과 끝 시점이 다를 때 끝 값.
    #[signal]
    fn view_changed(view: GString);
    /// 틱 시작과 끝 디버그 카메라가 다를 때 끝 값.
    #[signal]
    fn debug_camera_toggled(on: bool);
    /// 모든 틱의 마지막 시그널. `tick == tick_count()`.
    #[signal]
    fn tick_finished(tick: i64);
    /// 코스 실패·패닉. 한 번만 보낸다.
    #[signal]
    fn course_failed(error: GString);
    /// 시나리오 단계를 끝냄(번호는 0부터, `id`는 파일의 단계 이름). 틱 안에서 `tick_finished` 전에 보낸다.
    #[signal]
    fn scenario_step_done(index: i64, id: GString);
    /// 시나리오 메시지 이벤트가 일어남.
    #[signal]
    fn scenario_message(text: GString);
    /// 시나리오 실행 단계가 바뀜(`"briefing"`, `"running"`, `"paused"`, `"finished"`). 조작·끝 판정 때 보낸다.
    #[signal]
    fn scenario_phase_changed(phase: GString);
    /// 시나리오가 끝남(`"success"`, `"time_limit"`, `"collisions"`, `"aborted"`). `scenario_phase_changed` 다음에 보낸다.
    #[signal]
    fn scenario_finished(outcome: GString);

    /// 시나리오 조작(`"start"`, `"pause"`, `"resume"`, `"restart"`, `"end"`)을 바로 적용한다.
    /// 시나리오가 없거나 지금 단계에서 받을 수 없으면 `false`. 재시작은 차량을 시작 조건으로 되돌리고 시점을 전방으로 한다.
    #[func]
    fn scenario_control(&mut self, name: GString) -> bool {
        let name = name.to_string();
        let Some(control) = Control::parse(&name) else {
            godot_error!("SimVehicle.scenario_control: 알 수 없는 조작 \"{name}\"");
            return false;
        };
        let (Some(run), Some(session)) = (self.run.as_mut(), self.session.as_mut()) else {
            return false;
        };
        if run.control(control, session).is_err() {
            return false;
        }
        let phase = run.phase();
        if control == Control::Restart {
            let view_before = self.view;
            self.view = ViewDirection::Front;
            self.sync_transform();
            if view_before != self.view {
                self.base_mut().emit_signal(
                    "view_changed",
                    &[GString::from(view_name(ViewDirection::Front)).to_variant()],
                );
            }
        }
        self.emit_phase(phase);
        true
    }

    /// 시나리오 HUD 값(계약 v1.4). 시나리오가 없으면 `{active: false}`만 돌려준다.
    /// `active`, `title`, `briefing`, `phase`(계약 이름), `phase_text`(한국어), `outcome`(끝났을 때만, 아니면 빈 문자열),
    /// `step_index`(지금 단계 번호, 다 끝냈으면 단계 수), `step_count`, `guide`(지금 단계 안내, 다 끝냈으면 빈 문자열),
    /// `elapsed_s`, `time_limit_s`(없으면 -1), `collisions`, `max_collisions`(없으면 -1), `message`(없으면 빈 문자열),
    /// (v1.8) `max_impact_delta_v_mps`(시작 뒤 가장 큰 충돌 속도 변화 m/s, 없으면 0).
    #[func]
    fn scenario_status(&self) -> VarDictionary {
        let Some(run) = &self.run else {
            return vdict! { "active" => false };
        };
        let def = run.def();
        let phase = run.phase();
        let outcome = match phase {
            Phase::Finished(outcome) => outcome.name(),
            _ => "",
        };
        vdict! {
            "active" => true,
            "title" => def.title.as_str(),
            "briefing" => def.briefing.as_str(),
            "phase" => phase.name(),
            "phase_text" => crate::korean::scenario_phase(phase),
            "outcome" => outcome,
            "step_index" => run.steps_done() as i64,
            "step_count" => def.steps.len() as i64,
            "guide" => run.current_step().map_or("", |step| step.guide.as_str()),
            "elapsed_s" => f64::from(run.elapsed_s()),
            "time_limit_s" => def.end.time_limit_s.map_or(-1.0, f64::from),
            "collisions" => i64::from(run.collisions()),
            "max_collisions" => def.end.max_collisions.map_or(-1, i64::from),
            "max_impact_delta_v_mps" => f64::from(run.max_impact_delta_v_mps()),
            "message" => run.message().unwrap_or(""),
        }
    }

    /// 코스를 만들고 시작 자세에 놓았으면 `true`.
    #[func]
    fn is_ready(&self) -> bool {
        self.session.is_some()
    }

    /// 실패 사유. 정상이면 빈 문자열.
    #[func]
    fn last_error(&self) -> GString {
        GString::from(self.last_error.as_str())
    }

    /// 설정 파일을 읽다가 기본값으로 바꾼 항목의 경고문.
    #[func]
    fn config_warnings(&self) -> PackedStringArray {
        self.warnings
            .iter()
            .map(|warning| GString::from(warning.as_str()))
            .collect()
    }

    /// 충돌 판정에 쓰는 코스 데이터(계약 3절, 부를 때마다 새 복사본). 실패 상태면 `{ok: false, error}`.
    #[func]
    fn course_data(&self) -> VarDictionary {
        match &self.session {
            Some(session) => course_dict(session.course()),
            None => failure_dict(&self.last_error),
        }
    }

    /// 처리를 마친 물리 틱 수.
    #[func]
    fn tick_count(&self) -> i64 {
        self.session
            .as_ref()
            .map_or(0, |session| session.tick_count() as i64)
    }

    /// 부호 있는 속도(m/s, 후진 음수).
    #[func]
    fn speed_mps(&self) -> f64 {
        self.session
            .as_ref()
            .map_or(0.0, |session| f64::from(session.state().speed_mps()))
    }

    /// 표시용 절댓값 속도(km/h).
    #[func]
    fn speed_kmh(&self) -> f64 {
        self.speed_mps().abs() * 3.6
    }

    /// 기어(`"P"`, `"R"`, `"N"`, `"D"`).
    #[func]
    fn gear(&self) -> GString {
        let gear = self
            .session
            .as_ref()
            .map_or(Gear::Park, |session| session.state().gear());
        GString::from(gear_name(gear))
    }

    /// 시동 상태(비상 정지 중도 켜짐).
    #[func]
    fn engine_on(&self) -> bool {
        self.session.as_ref().is_some_and(DriveSession::engine_on)
    }

    /// 비상 정지 중인지.
    #[func]
    fn emergency_active(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(DriveSession::emergency_active)
    }

    /// 현재 바퀴 조향각(deg, 오른쪽 양수).
    #[func]
    fn steer_angle_deg(&self) -> f64 {
        self.session.as_ref().map_or(0.0, |session| {
            f64::from(session.motion().steer_angle_rad).to_degrees()
        })
    }

    /// 보정 측정 줄(한국어, 계약 v1.3): 회전반경·예상 정지거리·진행 중 시험·마지막 제동·가속 시험.
    /// 준비 전에는 빈 배열이다. 화면은 보정 코스에서만 표시한다.
    #[func]
    fn calibration_lines(&self) -> PackedStringArray {
        let Some(session) = &self.session else {
            return PackedStringArray::new();
        };
        crate::korean::calibration_lines(&session.calibration_readout())
            .iter()
            .map(GString::from)
            .collect()
    }

    /// 차체 움직임(계약 v1.9, 2-8): `pitch_rad`(앞이 내려가면 양수), `roll_rad`(오른쪽이 내려가면 양수),
    /// `heave_m`(위가 양수). 보이는 움직임만이며 값은 시뮬레이터 임시값으로 계산한다. 준비 전에는 모두 0.
    #[func]
    fn body_pose(&self) -> VarDictionary {
        let body = self
            .session
            .as_ref()
            .map(|session| *session.body())
            .unwrap_or_default();
        vdict! {
            "pitch_rad" => f64::from(body.pitch_rad),
            "roll_rad" => f64::from(body.roll_rad),
            "heave_m" => f64::from(body.heave_m),
        }
    }

    /// 마지막 충돌(계약 v1.8, 2-7). 없거나 시작 위치 복귀 뒤면 빈 사전.
    /// `tick`은 충돌한 틱 번호, 크기(`normal_speed_mps`·`impulse_ns`·`delta_v_mps`)는 시뮬레이터 임시값으로 계산한 값,
    /// `point`는 접촉점(높이 0)이다.
    #[func]
    fn last_impact(&self) -> VarDictionary {
        let Some((tick, hit)) = self.session.as_ref().and_then(DriveSession::last_impact) else {
            return VarDictionary::new();
        };
        vdict! {
            "tick" => tick as i64,
            "speed_mps" => f64::from(hit.speed_mps),
            "normal_speed_mps" => f64::from(hit.normal_speed_mps),
            "impulse_ns" => f64::from(hit.impulse_ns),
            "delta_v_mps" => f64::from(hit.delta_v_mps),
            "point" => Vector3::new(hit.contact_x_m, 0.0, hit.contact_z_m),
        }
    }

    /// 장애물·연석과 접촉한 상태인지.
    #[func]
    fn in_contact(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.contact().touching)
    }

    /// 입석 시점 방향(`"front"`, `"rear"`, `"left"`, `"right"`).
    #[func]
    fn view(&self) -> GString {
        GString::from(view_name(self.view))
    }

    /// 외부 디버그 카메라 켜짐.
    #[func]
    fn debug_camera(&self) -> bool {
        self.debug_camera
    }

    /// 주행에 쓰는 입력 장치(`"keyboard"`, `"gamepad"`, `"manual"`, `"none"`).
    /// `read_input = false`면 `"manual"`, 장치 상태가 아직 없으면 `"none"`.
    #[func]
    fn input_source(&self) -> GString {
        let name = match (&self.devices, self.read_input) {
            (_, false) => "manual",
            (Some(devices), true) => devices.source_name(),
            (None, true) => "none",
        };
        GString::from(name)
    }

    /// `config/camera.toml`의 입석 시점 값(`eye_m`, `pitch_deg`, `vertical_fov_deg`, `near_clip_m`, (v1.9) `body_follow`).
    #[func]
    fn operator_view(&self) -> VarDictionary {
        let view = &self.operator_view;
        let [x, y, z] = view.eye_m;
        vdict! {
            "eye_m" => Vector3::new(x, y, z),
            "pitch_deg" => view.pitch_deg,
            "vertical_fov_deg" => view.vertical_fov_deg,
            "near_clip_m" => view.near_clip_m,
            "body_follow" => view.body_follow,
        }
    }

    /// 화면에 계속 띄울 안내(임시값·출처 표기).
    #[func]
    fn course_notices(&self) -> PackedStringArray {
        self.session
            .as_ref()
            .map(|session| {
                session
                    .course()
                    .notices
                    .iter()
                    .map(|notice| GString::from(crate::korean::course_notice(notice).as_str()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 명령을 다음 틱 대기열에 넣는다. 이름이 틀리거나 실패 상태거나 대기열이 가득 차면 `false`.
    #[func]
    fn send_command(&mut self, name: GString) -> bool {
        let name = name.to_string();
        let Some(command) = DriveCommand::parse(&name) else {
            godot_error!("SimVehicle.send_command: 알 수 없는 명령 \"{name}\"");
            return false;
        };
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        match session.enqueue(command) {
            Ok(()) => true,
            Err(QueueFull) => {
                godot_warn!("SimVehicle.send_command: 대기열이 가득 차 \"{name}\"을 버립니다");
                false
            }
        }
    }

    /// `read_input = false`일 때 쓰는 연속 입력. 다시 부를 때까지 유지되고 다음 틱부터 반영된다.
    #[func]
    fn set_manual_input(&mut self, throttle: f64, brake: f64, steering: f64) {
        if let Some(session) = self.session.as_mut() {
            session.set_manual_input(VehicleInput {
                throttle: throttle as f32,
                brake: brake as f32,
                steering: steering as f32,
            });
        }
    }
}

impl SimVehicle {
    /// 세션의 자세를 노드 `position`·`rotation`에 옮긴다.
    fn sync_transform(&mut self) {
        let Some(motion) = self.session.as_ref().map(|session| *session.motion()) else {
            return;
        };
        let mut base = self.base_mut();
        base.set_position(Vector3::new(motion.x_m, 0.0, motion.z_m));
        base.set_rotation(Vector3::new(0.0, motion.heading_rad, 0.0));
    }

    /// 실패 상태로 바꾸고 `course_failed`를 한 번 보낸다. 위치는 원점으로 둔다.
    fn fail(&mut self, error: String) {
        godot_error!("SimVehicle: {error}");
        self.session = None;
        self.last_error = error.clone();
        let mut base = self.base_mut();
        base.set_position(Vector3::ZERO);
        base.set_physics_process(false);
        base.emit_signal(
            "course_failed",
            &[GString::from(error.as_str()).to_variant()],
        );
    }

    /// 틱 결과를 계약 2절 시그널 표 순서로 보낸다. 시점·디버그 카메라는 틱 시작 값과 비교한다.
    fn emit_report(
        &mut self,
        report: &TickReport,
        updates: &[ScenarioUpdate],
        view_before: ViewDirection,
        debug_before: bool,
    ) {
        let (view, debug_camera) = (self.view, self.debug_camera);
        let step_ids: Vec<String> = self.run.as_ref().map_or_else(Vec::new, |run| {
            run.def().steps.iter().map(|step| step.id.clone()).collect()
        });
        let mut base = self.base_mut();
        for result in &report.results {
            let command = GString::from(result.command.as_str()).to_variant();
            let source = GString::from(result.source.name()).to_variant();
            if result.accepted {
                base.emit_signal("command_applied", &[command, source]);
            } else {
                let reason = GString::from(crate::korean::reject_reason(&result.reason).as_str())
                    .to_variant();
                base.emit_signal("command_rejected", &[command, source, reason]);
            }
        }
        if let Some(gear) = report.gear_changed {
            base.emit_signal(
                "gear_changed",
                &[GString::from(gear_name(gear)).to_variant()],
            );
        }
        if let Some(on) = report.engine_changed {
            base.emit_signal("engine_changed", &[on.to_variant()]);
        }
        if let Some(active) = report.emergency_changed {
            base.emit_signal("emergency_changed", &[active.to_variant()]);
        }
        if report.course_reset {
            base.emit_signal("course_reset", &[]);
        }
        if report.collided.is_some() {
            base.emit_signal("collided", &[]);
        }
        if view != view_before {
            base.emit_signal(
                "view_changed",
                &[GString::from(view_name(view)).to_variant()],
            );
        }
        if debug_camera != debug_before {
            base.emit_signal("debug_camera_toggled", &[debug_camera.to_variant()]);
        }
        drop(base);
        for update in updates {
            match update {
                ScenarioUpdate::StepDone(index) => {
                    let id = step_ids.get(*index).map_or("", String::as_str);
                    self.base_mut().emit_signal(
                        "scenario_step_done",
                        &[(*index as i64).to_variant(), GString::from(id).to_variant()],
                    );
                }
                ScenarioUpdate::Message(text) => {
                    self.base_mut().emit_signal(
                        "scenario_message",
                        &[GString::from(text.as_str()).to_variant()],
                    );
                }
                ScenarioUpdate::Finished(outcome) => self.emit_phase(Phase::Finished(*outcome)),
            }
        }
        self.base_mut()
            .emit_signal("tick_finished", &[(report.tick as i64).to_variant()]);
    }

    /// `scenario_phase_changed`를 보내고, 끝났으면 이어서 `scenario_finished`를 보낸다.
    fn emit_phase(&mut self, phase: Phase) {
        let mut base = self.base_mut();
        base.emit_signal(
            "scenario_phase_changed",
            &[GString::from(phase.name()).to_variant()],
        );
        if let Phase::Finished(outcome) = phase {
            base.emit_signal(
                "scenario_finished",
                &[GString::from(outcome.name()).to_variant()],
            );
        }
    }
}
