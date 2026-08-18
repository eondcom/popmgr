# LibreOffice 한글 지원 — HWP 파일 포맷 + 한글 입력(IME)

두 가지는 서로 다른 문제다. 하나씩 따로 고친다.

1. **HWP 파일을 열고 읽을 수 있는가** — 파일 포맷 문제, 확장 설치로 해결
2. **문서 안에서 한글을 입력(한영 전환)할 수 있는가** — IME 문제. snap 환경 누출과 COSMIC Wayland 경로를 각각 진단

---

## 1. HWP(한글과컴퓨터) 파일 포맷 지원 — H2Orestart

기본 LibreOffice는 `.hwp`(5.0) 파일을 못 연다 (`--convert-to` 시도 시 `Error: source file could not be loaded`).
원인은 hwp 임포트 필터가 아예 없어서다. `H2Orestart`라는 오픈소스 확장이 이 필터를 추가한다.

### 증상
```
$ soffice --headless --convert-to odt --outdir out "파일.hwp"
Error: source file could not be loaded
```

### 원인 1 — Java 연동 패키지 누락
`unopkg`(확장 설치 CLI)가 자체 Java 브릿지를 못 찾아 실패:
```
ERROR: Exception occurred: [JavaVirtualMachine]:An unexpected error occurred while searching for a Java, 11
```
`libreoffice-java-common` 패키지가 없으면 발생한다 (Java가 시스템에 있어도 소용없음 — LO 전용 연동 패키지가 따로 필요).

```bash
sudo apt install -y libreoffice-java-common
```

### 원인 2 — hwp 임포트 필터 자체가 없음
`H2Orestart.oxt`를 받아서 `unopkg`로 설치한다.

```bash
# 최신 릴리스 다운로드
gh release download --repo ebandal/H2Orestart --pattern "H2Orestart.oxt" -O H2Orestart.oxt

# 설치 (기본 사용자 프로필에)
unopkg add --force H2Orestart.oxt

# 확인
unopkg list
# → ebandal.libreoffice.H2Orestart, is registered: yes 가 보이면 성공
```

### 검증
```bash
soffice --headless --norestore --convert-to odt --outdir /tmp/out "파일.hwp"
# convert ... using filter : writer8   ← 이 로그가 뜨면 정상 변환된 것
```

### 알아둘 것 — 이미 실행 중인 LibreOffice가 있으면 새 headless 호출이 조용히 무시된다
LO는 프로필(UserInstallation)당 인스턴스를 하나만 허용한다. 같은 사용자 프로필로 GUI가 이미 떠 있는 상태에서
`soffice --headless --convert-to ...`를 또 실행하면 기존 인스턴스에 연결을 시도하다 **아무 에러 없이 아무 파일도 안 만들고 끝난다**
(`exit=0`인데 출력 파일이 없음 — 가장 헷갈리는 실패 패턴).

해결: 자동화/변환 작업은 격리된 프로필로 돌린다.
```bash
soffice -env:UserInstallation=file:///tmp/lo_profile --headless --convert-to odt --outdir out "파일.hwp"
```
단, 확장(H2Orestart)도 **그 격리 프로필에 따로 설치**해야 한다 (`unopkg add -env:UserInstallation=file:///tmp/lo_profile ...`) —
기본 프로필에 설치했다고 격리 프로필에서 자동으로 보이지 않는다.

### 한계 — 쓰기(export)는 안 됨
H2Orestart는 **import 전용**이다. `.hwp`로 다시 저장하는 필터는 없다:
```
Error: no export filter for ..."파일.hwp" found, aborting.
```
즉 이 확장으로 hwp를 "열람/PDF 변환/텍스트 추출"까지는 되지만, 채워서 다시 hwp로 뽑는 건 안 된다.
그 용도로는 별도로 `hwplib`(Java, 순수 포맷 라이브러리) 직접 조작이나 한컴독스(웹) 수동 편집이 필요하다.

---

## 2. 한글 입력(IME, 한영 전환)

### 원인 A — waveterm snap 누출

#### 증상
fcitx5는 정상 실행 중이고 `GTK_IM_MODULE=fcitx` 등 환경변수도 맞게 잡혀 있는데,
LibreOffice(및 이 터미널에서 띄운 다른 GTK 앱)에서 한영 전환이 안 먹는다.
popmgr의 IME 탭으로 재연결/재시작을 해봐도 이 터미널에서 새로 띄운 앱은 여전히 안 됨.

