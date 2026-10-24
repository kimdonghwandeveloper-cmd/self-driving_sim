### 에이전트 규칙
- 코드 작성시에는 함수 마다 어떤 기능을 하는 지 주석을 달  것.
- 코드 검토 전 어느 파일에 어느 라인을 수정했는지 작업 수행 완료 후 답변 시 작성할 것.
- 기능 추가/ 테스트  시 브랜치 생성 하여 main 레포지토리에 영향이 가지 않게 할 것.

- 멋대로 완료되었다고 처리하지 말것.

### 워크트리 작업 흐름

| 워크트리 경로 | 브랜치 | 용도 |
| --- | --- | --- |
| `self-driving_sim/` (메인 리포지토리) | `main` | 테스트를 통과한 결과만 병합. 직접 개발 금지 |
| `../self-driving_sim-develop/` | `develop` → `feature/*`, `docs/*` 등 | 새 기능 개발 |
| `../self-driving_sim-test/` | 분리된 HEAD(detached) | 개발 브랜치 검증 |

1. **개발:** develop 워크트리에서 최신 `develop` 기준으로 작업 브랜치를 만든다.
   - `git switch develop && git merge --ff-only main`
   - `git switch -c feature/<기능명>` (문서는 `docs/<이름>`)
   - 작업 단위로 커밋한다. 커밋은 작업 브랜치에서만 한다.
2. **테스트:** test 워크트리에서 작업 브랜치의 커밋을 분리된 HEAD로 체크아웃해 검증한다.
   (같은 브랜치는 두 워크트리에서 동시에 체크아웃할 수 없으므로 `--detach`를 사용한다.)
   - `git switch --detach feature/<기능명>`
   - `cargo.exe fmt --all -- --check`
   - `cargo.exe clippy --locked --all-targets --all-features -- -D warnings`
   - `cargo.exe test --locked --all-features`
   - 화면/실행 관련 변경이면 `cargo.exe run --locked -- --smoke-test`
   - 테스트 워크트리에서는 코드를 수정하지 않는다. 실패하면 develop 워크트리에서 고친 뒤 다시 테스트한다.
3. **병합:** 테스트 결과를 보고하고 **사용자 승인을 받은 뒤에만** 메인 리포지토리에서 병합한다.
   - `git merge --no-ff feature/<기능명>`
   - `origin`으로의 push는 사용자가 요청할 때만 한다.
   - 병합 후 develop 워크트리에서 `git switch develop && git merge --ff-only main`으로 동기화하고,
     병합된 작업 브랜치는 사용자 확인 후 삭제한다.
4. 각 워크트리는 별도의 `target/` 빌드 폴더를 사용하므로 첫 빌드가 오래 걸릴 수 있다.