# 단계별 검증 신호

이 도구는 Claude(구현자)와 Codex(검토자)가 같은 Git 저장소의 서로 다른 워크트리에서 요청과 결과를 교환한다. 파일을 읽는 프로토콜이며 다른 채팅에 메시지를 보내거나 에이전트를 자동 실행하지 않는다. 양쪽 에이전트가 아래 절차를 따라야 한다. `/loop` 자체를 설치하거나 예약 작업을 등록하지 않는다.

- 상태 파일: `git rev-parse --path-format=absolute --git-common-dir` 아래 `verification-signals/<요청 ID>/`.
- 요청은 단계 번호와 전체 커밋 SHA에 고정된다. 수정 커밋이 생기면 새 요청을 만든다.
- `request.toml`: 요청. `checks.toml`: 자동 검사 통과 후 검토 대기. `decision.toml`: 검토자의 최종 진행/수정 신호.
- 자동 검사 성공만으로 진행 신호가 나오지 않는다. 검토자가 코드·PLAN 요구사항·회귀·필요한 수동 확인을 검토하고 근거를 기록해야 한다.
- `continue`는 다음 개발 단계 진행 허가다. main 병합, push, 브랜치 삭제나 PLAN 완료 표시는 기존 사용자 승인 규칙을 따른다.
- 한 저장소에서 한 번에 한 검증만 수행한다. 검증 중에는 해당 test 워크트리를 Claude나 사람이 사용하지 않는다. 실행 중인 Cargo/앱이 있으면 먼저 종료한다.
- 모든 참여자가 파일을 수정할 수 있으므로 암호학적 인증 또는 권한 분리 장치는 아니다. Claude는 decision/checks 파일을 쓰거나 approve 명령을 실행하면 안 된다.

## 준비

기능 브랜치가 있는 워크트리에서 Windows PowerShell로 빌드한다. 독립 Rust 패키지이므로 이 도구를 빌드할 때 시뮬레이터 엔진을 함께 빌드하지 않는다.

```powershell
cargo.exe build --locked --manifest-path tools/verification-gate/Cargo.toml
$gate = (Resolve-Path tools/verification-gate/target/debug/verification-gate.exe).Path
# PATH에 godot가 없으면 사용 중인 Godot 콘솔 실행 파일을 명시한다.
$env:GODOT_BIN = 'C:/사용자/Godot_console.exe'
```

아래 명령은 각 에이전트가 자신의 터미널에서 실행한다. `$gate`는 위 실행 파일의 절대 경로다. 개발 브랜치에 도구가 아직 없어도 이 경로로 호출할 수 있다. WSL 안에서도 Windows용 실행 파일과 Windows 경로를 사용한다.

## Claude에게 전달할 지시

> Phase 3 하위 항목 하나를 구현한 뒤 커밋하고 작업 트리를 깨끗하게 유지한다. 아래 request 명령의 ID를 Codex에게 전달하고 대기한다. 요청한 커밋을 변경하지 않는다. wait/status가 0(continue)일 때만 다음 항목을 개발한다. 2이면 지적사항을 수정하고 새 커밋으로 새 요청을 만든다. 3은 대기 중/시간 만료이므로 진행하지 않는다. 1은 도구 오류이며 승인으로 해석하지 않는다. 검증 결과나 승인 신호를 스스로 작성하지 않는다. 지도 원본/사용자 판단 등이 없으면 임의로 해결됐다고 표시하지 않고 중단 사유를 보고한다.

```powershell
# 개발 워크트리에서 실행. 화면/실행 검사는 기본 포함.
$id = & $gate request 3-2
& $gate wait $id 60
$code = $LASTEXITCODE
# code 3이면 다음 loop 주기에 다시 wait. 0만 다음 단계 허용.
```

문서만 변경해 실행 검사가 불필요한 경우 `request <단계> --no-smoke`를 쓸 수 있다. 검토자는 요청 파일의 smoke 값을 확인한다. 요청 ID는 영문/숫자/하이픈만 허용한다.

## Codex 검증 절차

1. 요청 ID와 request.toml의 커밋/단계, 변경 내역을 확인한다.
2. test 워크트리가 다른 검증에 사용 중이지 않은지 확인한다. clean한 detached HEAD여야 한다. 도구는 변경 내용을 stash/reset하지 않는다.
3. 다음 명령으로 요청 커밋을 체크아웃하고 fmt, clippy, 전체 테스트, `sim-godot` 빌드·Godot import·기본 smoke 검사를 실행한다. stdout/stderr는 요청 폴더의 명령별 로그로 남는다.

```powershell
& $gate verify $id C:/Users/kimdo/RustroverProjects/self-driving_sim-test
```

4. 로그와 실제 변경 코드를 읽고 PLAN의 해당 항목을 대조한다. 필요한 동작/실패 경로/회귀 테스트, 화면 확인을 추가로 수행한다. 미실시 수동 검증이 완료 기준에 필요하면 승인하지 않는다.
5. 검토 근거와 제한을 기록해 결정한다. 검토자는 테스트한 커밋이 요청 SHA와 같은지 확인한다.

```powershell
& $gate approve $id '코드/요구사항/회귀 검토 근거 및 수동 확인 결과를 구체적으로 기재'
# 또는
& $gate reject $id '수정 위치, 재현 절차, 기대 동작을 구체적으로 기재'
```

자동 검사 실패 시 changes-required가 기록된다. 자동 검사 통과 후에는 awaiting-review이며 approve 전까지 대기한다. approve는 checks.toml이 없거나 SHA/ID가 다르면 실패한다. 한 요청의 검사/결정은 덮어쓰지 않는다. 재시도도 새 요청으로 한다.

## 상태와 복구

`status <id>` / `wait <id> <시간초>`의 종료 코드:

| 코드 | 의미 |
| --- | --- |
| 0 | continue: 해당 요청 SHA에서 다음 개발 단계 진행 가능 |
| 1 | 도구/상태 오류: 진행 금지 |
| 2 | changes-required: 수정 후 새 요청 |
| 3 | pending: 계속 대기, wait에서는 제한 시간 만료 포함 |

검사 명령에는 강제 시간 제한이 없다. 응답이 없는 검사라면 검토자가 프로세스를 확인하고 중단한다. 중단 후 불완전한 요청은 승인하지 않고 새 요청을 만든다. 프로세스를 강제 종료하면 공유 폴더에 `lock` 디렉터리가 남을 수 있다. 다른 검증 프로세스가 없음을 확인한 뒤 그 빈 lock 디렉터리만 제거한다. 자동으로 잠금을 강탈하지 않는다.

이 도구는 상시 감시 서비스가 아니다. Claude와 Codex의 별도 loop가 요청/결과를 주기적으로 확인해야 한다. Codex는 요청 ID를 확인하고 실제 검토한 뒤만 approve한다. 구현 시점에는 Claude 세션과의 실시간 왕복 전달은 검증하지 않았다.

Codex의 loop는 & $gate list로 요청 목록을 확인한다. pending 요청만 선택하고, awaiting-review는 로그·코드 검토를 마친 뒤 결정한다. started-or-interrupted는 현재 실행 중인지 먼저 확인하며 자동 재실행/승인하지 않는다.

2026-11-18 통합: 삭제된 Bevy 실행 파일 대신 `sim-godot` 빌드와 Godot 헤드리스 import/smoke를 검사한다. 이 도구의 진행 신호는 사용자 병합 승인을 대신하지 않는다.
