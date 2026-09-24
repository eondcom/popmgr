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
- **LibreOffice 한글 입력 호환 모드**: COSMIC Wayland에서 LibreOffice가 fcitx 입력 컨텍스트를 만들지 못하는 상태를 실행 프로세스에서 진단. 버튼 한 번으로 시스템 파일은 건드리지 않고 사용자 범위 desktop 런처에 X11/XIM 입력 경로를 적용하거나 해제.
- **JetBrains IDE vmoptions 자동 패치**: `~/.config/JetBrains/<IDE>/*.vmoptions` 파일들을 스캔해 XIM 안정화 옵션(`-Dawt.toolkit.name=XToolkit`, `-Drecreate.x11.input.method=true`) 누락 여부를 표시. "패치" 버튼으로 백업 후 자동 추가 — IntelliJ Ultimate 의 `XInputMethod.setXICFocusNative` 133초 freeze 같은 사고를 예방.
- **fcitx5 한/영 상태 유지 설정**: `~/.config/fcitx5/config` 의 `ShareInputState`/`ActiveByDefault`/`AltTriggerKeys` 를 진단. fcitx5 기본값은 창마다 한/영 상태를 따로 기억하고 새 창을 영문으로 시작해 "다른 창에 갔다 오면 영문만 입력되는" 원인이 된다. "고치기" 버튼(또는 `popmgr --fix-fcitx5-behavior`)으로 `ShareInputState=All`, `ActiveByDefault=True`, 왼쪽 Shift 단독 탭 영문 전환 해제를 적용하고 `fcitx5-remote -r` 로 **재시작 없이** 반영.
- **fcitx5 툴킷 프론트엔드 누락 진단**: `QT_IM_MODULE=fcitx` 인데 `fcitx5-frontend-qt5/qt6` 같은 IM 모듈 패키지가 없으면 경고 + 설치 버튼.
- **IME 재시작 버튼**: `/etc/environment` 재작성(pkexec) 없이 활성 IME 데몬만 재시작.

### USB 탭
- USB 장치 전체 목록 (Kensington 트랙볼·Realforce 키보드 강조)
- USB 열거 실패 포트 감지 및 경고
- ktrackball 데몬 상태 표시 / 재시작
- 개별 장치 재인식 / 전체 USB 재인식
- xHCI 컨트롤러 리셋 (확인 다이얼로그 포함)
- 블루투스 트랙볼 페어링/연결/해제 (스캔→pair→trust→connect 를 단일 `bluetoothctl` 세션으로 수행)
  - 페어링 모드 진입은 기기 물리 조작(Expert 계열은 상단 버튼 4개 3초). 동글↔BT **모드 전환은 펌웨어 전용이라 소프트웨어로 불가**
  - 배경과 함정은 [`docs/bluetooth-usb-trackball-notes.md`](docs/bluetooth-usb-trackball-notes.md), 사용자용 정리는 [`docs/tip-kensington-trackball-bluetooth-linux.md`](docs/tip-kensington-trackball-bluetooth-linux.md)

### 디스플레이 탭
- 내장·외부 모니터 **밝기 조절** (외부는 명암까지)
- **내장 디스플레이**: logind `SetBrightness` 로 root 없이 백라이트 제어 (`/sys/class/backlight`)
- **외부 모니터**: DDC/CI(`ddcutil`)로 제어 — COSMIC 상단바 밝기 슬라이더는 백라이트 sysfs만 읽어 외부 모니터가 누락되는 문제를 보완
- 슬라이더는 드래그 중 즉시 반영하고 놓을 때 적용 (ddcutil 호출 지연 회피)
- **재인식** 버튼: 재부팅 후 `i2c-dev` 모듈 미로드 등으로 외부 모니터가 안 보일 때 모듈 재로드 + udev 재트리거
- ddcutil 설치 시 udev 룰이 연결된 모니터 i2c 장치에 세션 사용자 ACL(uaccess)을 부여하므로 i2c 그룹 가입·재로그인 없이 동작. 미설정 시 "권한 설정" 카드 노출

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
- Orca 설치 — 공식 릴리스(`stablyai/orca`)에서 최신 `.deb`를 받아 설치하고
  독 즐겨찾기까지 등록한다. 버전과 sha512는 릴리스의 `latest-linux.yml`에서
  읽으므로 새 버전이 나와도 그대로 동작한다. 네트워크가 막혔거나 이미
  AppImage를 받아둔 경우에는 AppImage를 `~/Applications`로 정리해
  아이콘·바로가기·독에 등록하는 경로로 넘어간다.
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
2. `~/.local/share/applications/com.eondcom.Popmgr.desktop` 등록
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
cp com.eondcom.Popmgr.desktop ~/.local/share/applications/
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

## 커널 알려진 문제

커널 버전별 안정성 문제와 권장 버전은 [`docs/kernel-known-issues.md`](docs/kernel-known-issues.md) 에 기록한다.
새 커널 업데이트 후 시스템이 불안정해지면 이 문서를 먼저 확인하고 권장 버전으로 롤백한다.

- **현재 권장 커널: `7.0.9-76070009-generic`**
- 회피: `7.0.11-76070011` — slab shrinker 손상으로 반복 하드 프리즈 (6/12 업그레이드 회귀)

## 참고

