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
- **fcitx5 툴킷 프론트엔드 누락 진단**: 그 툴킷 GUI 라이브러리(libgtk/libqt*gui)가 설치된 경우에만 `fcitx5-frontend-*` 누락을 경고 + 설치 버튼. (Qt 앱이 없는데 무조건 요구하던 것을 2026-09-24 수정)
- **fcitx5 중복 기동 진단**: `fcitx5-korean.service` 와 xdg 자동실행이 fcitx5 를 2개 띄우면 Wayland IM 을 잃어 Wayland 앱에서만 영문이 된다. '중복 끄기'가 `~/.config/autostart` 에 같은 이름 `Hidden=true` 를 써서 유닛 하나만 남긴다(다음 로그인부터).
- **절전 복귀 훅 v3**: `/etc/systemd/system-sleep/zz-popmgr-ime-restart` 가 `systemd-run --user`(KillMode=process)로 IME 를 재시작. v1·v2 는 동작하지 않았다(v2: 자식이 suspend cgroup 과 함께 종료). 실행 기록은 `journalctl -t popmgr-ime`.
- **IME 재시작 버튼**: `/etc/environment` 재작성(pkexec) 없이 활성 IME 데몬만 재시작. fcitx5 는 `--replace` 대신 종료 대기 후 유닛 재시작.

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

### 아이패드 탭 (보조화면 / 터치)
- 아이패드를 Wi-Fi 로 **터치 되는 보조화면**으로 — Weylus CE(flatpak)의 COSMIC 패치판을 관리한다.
  - 원본 Weylus 는 COSMIC 에서 `failed to init screen cast`: COSMIC 스크린캐스트는 RGBA/BGRA 만 주는데
    Weylus 는 BGRx/RGBx 만 받는다. flatpak 바이너리 복사본(`~/Applications/weylus-cosmic/weylus`)의 형식 문자열 5곳을 바꾼다.
- 설치·패치 상태(원본 sha256 비교로 flatpak 업데이트 감지) / 다시 패치(치환 수가 5가 아니면 중단).
- 시작·중지(`--no-gui`, 접속 코드), 접속 URL + **QR 코드**(`?access_code=`), LAN/Tailscale 전환, 코드 재생성.
- 인코더 CPU / Intel GPU(VAAPI) / NVIDIA(NVENC) — 고르면 설명(발열 기준 실측)이 보이고 '적용'으로 저장·재시작. 기본 VAAPI.
  - GPU 인코딩은 자체 빌드 ffmpeg 8.1.1(`~/Applications/weylus-cosmic/ffmpeg`, 빌드 스크립트 `build-ffmpeg.sh`)이 있어야 한다.
- 방화벽 허용(pkexec ufw, LAN /24), 사용 팁. CLI `popmgr --tablet-status | --tablet-start | --tablet-stop`.

### COSMIC 트윅 탭
- **cosmic-files copy-path** — 탐색기 우클릭에 '경로 복사' 항상 표시 (패치 없이도 Shift+우클릭 / Ctrl+Shift+C 로 가능)
- **3손가락 제스처** — 터치패드 3손가락 위 스와이프 → 워크스페이스 오버뷰 열기/닫기.
  컴포지터 패치가 아니라 `libinput debug-events` 를 읽는 **systemd 사용자 서비스**(`popmgr-gestures.service`)라
  root·재빌드가 필요 없고 apt 업그레이드에 지워지지 않는다. 예전 cosmic-comp 패치가 남아 있으면 먼저 제거하도록 안내한다.

copy-path 패치 적용 방식:
1. `dpkg`로 현재 설치된 버전의 커밋 해시 확인
2. GitHub에서 해당 커밋 타르볼 다운로드
3. `patch -p1` 적용(기본 fuzz) 후 메뉴 2곳이 바뀌었는지 검증 — 실패하면 설치하지 않음
4. `cargo build --release`
5. `pkexec`로 `/usr/bin`에 설치 (원본 `.bak` 백업), 적용 버전 기록

> apt 업그레이드가 패치를 덮어쓰면 카드에 "업그레이드로 소실 — 다시 적용"이 뜬다.

