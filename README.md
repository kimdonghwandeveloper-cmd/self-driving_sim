# 자율주행 셔틀 조작 훈련 시뮬레이터

[PLAN.md](PLAN.md)의 Phase 1-1에 해당하는 기본 개발환경입니다.
현재는 창, 카메라, 조명과 기본 도형으로 3D 렌더링을 확인합니다.
게임패드 조작, 차량, 충돌 처리, 지도, 교육 시나리오는 아직 구현하지 않았습니다.
실제 차량의 성능이나 안전성을 검증하는 도구가 아닙니다.

## 준비

- Rustup: `rust-toolchain.toml`이 Rust **1.99.1**, rustfmt, Clippy를 선택합니다.
- Windows: Visual Studio Build Tools의 **Desktop development with C++** 및 Windows SDK가 필요합니다. 애플리케이션 구현 언어는 Rust입니다.
- 설치 구성은 루트 `.vsconfig`에 기록했습니다. Visual Studio Installer에서 가져온 뒤 권장 구성 요소를 포함하여 설치합니다.
- 그래픽 드라이버와 데스크톱 세션이 필요합니다.
- RustRover에서 루트 `Cargo.toml`을 Cargo 프로젝트로 연결합니다.

```powershell
rustup show
cargo fetch --locked
cargo run --locked
```

첫 실행은 엔진 의존성 컴파일 때문에 오래 걸립니다. 도형이 나타나면 창의 닫기 버튼으로 종료합니다.
실행 작업 디렉터리는 저장소 루트로 지정합니다.

### WSL에서 작업하는 경우

이 저장소의 기본 실행 대상은 Windows MSVC입니다. WSL 터미널에서 Windows에
설치된 Rust를 사용할 때는 `rustup.exe`, `cargo.exe`, `rustc.exe`처럼 확장자를
붙입니다. 예를 들어 `cargo.exe run --locked`로 Windows 창을 실행할 수 있습니다.
RustRover의 Windows 터미널에서는 위 PowerShell 명령을 그대로 사용합니다.
WSL Linux용 Rust와 그래픽 환경은 별도로 설치·검증하지 않았습니다.

## 개발 명령

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo run --locked
cargo run --locked --release
```

`physics` 기능은 Bevy/Rapier 빌드 호환성 확인용 선택 의존성입니다.
기본 실행에는 Rapier 플러그인을 등록하지 않습니다.
입력은 이후 Bevy의 게임패드 API를 사용하며 별도의 gilrs 직접 의존성을 추가하지 않았습니다.

창과 렌더러를 자동 실행하고 약 5초 후 종료하려면 다음 명령을 사용합니다.
이 검증은 GPU 초기화와 실행 오류 확인용이며 화면의 시각적 검수를 대체하지 않습니다.

```powershell
cargo run --locked -- --smoke-test
```

## 로그

Bevy의 기본 로그는 터미널에 출력됩니다. PowerShell에서 상세 로그와 파일 저장을 설정할 수 있습니다.

```powershell
$env:RUST_LOG = 'info,self_driving_sim=debug,wgpu=warn'
$env:RUST_BACKTRACE = '1'
New-Item -ItemType Directory -Force logs
cargo run --locked 2>&1 | Tee-Object -FilePath logs/debug.log
```

Release도 `cargo run --locked --release`에 같은 환경변수를 적용합니다.
`logs/`와 `target/`은 버전 관리에서 제외합니다. `Cargo.lock`은 커밋하여 의존성 해석을 고정합니다.

## 구성과 기준

- `src/main.rs`: 진입점과 자동 종료 검증 옵션
- `src/app.rs`: Bevy 앱과 개발환경 확인용 3D 장면
- `assets/{models,textures,maps}/`: 추후 에셋 경로
- [docs/development.md](docs/development.md): 선정 버전, 개발 PC, 검증 결과

설정 파일과 도메인 모듈은 해당 기능을 구현할 때 추가합니다.
엔진 설정은 [Bevy 공식 가이드](https://bevy.org/learn/quick-start/getting-started/setup/)와
[0.19.1 기능 목록](https://docs.rs/crate/bevy/0.19.1/features)을 참고했습니다.