- 한글 IME 설정 가이드: [cosmic-os-korean](https://github.com/Hostingglobal-Tech/cosmic-os-korean)
- Kensington 트랙볼 Bluetooth/USB 조사 노트: [`docs/bluetooth-usb-trackball-notes.md`](docs/bluetooth-usb-trackball-notes.md) (2026-09-11 해결)
- 리눅스에서 켄징턴 트랙볼 블루투스 연결하기(게시판 팁): [`docs/tip-kensington-trackball-bluetooth-linux.md`](docs/tip-kensington-trackball-bluetooth-linux.md)
- 폰트: Pretendard Regular/SemiBold/Bold (EOND UI App, OFL — `assets/LICENSE-Pretendard.txt`) + DejaVu Sans (기호 폴백)

## 변경 이력

### 2026-09-24 — 배터리 자동 절전 · 한글 풀림 · 3손가락 · 경로 복사 · 디자인 정합
- **전원 탭 "배터리 부족 시 자동 절전"**: 방전 중 임계값(기본 5%) 이하면 root systemd 타이머(1분)가 절전, 복귀 후 3분 유예.
  - 이유: UPower 1.90.3 은 `CriticalPowerAction=Suspend` 미지원이고 이 PC 는 디스크 스왑이 없어 HybridSleep 불가 → 2%에서 PowerOff 로 폴백해 작업이 날아갔다. Pop!_OS 저장소엔 1.90.3 뿐(Suspend 지원은 1.90.9+).
- **IME 탭 fcitx5 중복 기동 진단/해제**: 유닛과 `/etc/xdg/autostart` 가 fcitx5 를 2개 띄워 Wayland IM 을 잃는 경우(Wayland 앱에서만 영문) — 자동실행을 `Hidden=true` 로 끔. 재시작은 `--replace` 대신 유닛 경유. 절전 훅 v3(`systemd-run --user`, v2 는 suspend cgroup 과 함께 죽어 무효였음).
- **3손가락 제스처**: cosmic-comp 패치 폐기 → libinput 누적 이동량으로 판정하는 사용자 서비스(apt 업그레이드에 안 지워짐).
- **경로 복사 패치**: `--fuzz 5` 제거, 적용 후 검증, 업그레이드로 소실 시 '다시 적용' 표시.
- **디자인**: ui.eond.com/app 토큰 적용 — Pretendard(본문 400·버튼 600·제목 700), 버튼 `eond_ui_theme` 스타일(36 높이·모서리 10), 카드 `container::card`(모서리 14·여백 14), 글자 13/12/11, 라운딩 토큰. 이전엔 색만 토큰이었다.

### 2026-09-13 — fcitx5 한/영 상태 유지
- fcitx5 전역 설정 진단/교정 카드 추가 (`ShareInputState=All`, `ActiveByDefault=True`, `AltTriggerKeys=` 빈 값).
  - 이유: "다른 창 왔다갔다하면 영문만 입력되고 popmgr 재설정해야 돌아온다" 제보. 원인은 fcitx5 기본값 `ShareInputState=No`(입력 컨텍스트마다 상태 분리) + `ActiveByDefault=False`(새 컨텍스트는 영문 시작). 기존 "적용"은 데몬을 재시작해 모든 컨텍스트를 새로 만들므로 오히려 전부 영문으로 리셋하고 XIM 앱 연결까지 끊었다.
  - `AltTriggerKeys` 기본값 `Shift_L` 은 왼쪽 Shift 를 단독으로 눌렀다 떼면 영문으로 바뀌는 동작 — "이유 모를 영문 전환"의 또 다른 원인이라 함께 해제. 섹션만 지우면 fcitx5 가 기본값으로 되돌리므로 `[Hotkey]` 에 `AltTriggerKeys=` 를 명시.
  - 파일 수정 후 `fcitx5-remote -r` 로 재로드하고 D-Bus `GetConfig` 로 실제 반영을 확인 — 재시작 없음.
- "적용" 시 fcitx5 면 위 설정을 함께 보장. 설치 패키지에 `fcitx5-frontend-gtk4/qt5/qt6` 추가, 누락 진단 카드 추가.
- "IME 재시작" 버튼 추가 (pkexec 없이 데몬만 재시작). CLI `popmgr --fix-fcitx5-behavior` 추가.

### 2026-06-19 — 디스플레이 탭 신설 (모니터 밝기)
- 내장(logind)·외부(ddcutil DDC/CI) 모니터 밝기·명암 조절 탭 추가. 외부 모니터는 백라이트 sysfs가 없어 COSMIC 상단바에 안 뜨던 것을 보완.
- 외부 모니터 미인식 대비 '재인식' 버튼(i2c-dev 재로드 + udev 재트리거) 및 권한 미설정 시 '권한 설정' 카드 제공.
- 기본 글꼴을 NanumSquare **Bold** 페이스로 변경 — 라이트 테마에서 Regular 가 가늘어 흐려 보이던 문제 해결 (cosmic-text 0.12 는 합성 볼드 미지원이라 Bold ttf 임베드).

### 2026-06-18 — 커널 알려진 문제 문서 추가
- `docs/kernel-known-issues.md` 신설. 커널 `7.0.11-76070011` 의 slab shrinker 손상 반복 프리즈 진단/롤백 절차 기록.
  - 이유: 6/12 커널 업그레이드(7.0.9→7.0.11) 직후부터 `kswapd0 → shrink_slab` 경로에서 GPF 가 반복 발생해 하드 프리즈. 하드웨어 오류(MCE/EDAC) 없음 → 커널 회귀로 판정. 권장 커널 `7.0.9-76070009` 명시.

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
