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

## 라이선스

MIT