### 전원 탭
- 지금 절전 / 절전 방지(최대 12시간) / 예약 절전
- **배터리 부족 시 자동 절전**: 방전 중 임계값(기본 5%) 이하면 root systemd 타이머(`popmgr-battery-guard.timer`, 1분)가
  `systemctl suspend -i`. 복귀 후 3분 유예. popmgr 가 꺼져 있어도 동작. '지금 점검'은 DRY_RUN 으로 판정만 보여 줌.

### 앱 관리 탭
- KakaoTalk Wine 설치 / 실행
- Orca 설치 — 공식 릴리스(`stablyai/orca`)에서 최신 `.deb`를 받아 설치하고
  독 즐겨찾기까지 등록한다. 버전과 sha512는 릴리스의 `latest-linux.yml`에서
  읽으므로 새 버전이 나와도 그대로 동작한다. 네트워크가 막혔거나 이미
  AppImage를 받아둔 경우에는 AppImage를 `~/Applications`로 정리해
  아이콘·바로가기·독에 등록하는 경로로 넘어간다.
- APT·Flatpak 패키지 검색 및 일괄 제거
- **화면 녹화**(GPU Screen Recorder, NVENC): Ctrl+Shift+6 전체 화면, **Ctrl+Shift+7 영역**(맥 Cmd+Shift+5 의 "선택 부분 기록").
  영역은 slurp 로 드래그해 고르고, 포털로 전체를 녹화한 뒤 정지할 때 ffmpeg 로 그 영역만 잘라 저장(배율 반영, NVENC → x264 폴백).
  GSR `-w region`(KMS)은 이 PC 에서 root 헬퍼가 실패해 쓰지 않는다. 출력: `~/Videos/Recordings`

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
3. cosmic-files copy-path 패치 적용 (설치된 버전 커밋 기준, 검증 실패 시 건너뜀)
4. 3손가락 제스처는 설치 후 popmgr COSMIC 탭에서 '켜기'

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

# (예전 3-finger 컴포지터 패치가 남아 있을 때) cosmic-comp 스톡 복원 — 이후 재로그인
sudo apt-get install --reinstall cosmic-comp
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

### 2026-09-28 — 아이패드 보조화면 탭 · 사이드바 스크롤
- **아이패드 탭**(위 기능 참고). 조사 순서와 버린 대안:
  - USB-C 직결 불가(아이패드는 영상 입력 없음, `lsusb` 에 USB 기기로만 잡힘). 사이드카·Spacedesk·Duet 은 리눅스 미지원. AirPlay 는 방향 반대.
  - Deskreen CE(AppImage)는 동작하지만 보기 전용 → 터치가 되는 Weylus 선택.
  - Weylus 실패 원인은 PipeWire `no more output formats`(COSMIC RGBA vs Weylus RGBx). 소스 `src/capturable/pipewire.rs:145-154`.
    재빌드엔 gstreamer 등 -dev 패키지(sudo)가 필요해 바이너리 문자열 치환으로 해결, 실기로 영상·터치 확인.
  - `--try-vaapi` 무효 원인 2겹: flatpak 번들 ffmpeg 가 VAAPI 없이 빌드됨(런타임 ffmpeg 는 7.x 라 교체 불가) +
    ffmpeg 8 은 필터 그래프 parse 중 init 하는데 Weylus 는 그 뒤에 hwupload 에 장치를 붙임. ffmpeg 8.1.1 자체 빌드 +
    `vf_hwupload.c` 지연 획득 패치 → `h264_vaapi`, Weylus CPU 123~145% → 60%.
  - 창(window) 공유 시 터치 좌표가 어긋남(Wayland 입력은 모니터 좌표) → 팁에 "모니터 선택" 명시.
- **사이드바 스크롤**: 탭이 11개가 되며 높이 680 창에서도 '앱 관리'가 잘렸다 → 탭 목록 `scrollable`.
- 커뮤니티 글 https://ai.eond.com/community/493465 · 스레드 https://www.threads.com/@eondcom/post/Ddz_hc-k2qT

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

## 다음 세션에서 할 일