#### 원인 — `GTK_IM_MODULE_FILE`이 snap 캐시를 가리킴
```bash
$ echo $GTK_IM_MODULE_FILE
/home/dell/snap/waveterm/common/.cache/immodules/immodules.cache
```
이 세션이 돌아가는 **Wave Terminal(`waveterm`)이 snap(classic confinement) 패키지**다.
snap이 자기 컨테이너 안의 GTK IM 모듈 캐시 경로를 자식 프로세스(이 셸에서 띄우는 모든 GTK 앱)에 주입한다.
문제는 그 캐시 파일 안에 **fcitx5 GTK 모듈이 등록되어 있지 않다는 것** —
```bash
$ grep fcitx "$GTK_IM_MODULE_FILE"
(결과 없음)
```
그래서 이 터미널에서 켠 GTK 앱은 fcitx5 데몬이 떠 있어도 그 모듈 자체를 못 찾아 한영 전환이 죽는다.

이건 popmgr README가 이미 경고하는 "snap 누출" 패턴과 정확히 같은 종류지만,
popmgr의 자동 정리는 `~/.profile`, `~/.bashrc` 같은 **정적 파일에 박힌 export**만 손본다.
이 값은 snap이 **세션마다 동적으로 주입**하는 것이라 dotfile 정리로는 안 잡힌다 — popmgr를 돌려도 안 고쳐지는 이유.

#### 해결
**즉시(세션 한정)**: 문제되는 앱을 띄우기 직전에 override
```bash
GTK_IM_MODULE_FILE=/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache soffice
```

**영구(이 터미널에서 매번)**: 셸 rc 파일(`~/.bashrc` 등, waveterm이 소싱하는 시점 *이후*)에 강제 override 한 줄 추가
```bash
export GTK_IM_MODULE_FILE=/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache
```
시스템 아키텍처가 다르면 경로가 다를 수 있음 — 실제 값은 (이 파일은 패키지가 아니라 런타임에 생성되는
캐시라 `dpkg -L`로는 안 잡히고 `find`로 찾아야 함):
```bash
find /usr/lib -xdev -iname "immodules.cache" 2>/dev/null | grep -v snap
```

**근본적으로는**: waveterm 자체를 snap이 아닌 다른 배포 방식(있다면)으로 바꾸면 이 클래스의 누출이 원천적으로 없어짐.

#### popmgr 개선 여지 (참고)
현재 popmgr의 snap 누출 감지는 `GTK_IM_MODULE_FILE`이 snap 경로를 가리키는 걸 **경고**는 하지만
자동 정리 대상에 이 변수 자체는 없는 것으로 보임(대상은 IME 관련 export들). dotfile에 override export를
자동으로 추가/제거하는 기능을 추가하면 이 케이스도 popmgr 한 번 실행으로 해결 가능해질 것.

### 원인 B — LibreOffice의 COSMIC Wayland 입력 경로

LibreOffice를 COSMIC 앱 런처에서 실행한 경우에는 `GTK_IM_MODULE_FILE` 누출이 없고
`GTK_IM_MODULE=fcitx`, `XMODIFIERS=@im=fcitx`도 정상인데 한/영 전환이 안 될 수 있다.
이때는 실행 프로세스가 fcitx GTK 모듈을 실제로 연결했는지 확인한다.

```bash
pid="$(pgrep -x soffice.bin | head -n 1)"
grep -E 'im-fcitx5|libgtk-3|libwayland' "/proc/$pid/maps"
```

이 머신에서는 `libgtk-3`와 `libwayland`는 로드됐지만 `im-fcitx5.so`는 없었다.
fcitx5 데몬, Hangul 엔진, GTK3 모듈 캐시는 모두 정상이므로 LibreOffice의 native Wayland
입력 경로 문제로 좁혀진다.

`GDK_BACKEND=x11`만 지정하면 COSMIC에서 LibreOffice가 계속 gdk-wayland 소켓을 사용하는 것이
확인됐다. `WAYLAND_DISPLAY`를 환경에서 제거하고 GTK의 XIM 모듈을 선택해야 fcitx5에
`program:soffice.bin frontend:xim` 입력 컨텍스트가 생성된다.

```bash
env -u WAYLAND_DISPLAY GDK_BACKEND=x11 GTK_IM_MODULE=xim libreoffice --writer
```

popmgr IME 탭의 **LibreOffice 한글 입력 호환 모드**는 `/usr/share/applications`의 시스템 파일을
수정하지 않는다. 대신 같은 desktop ID의 사용자 사본을 `~/.local/share/applications`에 만들고
모든 `Exec=`에 `env -u WAYLAND_DISPLAY GDK_BACKEND=x11 GTK_IM_MODULE=xim`을 붙인다.
해제 시 popmgr 마커가 있는 사본만 삭제한다.
설치 또는 해제 후에는 열려 있는 LibreOffice를 모두 닫고 다시 실행해야 한다.
