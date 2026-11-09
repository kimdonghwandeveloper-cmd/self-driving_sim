//! Phase 5-0 Godot 연결 계층.
//!
//! Godot 에디터·게임이 `sim_godot.dll`을 불러오면 여기서 등록한 클래스를 GDScript에서 쓸 수 있다.
//! 엔진과 무관한 로직은 `sim-core`에 있고, 이 계층은 그 기능을 Godot 클래스로 감싼다
//! (클래스·좌표·실패 규칙은 `docs/design/godot-interface.md` v1).

pub mod course;
pub mod replay;
// gdext 0.5.5의 `#[init(val = ...)]`가 만든 생성 코드가 `base: base` 형태라 clippy가 짚는다.
// 생성 코드는 구조체 밖이라 모듈 단위로 허용한다.
#[allow(clippy::redundant_field_names)]
pub mod vehicle;

use godot::prelude::*;

/// GDExtension 진입점 표식. Godot이 dll을 불러올 때 `gdext_rust_init`을 통해 이 타입으로 클래스를 등록한다.
struct SimGodotExtension;

#[gdextension]
unsafe impl ExtensionLibrary for SimGodotExtension {}

/// Godot에 노출하는 연결 확인용 클래스. 지금은 정적 함수만 있다.
#[derive(GodotClass)]
#[class(init, base = RefCounted)]
struct SimBridge {}

#[godot_api]
impl SimBridge {
    /// 확장 이름과 crate 버전을 돌려준다. Godot 쪽에서 dll이 제대로 불러와졌는지 확인할 때 쓴다.
    #[func]
    fn version() -> GString {
        GString::from(concat!("sim-godot ", env!("CARGO_PKG_VERSION")))
    }

    /// 시속(km/h)을 초속(m/s)으로 바꾼다. Rust 계산 결과가 Godot으로 그대로 넘어오는지 확인하는 용도다.
    #[func]
    fn kmh_to_mps(kmh: f64) -> f64 {
        kmh / 3.6
    }
}
