# popmgr

Pop!_OS / COSMIC 데스크톱 관리 도구 — Rust + [Iced](https://github.com/iced-rs/iced) GUI

여러 개의 유틸리티 스크립트·앱을 하나로 통합했습니다.

## 기능

| 탭 | 기능 |
|---|---|
| **IME** | ibus / fcitx5 / kime 설치·선택·적용 (`/etc/environment` 자동 관리) |
| **USB** | USB 장치 목록, ktrackball 데몬 상태/재시작, xHCI 컨트롤러 리셋 |
| **COSMIC 트윅** | [cosmic-files copy-path 패치](https://github.com/eondcom/cosmic-files-copy-path) — 항상 '경로 복사' 표시<br>[cosmic-comp 3-finger 패치](https://github.com/eondcom/cosmic-three-finger-gesture) — 3손가락 워크스페이스 전환 |
| **앱 관리** | [KakaoTalk Wine](https://github.com/eondcom/kakaotalk-wine) 설치/실행, APT·Flatpak 패키지 검색·제거 |

## 빌드 및 실행

```bash
# 의존성 (Ubuntu / Pop!_OS)
sudo apt install libfontconfig1-dev libxkbcommon-dev

# 빌드
cargo build --release

# 실행
./target/release/popmgr
```

## COSMIC 패치 동작 방식

"패치 적용" 버튼을 누르면:
1. 패치가 적용된 소스를 `/tmp`에 클론
2. `cargo build --release` (수 분 소요)
3. `pkexec`로 빌드된 바이너리를 `/usr/bin`에 설치

시스템 업데이트 후 패치가 덮어쓰이면 "패치 적용"을 다시 누르세요.

## 참고

- 한글 IME 설정: [cosmic-os-korean](https://github.com/Hostingglobal-Tech/cosmic-os-korean)
- COSMIC 환경에서는 **kime**가 가장 안정적입니다

## 라이선스

MIT
