# popmgr

Pop!_OS / COSMIC 데스크톱 관리 도구 — Rust + [Iced](https://github.com/iced-rs/iced) GUI

흩어져 있던 Pop!_OS 유틸리티들을 하나의 앱으로 통합했습니다.

| 통합 레포 | 기능 |
|---|---|
| [ime-manager](https://github.com/eondcom/ime-manager) | 한글 IME 관리 (Python → Rust 재작성) |
| [kensington-trackball-linux](https://github.com/eondcom/kensington-trackball-linux) | ktrackball 데몬 + USB 관리 |
| [cosmic-files-copy-path](https://github.com/eondcom/cosmic-files-copy-path) | 탐색기 우클릭 '경로 복사' 항상 표시 |
| [cosmic-three-finger-gesture](https://github.com/eondcom/cosmic-three-finger-gesture) | 3손가락 위 스와이프 → 워크스페이스 오버뷰 |
| [kakaotalk-wine](https://github.com/eondcom/kakaotalk-wine) | KakaoTalk Wine 설치/실행 |
| [popremover](https://github.com/eondcom/popremover) | APT·Flatpak 패키지 제거 (Python → Rust 재작성) |

---

## 기능

### IME 탭
- ibus / fcitx5 / kime 설치 상태 확인 및 전환
- `/etc/environment` 자동 업데이트
- `dbus-update-activation-environment` + 데몬 재시작으로 **재로그인 없이 즉시 적용**
- popmgr 시작 시 실행 중인 IME 데몬 자동 재연결 (Wayland 연결 끊김 방지)
- COSMIC 환경 권장 IME: **kime**
- **셸 init 충돌 진단**: `~/.profile`, `~/.bashrc`, `~/.zshrc`, `~/.zprofile`, `~/.bash_profile` 에서 활성 IME와 모순되는 `GTK_IM_MODULE`/`QT_IM_MODULE`/`XMODIFIERS` 등 export 라인을 감지. "정리" 버튼으로 자동 백업 후 주석 처리.
- **snap 누출 감지**: 현재 환경의 `GTK_IM_MODULE_FILE` 이 snap 캐시(`~/snap/.../immodules.cache`)를 가리키면 경고. snap 앱이 띄운 셸에서 IDE 를 실행하면 시스템 GTK IM 모듈을 못 찾아 한글 입력이 깨지는 사고를 미리 차단.
- **JetBrains IDE vmoptions 자동 패치**: `~/.config/JetBrains/<IDE>/*.vmoptions` 파일들을 스캔해 XIM 안정화 옵션(`-Dawt.toolkit.name=XToolkit`, `-Drecreate.x11.input.method=true`) 누락 여부를 표시. "패치" 버튼으로 백업 후 자동 추가 — IntelliJ Ultimate 의 `XInputMethod.setXICFocusNative` 133초 freeze 같은 사고를 예방.

### USB 탭
- USB 장치 전체 목록 (Kensington 트랙볼·Realforce 키보드 강조)
- USB 열거 실패 포트 감지 및 경고
- ktrackball 데몬 상태 표시 / 재시작
- 개별 장치 재인식 / 전체 USB 재인식
- xHCI 컨트롤러 리셋 (확인 다이얼로그 포함)

### COSMIC 트윅 탭
- **cosmic-files copy-path** — 탐색기 우클릭에 '경로 복사' 항상 표시
- **cosmic-comp 3-finger** — 터치패드 3손가락 위 스와이프 → 워크스페이스 오버뷰

패치 적용 방식:
1. `dpkg`로 현재 설치된 버전의 커밋 해시 확인
2. GitHub에서 해당 커밋 타르볼 다운로드
3. `patch -p1 --fuzz 5` 적용
4. `cargo build --release`
5. `pkexec`로 `/usr/bin`에 설치 (원본 `.bak` 백업)

> 시스템 업데이트 후 패치가 덮어쓰이면 "패치 적용"을 다시 누르세요.
> cosmic-comp 패치 적용 후에는 **로그아웃 → 재로그인** 필요.

### 앱 관리 탭
- KakaoTalk Wine 설치 / 실행
- APT·Flatpak 패키지 검색 및 일괄 제거

---

## 설치

### 원클릭 설치 (popmgr + COSMIC 패치 자동 적용)

```bash
git clone https://github.com/eondcom/popmgr
cd popmgr
bash install.sh
```

`install.sh`가 다음을 순서대로 실행합니다:
1. popmgr 빌드 → `~/.local/bin/popmgr` 설치
2. `~/.local/share/applications/popmgr.desktop` 등록
3. cosmic-files copy-path 패치 적용
4. cosmic-comp 3-finger 패치 적용

### 수동 빌드

```bash
# 빌드 의존성
sudo apt install libfontconfig1-dev libxkbcommon-dev curl patch

# 빌드
cargo build --release

# 실행
./target/release/popmgr

# 앱 런처 등록
cp popmgr.desktop ~/.local/share/applications/
```

---

## 복구 (패치 제거)

```bash
# cosmic-files 복구
sudo cp /usr/bin/cosmic-files.bak /usr/bin/cosmic-files

# cosmic-comp 복구
sudo cp /usr/bin/cosmic-comp.bak /usr/bin/cosmic-comp
```

또는 popmgr COSMIC 탭에서 "패치 제거" 버튼 클릭 (`apt-get install --reinstall`).

---

## 참고

- 한글 IME 설정 가이드: [cosmic-os-korean](https://github.com/Hostingglobal-Tech/cosmic-os-korean)
- 폰트: NanumSquare (UI) + NanumGothic (한글 폴백)

## 변경 이력

### 2026-06-06 — IME 진단 확장
- `~/.profile` 등 사용자 셸 init 파일의 IME export 충돌 감지/정리 추가.
  - 이유: `/etc/environment` 만 동기화하면 사용자가 `.profile` 에 손수 박은 옛 IME 변수(예: kime → ibus 전환 시 흔적)가 시스템 설정을 덮어쓰며 IntelliJ AWT 가 존재하지 않는 ibus XIM 서버에 133초 freeze 하는 사고가 발생.
- 현재 환경의 `GTK_IM_MODULE_FILE` snap 누출 감지.
  - 이유: snap 으로 띄운 터미널 안에서 다른 앱을 실행하면 snap 컨테이너의 immodules.cache 가 부모로 누출돼 시스템 GTK IM 모듈을 못 찾는 사례 발견 (waveterm snap).
- JetBrains IDE vmoptions 자동 패치 (`~/.config/JetBrains/<IDE>/*.vmoptions`).
  - 추가 옵션: `-Dawt.toolkit.name=XToolkit`, `-Drecreate.x11.input.method=true`.
  - 이유: native Wayland IM 경로의 freeze 회피 + IME 데몬 재시작 후 입력 컨텍스트 재구성.

## 라이선스

MIT