작업 현황판: 구글시트 "EOND ALL Project management" → `popmgr` 탭(#1~#5). 스펙: `.claude/plans/2026-09-24-battery-guard.md`.

1. **한글 — 재로그인 후 확인**: fcitx5 가 하나만 떠야 한다(중복 끄기 적용됨, 2026-09-24 22:18).
   ```bash
   pgrep -c -x fcitx5                                   # 1 이어야 함
   journalctl --user -b _COMM=fcitx5 | grep -c 'Loaded addon waylandim'   # 1 이어야 함
   ```
2. **한글 — 절전 복귀 확인**: 훅 v3 설치됨. 절전 → 복귀 후 한글 입력 + 로그 확인.
   ```bash
   journalctl -b -t popmgr-ime          # "resume: fcitx5 restart scheduled" 가 있어야 함
   ```
3. **3손가락**: COSMIC 탭 '컴포지터 패치 제거' → 재로그인 → '켜기' → 실제 스와이프 확인
   (`journalctl --user -u popmgr-gestures` 에 `UP <이동량>`). 임계값 60 이 안 맞으면 스크립트 `THRESH` 조정.
4. **경로 복사**: COSMIC 탭 '패치 적용'(빌드 수 분) → 우클릭 메뉴 확인.
5. **PR #13 병합**(draft): https://github.com/eondcom/popmgr/pull/13
6. (선택) plucky 에서 들어온 고아 Qt 라이브러리 정리 — 나중에 apt 로 Qt 앱 설치 시 ABI 충돌 원인:
   `sudo apt remove libqt6core6t64 libqt6dbus6 libqt5core5t64 libqt5dbus5t64 libqt5network5t64`
7. (선택) popmgr 입력칸 한글 입력: iced 0.13 은 IME 미지원 → iced 0.14 업그레이드 필요.
8. **아이패드 탭 실기 확인**(2026-09-28 설치본): 아이패드로 QR 스캔 → 접속, 인코더 '적용' 버튼으로 재시작되는지.
   ```bash
   popmgr --tablet-status        # running·clients·current_encoder 확인
   grep 'Video:' ~/.cache/popmgr/weylus.log | tail -1   # @h264_vaapi 여야 함
   ```
9. (선택) 아이패드 NVENC 실기 확인 — 자체 ffmpeg 는 nv-codec-headers 13.0(드라이버 580 호환)으로 빌드했지만 미검증.
10. (선택) Weylus 자동 시작(탭 열 때 / 로그인 때) — 사용자에게 제안만 함, 미구현.
11. (선택) 릴리스 빌드 단축: `lto = "thin"`, `codegen-units = 16` — 현재 fat LTO 로 최종 링크가 코어 1개로 수 분.
12. flatpak Weylus 가 업데이트되면 탭에 "다시 패치 필요" 경고 → '다시 패치'. ffmpeg 는 `~/Applications/weylus-cosmic/build-ffmpeg.sh`
    재실행 시 `vf_hwupload.c` 패치를 다시 넣어야 한다(소스 트리 `~/Applications/weylus-cosmic/src/ffmpeg-8.1.1` 에 적용돼 있음).

## 세션 로그

- **2026-09-24**: 배터리 저잔량 자동 절전(root 타이머, 실기 9%에서 절전 확인), fcitx5 이중 기동·절전 훅 v3,
  3손가락 사용자 서비스, copy-path 패치 안전화, EOND UI 디자인 토큰(Pretendard 등), Qt 툴킷 경고 수정, install.sh 정리.
  Codex 는 사용량 한도(10-18까지)로 Claude 가 직접 구현. 스레드 글 Ddq3iyWE6-c + 답글 3개(스크린샷).

- **2026-09-28**: 아이패드 보조화면 — Deskreen(보기 전용) → Weylus COSMIC 형식 패치(터치) → ffmpeg 8.1.1 VAAPI 자체 빌드·
  hwupload 패치(CPU 145→60%). popmgr '아이패드' 탭(QR·접속코드·인코더 선택→적용·재패치·방화벽) + 사이드바 스크롤.
  Codex 한도로 Sonnet 서브에이전트 구현 → Claude 검증(테스트 109, CLI E2E, 접속코드, Xvfb 캡처). master e130aad.
  커뮤니티 493465 + 스레드 Ddz_hc-k2qT. 산출물 `.claude/plans/2026-09-28-artifacts/`.

&nbsp;

관련 세션

    claude --resume 75627a71-ba2d-49de-a38a-07aa8a43567d   # 2026-09-24
    claude --resume cbfa4506-1789-4d69-8742-6ff640e3a46e   # 2026-09-28 아이패드

## 라이선스

MIT
