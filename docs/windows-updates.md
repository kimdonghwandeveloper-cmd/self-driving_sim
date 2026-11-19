# Windows EXE와 업데이트 운영

## 사용자 실행

배포 폴더의 `Simulator.exe`를 실행한다. Godot과 DLL은 폴더에 함께 있어야 하며 EXE 하나만 복사해서 실행하는 단일 파일 배포가 아니다. Windows 10/11의 Windows PowerShell과 .NET Framework를 사용한다. 관리자 권한 없이 쓸 수 있는 쓰기 가능한 폴더에 압축을 푼다.

기본 `update.json`은 비활성이다. 서버가 없거나 업데이트가 실패하면 현재 프로그램을 실행한다. 시작할 때만 확인하며 주행 중 프로그램을 교체하지 않는다. 실행 중에는 설치 폴더의 파일 잠금을 유지한다.

## 게시 준비

실제 서버·서명 키는 이번 구현에서 생성하거나 공개 게시하지 않는다. 배포 담당자는 외부 보안 위치에서 RSA 2048비트 이상의 키를 생성·보관하고 공개 키의 XML 문자열만 최초 번들의 `update.json`에 넣는다. 개인 키는 Git·배포 폴더·웹 서버에 넣지 않는다. 업데이트 서명은 Windows EXE의 Authenticode 서명과 별개이며 현재 EXE는 코드 서명되지 않는다.

```json
{"enabled":true,"url":"https://YOUR-SERVER/releases/stable/","public_key":"<RSAKeyValue>공개 키 XML</RSAKeyValue>"}
```

1. 기존 `scripts/package_release.ps1 -ReleaseVersion 1`로 독립 번들을 만들고 검증한다. 배포 세대는 매번 증가시키며 신규 설치도 이 번호에서 시작하므로 이전 서버 버전으로 내려가지 않는다. 기본값 0은 로컬 검토용이며 게시할 수 없다.
2. `scripts/publish-update.ps1 -Bundle dist/검증번들 -OutputDir dist/새게시폴더 -PrivateKeyFile D:/보안위치/private.xml -Version 1`을 실행한다. 세대 번호는 매번 증가시킨다. 기존 출력 폴더는 덮어쓰지 않는다.
3. 일반 HTTPS 서버라면 `files/`의 모든 파일을 먼저 올리고 `manifest.json`과 `manifest.sig`를 마지막에 함께 전환한다. HTTPS 리다이렉트는 최대 다섯 번 허용한다.
4. 시험 설치에서 업데이트·기존 저장 경로 재진입을 확인한 뒤 배포한다. Git push 자체는 공개 업데이트 게시를 뜻하지 않는다.

## GitHub Releases 채널 (사용자 선택)

2026-11-19 GitHub 공개 API에서 `kimdonghwandeveloper-cmd/self-driving_sim`이 공개 저장소임을 확인했다. 사용자는 토큰 없이 자산을 받는다. 게시자만 환경 변수 `GITHUB_TOKEN`으로 해당 저장소의 Contents 쓰기 권한을 제공한다. 토큰은 EXE/설정/ZIP에 포함하지 않는다.

```powershell
./scripts/publish-github-release.ps1 -Bundle dist/검증번들 -OutputDir dist/github-v1 -PrivateKeyFile D:/보안위치/private.xml -Version 1 -Commit <검증한40자리커밋> -PrepareOnly
```

이 명령은 해시 이름의 `.blob` 파일·서명 manifest·공개 키와 채널이 설정된 최초 설치 ZIP을 로컬에서 준비한다. 아직 GitHub를 바꾸지 않는다. `-PrepareOnly`를 빼면 새 draft release를 생성하고 파일을 올린다. `-Publish`까지 명시해야 전체 업로드 성공 후 공개하며 latest를 변경한다. 실패한 draft는 남기며 기존 release를 덮어쓰지 않는다. 업로드 제한에 맞춰 고유 자산 1,000개 및 개별 2 GiB 미만을 검사한다.

기본 채널은 `https://github.com/kimdonghwandeveloper-cmd/self-driving_sim/releases/latest/download/`이다. `layout="github-assets"`이면 각 파일을 서명 manifest의 SHA-256 이름으로 받는다. GitHub Actions 자동 빌드/태그 게시와 실제 운영 서명 키 생성·공개 release 게시·인터넷 업데이트 왕복은 아직 실행하지 않았다. 전국 지도 원본은 Git에 없으므로 기존 검증 Windows PC의 번들을 게시하는 수동 파이프라인부터 사용한다.

GitHub 제한과 다운로드 형식: https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases 및 https://docs.github.com/en/rest/releases/assets

## 적용 계약

- 원문 manifest의 RSA/SHA-256 서명 → 단조 증가 세대·경로·필수 파일 검사 → 파일별 길이/해시 → 새 Godot/DLL smoke 시험 → 사용자 자료 복사 → 현재 세대 포인터 원자 교체 순서다.
- `versions/stage-*`에서 준비하고 성공 시 `versions/v번호`로 이동한다. 기존 세대는 삭제하지 않는다. 실패 stage와 `update.log`도 진단용으로 남긴다.
- 기존과 해시가 같은 파일은 다운로드하지 않고 복사한다. 전국 지도 변경이 없는 프로그램 업데이트에 전국 팩을 다시 다운로드하지 않는다. 로컬 디스크에는 새 세대 사본을 위한 공간이 필요하다.
- `config/`, `cache/`, `logs/`를 직전 세대에서 새 세대로 보존한다. Godot `user://` 기록은 기존 OS 사용자 위치를 유지한다. 기존 사용자 설정을 새 기본값으로 덮어쓰지 않는다. 설정 스키마가 바뀌는 릴리스는 별도 마이그레이션이 필요하다.
- 블록/OSM 저장 경로가 외부 파일을 참조하면 그 외부 파일도 계속 존재해야 한다. 외부 지도까지 이동·갱신하는 기능은 아니다.
- 서명·네트워크·파일·smoke 검사 실패는 현재 세대를 바꾸지 않는다. 설치 후의 모든 주행 오류를 자동 판정하거나 되돌리는 기능은 아니다. 런처 자체의 원격 교체·키 교체·예약 자동 게시·버전별 자동 삭제는 후속 범위다.

## 검증

`powershell.exe -NoProfile -File scripts/test-updater.ps1`은 경로 탈출/Windows 특수 이름, 서명 변조, 필수 파일/중복/과거 세대 거부, 파일 재사용, 원자 세대 전환, 해시 및 엔진 실패 시 기존 세대 유지, 사용자 자료, 실제 EXE 오프라인/오류 복구 실행, 게시 서명 왕복을 검사한다. 시험용 엔진/키는 임시 폴더에만 만든다. 실제 인터넷 HTTPS 서버의 운영 배포와 별도 PC 수동 검수는 별도다.
