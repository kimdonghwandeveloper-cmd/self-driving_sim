# 개발환경 기록

기록일: 2026-10-22 (Asia/Seoul)

## 고정 버전

| 항목 | 버전 / 기준 |
| --- | --- |
| Rust / Cargo | 1.99.1 |
| Rust edition | 2024 |
| 호스트 | x86_64-pc-windows-msvc |
| Bevy | =0.19.1, 기본 기능 비활성화 후 `3d` 사용 |
| bevy_rapier3d | =0.36.0, 선택 기능 `physics`, `dim3`만 사용 |
| 잠금 | rust-toolchain.toml 및 Cargo.lock |

Rapier 0.36.0의 공식 배포 Cargo.toml은 Bevy `0.19.0` 범위를 요구하므로 0.19.1과 호환되는 조합입니다.
Bevy 0.20으로 올리는 대신 이 조합을 고정합니다.
출처: [Bevy 0.19.1](https://docs.rs/crate/bevy/0.19.1),
[Rapier 0.36.0 배포 소스](https://docs.rs/crate/bevy_rapier3d/0.36.0/source/Cargo.toml).

개발 프로필은 앱 최적화 수준 1, 외부 의존성 3을 사용합니다.
별도의 nightly 기능, 동적 링크나 사용자 지정 링커는 요구하지 않습니다.

## 확인한 개발 PC

| 항목 | 값 |
| --- | --- |
| OS | Microsoft Windows 11 Pro, 10.0.26300 |
| GPU 1 | AMD Radeon(TM) Graphics, 드라이버 31.0.21921.13001 |
| GPU 2 | NVIDIA GeForce RTX 3050 Ti Laptop GPU, 드라이버 32.0.15.8157 |
| 빌드 도구 | Visual Studio 18 BuildTools |
| Rust 구성 요소 | rustfmt, clippy 설치 확인 |

게임패드 모델은 아직 확인하지 않았습니다. 다른 OS/GPU는 검증하지 않았습니다.

## 검증

검증일: 2026-10-22. WSL에서 Windows Rust 실행 파일(`cargo.exe`)을 호출하여
`x86_64-pc-windows-msvc` 대상으로 확인했습니다. WSL 자체에는 Rust가 설치되어
있지 않으며 Linux 대상 빌드 결과를 의미하지 않습니다.

| 명령 | 결과 |
| --- | --- |
| `rustup.exe show` | Rust 1.99.1 및 Windows MSVC 대상 설치 확인 |
| `cargo.exe fmt --all -- --check` | 통과 |
| `cargo.exe clippy --locked --all-targets --all-features -- -D warnings` | 통과, Bevy/Rapier 포함, 경고 없음 |
| `cargo.exe test --locked --all-features` | 통과, 테스트 0개(도메인 로직 구현 전), 테스트 실행 파일 빌드 및 정상 종료 확인 |
| `cargo.exe run --locked -- --smoke-test` | 통과, 창 생성·장면 초기화·약 5초 후 자동 종료, 종료 코드 0 |

실행 로그에서 NVIDIA GeForce RTX 3050 Ti Laptop GPU, NVIDIA 드라이버 581.57,
Vulkan 백엔드 사용을 확인했습니다. OS는 Windows 11 Pro(26300), CPU는
AMD Ryzen 5 5600H입니다. 화면의 시각적 검수 및 Release 빌드 실행은 이번
검증에 포함하지 않았으며, Debug/Release 실행 절차와 로그 설정은 README에 기록했습니다.

Phase 1-1의 개발환경 기초 설정 및 최종 실행 검증을 완료했습니다.

첫 빌드에서는 엔진 의존성 컴파일에 시간이 걸립니다. Clippy 검사와 실행 파일
빌드는 생성하는 산출물이 달라 각각 최초 컴파일이 필요할 수 있습니다.
