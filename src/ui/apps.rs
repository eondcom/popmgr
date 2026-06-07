use iced::{
    widget::{column, container, row, scrollable, text, text_input, Space},
    Color, Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_DIM, C_ERR, C_OK, C_WARN};

#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub description: String,
    pub kind: PkgKind,
    pub marked: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PkgKind { Apt, Flatpak }

#[derive(Debug, Clone)]
pub struct AppsStatus {
    pub kakaotalk_installed: bool,
    pub kakaotalk_launcher: Option<String>,
    pub kakaotalk_exe: Option<String>,
    pub kakaotalk_desktop: Option<String>,
    pub kakaotalk_wmclass_ok: bool,
    pub kakaotalk_icon_ok: bool,
    pub kakaotalk_ime_patched: bool,
    pub packages: Vec<Package>,
}

#[derive(Debug, Clone)]
pub enum AppsMsg {
    Refresh,
    Refreshed(AppsStatus),
    SearchChanged(String),
    TogglePkg(usize),
    RemoveMarked,
    InstallKakaotalk,
    LaunchKakaotalk,
    FixKakaotalkDesktop,
    FixKakaotalkIcon,
    FixKakaotalkIme,
    Done(CmdResult),
}

pub struct AppsState {
    pub status: Option<AppsStatus>,
    pub search: String,
    pub running: Option<String>,
}

impl AppsState {
    pub fn new() -> Self {
        Self { status: None, search: String::new(), running: None }
    }

    pub fn update(&mut self, msg: AppsMsg) -> (Task<AppsMsg>, Option<CmdResult>) {
        match msg {
            AppsMsg::Refresh => {
                let t = Task::perform(async { scan_apps().await }, AppsMsg::Refreshed);
                (t, None)
            }
            AppsMsg::Refreshed(s) => { self.status = Some(s); (Task::none(), None) }
            AppsMsg::SearchChanged(s) => { self.search = s; (Task::none(), None) }
            AppsMsg::TogglePkg(i) => {
                if let Some(st) = &mut self.status {
                    if let Some(pkg) = st.packages.get_mut(i) {
                        pkg.marked = !pkg.marked;
                    }
                }
                (Task::none(), None)
            }
            AppsMsg::RemoveMarked => {
                let pkgs: Vec<Package> = self.status.as_ref()
                    .map(|s| s.packages.iter().filter(|p| p.marked).cloned().collect())
                    .unwrap_or_default();
                if pkgs.is_empty() {
                    return (Task::none(), Some(CmdResult { success: false, output: "제거할 패키지를 선택해주세요.".into() }));
                }
                let names: Vec<&str> = pkgs.iter().map(|p| p.name.as_str()).collect();
                self.running = Some(format!("제거 중: {}...", names.join(", ")));

                let apt: Vec<String> = pkgs.iter().filter(|p| p.kind == PkgKind::Apt)
                    .map(|p| p.name.clone()).collect();
                let flat: Vec<String> = pkgs.iter().filter(|p| p.kind == PkgKind::Flatpak)
                    .map(|p| p.name.clone()).collect();

                let mut script = String::new();
                if !apt.is_empty() {
                    script.push_str(&format!("pkexec apt-get remove --purge -y {} && pkexec apt-get autoremove -y\n", apt.join(" ")));
                }
                for f in &flat {
                    script.push_str(&format!("flatpak uninstall -y {f}\n"));
                }

                let t = Task::perform(async move { runner::run_sh(&script).await }, AppsMsg::Done);
                (t, None)
            }
            AppsMsg::InstallKakaotalk => {
                self.running = Some("KakaoTalk 검증 환경 설치 중...".into());
                // 검증된 셋업: Bottles flatpak + Wine 11.10-staging runner + KakaoTalk32 win32 prefix
                // + d3d builtin DLL + portable i386 libs (시스템 broken 우회) + 한글/EGL fix 런처
                // 시스템 dpkg 상태 무관하게 동작. 각 단계 idempotent.
                let script_active = r##"
                    set -e
                    echo "=== [1/8] Bottles flatpak (--user) ==="
                    if flatpak info --user com.usebottles.bottles >/dev/null 2>&1; then
                        echo "이미 설치됨"
                    else
                        flatpak install --user --noninteractive flathub com.usebottles.bottles 2>&1 | tail -5
                    fi

                    echo
                    echo "=== [2/8] Wine 11.10-staging runner ==="
                    RUNNER_DIR="$HOME/.var/app/com.usebottles.bottles/data/bottles/runners/wine-11.10-staging-amd64"
                    if [ -x "$RUNNER_DIR/bin/wine" ]; then
                        echo "이미 있음"
                    else
                        mkdir -p "$(dirname "$RUNNER_DIR")"
                        TMPR=$(mktemp -d)
                        curl -L --progress-bar -o "$TMPR/wine.tar.xz" \
                            https://github.com/Kron4ek/Wine-Builds/releases/download/11.10/wine-11.10-staging-amd64.tar.xz
                        tar -xJf "$TMPR/wine.tar.xz" -C "$(dirname "$RUNNER_DIR")"
                        rm -rf "$TMPR"
                    fi

                    echo
                    echo "=== [3/8] portable i386 libs (시스템 dpkg 안 건드림) ==="
                    I386_DIR="$HOME/.kakaotalk-wine/i386libs"
                    if [ -f "$I386_DIR/usr/lib/i386-linux-gnu/libfreetype.so.6" ]; then
                        echo "이미 있음"
                    else
                        # i386 multiarch 활성화 시도 (시스템 broken 있으면 그대로 진행)
                        pkexec sh -c 'dpkg --add-architecture i386 2>/dev/null; apt-get update 2>/dev/null; true' || true
                        TMPI=$(mktemp -d) && cd "$TMPI"
                        apt download \
                            libfreetype6:i386 libfontconfig1:i386 \
                            libx11-6:i386 libxext6:i386 libxrender1:i386 libxrandr2:i386 \
                            libxcomposite1:i386 libxi6:i386 libxcursor1:i386 libxfixes3:i386 \
                            libpng16-16t64:i386 libexpat1:i386 \
                            libbrotli1:i386 libbz2-1.0:i386 zlib1g:i386 \
                            libxcb1:i386 libxau6:i386 libxdmcp6:i386 2>&1 | tail -3
                        mkdir -p "$I386_DIR"
                        for d in *.deb; do dpkg-deb -x "$d" "$I386_DIR/" 2>/dev/null; done
                        cd / && rm -rf "$TMPI"
                    fi

                    echo
                    echo "=== [4/8] KakaoTalk32 prefix + 카카오톡 본체 ==="
                    PREFIX="$HOME/.var/app/com.usebottles.bottles/data/bottles/bottles/KakaoTalk32"
                    KAKAO_EXE="$PREFIX/drive_c/Program Files/Kakao/KakaoTalk/KakaoTalk.exe"
                    if [ -f "$KAKAO_EXE" ]; then
                        echo "이미 설치됨"
                    else
                        mkdir -p "$(dirname "$PREFIX")"
                        # wineboot
                        flatpak run --command=bash com.usebottles.bottles -c "
                            export WINEPREFIX='$PREFIX'
                            export WINEARCH=win32
                            export WINEDEBUG=-all
                            '$RUNNER_DIR/bin/wine' wineboot --init 2>&1 | head -3
                        " || true
                        # 카오톡 setup 다운로드
                        SETUP=/tmp/KakaoTalk_Setup.exe
                        curl -L --progress-bar -o "$SETUP" \
                            https://app-pc.kakaocdn.net/talk/win32/KakaoTalk_Setup.exe
                        flatpak run --command=bash com.usebottles.bottles -c "
                            export WINEPREFIX='$PREFIX'
                            export WINEARCH=win32
                            export WINEDEBUG=-all
                            '$RUNNER_DIR/bin/wine' '$SETUP' /S 2>&1 | head -5
                            '$RUNNER_DIR/bin/wineserver' -w 2>/dev/null
                        " || true
                        rm -f "$SETUP"
                    fi

                    echo
                    echo "=== [5/8] d3d builtin DLL 복사 (대화창 흰/검 창 차단) ==="
                    SYS32="$PREFIX/drive_c/windows/system32"
                    WINE_DLLS="$RUNNER_DIR/lib/wine/i386-windows"
                    for dll in d3d9 d3d10 d3d10core d3d11 dxgi; do
                        if [ -f "$WINE_DLLS/${dll}.dll" ]; then
                            cp -n "$WINE_DLLS/${dll}.dll" "$SYS32/" 2>/dev/null && echo "복사: ${dll}.dll" || true
                        fi
                    done

                    echo
                    echo "=== [6/8] 사용자 런처 (popmgr-ime-fix-v7) ==="
                    cat > "$HOME/.local/bin/kakaotalk" <<'LAUNCHER_EOF'
#!/bin/bash
# popmgr-ime-fix-v7 — Bottles KakaoTalk32 + 시스템 IM 자동 감지 + EGL NVIDIA path 명시
WIN32_PREFIX="$HOME/.var/app/com.usebottles.bottles/data/bottles/bottles/KakaoTalk32"
RUNNER="$HOME/.var/app/com.usebottles.bottles/data/bottles/runners/wine-11.10-staging-amd64"
KAKAO_EXE="$WIN32_PREFIX/drive_c/Program Files/Kakao/KakaoTalk/KakaoTalk.exe"
[ -z "$DISPLAY" ] && export DISPLAY=:1

# 이미 실행 중이면 윈도우 활성화
if pgrep -f "KakaoTalk\.exe" >/dev/null 2>&1; then
    wid=""
    if command -v xdotool >/dev/null; then
        wid=$(xdotool search --class "kakaotalk\.exe" 2>/dev/null | head -1)
        [ -z "$wid" ] && wid=$(xdotool search --name "^KakaoTalk$" 2>/dev/null | head -1)
    fi
    if [ -n "$wid" ]; then
        xdotool windowmap "$wid" 2>/dev/null
        xdotool windowactivate "$wid" 2>/dev/null
        xdotool windowraise "$wid" 2>/dev/null
        exit 0
    fi
    pkill -9 -f "KakaoTalk\.exe" 2>/dev/null
    pkill -9 -f "winedbg" 2>/dev/null
    sleep 0.5
fi

# 좀비 KakaoTalk32 wineserver 정리
for pid in $(pgrep -f wineserver 2>/dev/null); do
    [ -r "/proc/$pid/environ" ] && grep -qz "KakaoTalk32" "/proc/$pid/environ" 2>/dev/null && kill -9 "$pid" 2>/dev/null
done
sleep 0.3

# 시스템 IM 자동 감지 (ibus 강제 X)
SYS_IM="${XMODIFIERS#@im=}"
[ -z "$SYS_IM" ] && SYS_IM="ibus"
case "$SYS_IM" in
    fcitx|fcitx5)
        SYS_IM=fcitx
        pgrep -x fcitx5 >/dev/null 2>&1 || fcitx5 -d --replace >/dev/null 2>&1 &
        ;;
    ibus)
        pgrep -x ibus-daemon >/dev/null 2>&1 || ibus-daemon -dxr >/dev/null 2>&1 &
        ;;
esac
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
    xprop -root XIM_SERVERS 2>/dev/null | grep -qi "$SYS_IM" && break
    sleep 0.2
done

xsetroot -cursor_name left_ptr 2>/dev/null

exec flatpak run \
    --env=DISPLAY="$DISPLAY" \
    --env=XMODIFIERS="${XMODIFIERS:-@im=$SYS_IM}" \
    --env=QT_IM_MODULE="${QT_IM_MODULE:-$SYS_IM}" \
    --env=GTK_IM_MODULE="${GTK_IM_MODULE:-$SYS_IM}" \
    --env=LANG="${LANG:-ko_KR.UTF-8}" \
    --env=LC_ALL="${LC_ALL:-ko_KR.UTF-8}" \
    --env=__EGL_VENDOR_LIBRARY_DIRS="/usr/lib/x86_64-linux-gnu/GL/glvnd/egl_vendor.d:/app/lib/i386-linux-gnu/GL/glvnd/egl_vendor.d:/usr/lib/x86_64-linux-gnu/GL/default/glvnd/egl_vendor.d" \
    --env=__GLX_VENDOR_LIBRARY_NAME="${__GLX_VENDOR_LIBRARY_NAME:-nvidia}" \
    --command=bash com.usebottles.bottles -c "
export WINEPREFIX='$WIN32_PREFIX'
export WINEARCH=win32
xsetroot -cursor_name left_ptr 2>/dev/null
'$RUNNER/bin/wine' '$KAKAO_EXE'
"
LAUNCHER_EOF
                    chmod +x "$HOME/.local/bin/kakaotalk"
                    echo "✓ $HOME/.local/bin/kakaotalk"

                    echo
                    echo "=== [7/8] 사용자 desktop + 아이콘 ==="
                    mkdir -p "$HOME/.local/share/applications"
                    cat > "$HOME/.local/share/applications/kakaotalk.desktop" <<DESK_EOF
[Desktop Entry]
Name=KakaoTalk
Name[ko]=카카오톡
Comment=KakaoTalk Messenger
Comment[ko]=카카오톡 메신저
Exec=$HOME/.local/bin/kakaotalk %U
Icon=kakaotalk
Type=Application
Categories=Network;InstantMessaging;Chat;
Keywords=kakao;kakaotalk;카카오;카카오톡;메신저;
StartupNotify=true
StartupWMClass=kakaotalk.exe
MimeType=x-scheme-handler/kakaotalk;
DESK_EOF
                    # 아이콘: 시스템 deb 있으면 그것, 없으면 SVG 폴백
                    ICON_DST_DIR="$HOME/.local/share/icons/hicolor/128x128/apps"
                    mkdir -p "$ICON_DST_DIR"
                    if [ -f /usr/share/icons/hicolor/128x128/apps/kakaotalk.png ]; then
                        cp -n /usr/share/icons/hicolor/128x128/apps/kakaotalk.png "$ICON_DST_DIR/" 2>/dev/null || true
                    elif [ ! -f "$ICON_DST_DIR/kakaotalk.png" ]; then
                        SVG_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"
                        mkdir -p "$SVG_DIR"
                        cat > "$SVG_DIR/kakaotalk.svg" <<'SVG_EOF'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256"><rect width="256" height="256" rx="48" fill="#FEE500"/><path d="M128 56c-44 0-80 28-80 64 0 22 14 41 35 52l-9 32 36-22c6 1 12 2 18 2 44 0 80-28 80-64s-36-64-80-64z" fill="#3C1E1E"/></svg>
SVG_EOF
                    fi

                    echo
                    echo "=== [8/8] 데스크톱/아이콘 캐시 갱신 ==="
                    update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
                    gtk-update-icon-cache "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

                    echo
                    echo "✓ KakaoTalk 검증 환경 설치 완료"
                    echo "  → 앱 메뉴/독에서 카오톡 클릭 → 정상 실행 + 한글 입력 안정"
                    echo "  → 검은/흰 대화창 없음 (builtin d3d DLL 적용)"
                    echo "  → 한 번 띄운 후 다시 클릭하면 윈도우 활성화 (트레이 없어도 OK)"
                "##;
                let _unused = r##"
                    set -e
                    LAUNCHER="$HOME/.local/bin/kakaotalk"
                    if [ -f "$LAUNCHER" ]; then
                        echo "=== [1/4] kakaotalk-wine 이미 설치됨 — 건너뜀 (보정만 적용) ==="
                    else
                        echo "=== [1/4] kakaotalk-wine 설치 ==="
                        TMP=$(mktemp -d)
                        git clone --depth 1 https://github.com/eondcom/kakaotalk-wine "$TMP/repo" 2>&1
                        bash "$TMP/repo/install.sh" 2>&1
                        rm -rf "$TMP"
                    fi

                    echo
                    echo "=== [2/4] StartupWMClass 보정 (독 아이콘 매칭) ==="
                    DESK="$HOME/.local/share/applications/kakaotalk.desktop"
                    if [ -f "$DESK" ]; then
                        if ! grep -q '^StartupWMClass=' "$DESK"; then
                            printf '\nStartupWMClass=kakaotalk.exe\n' >> "$DESK"
                            echo "StartupWMClass=kakaotalk.exe 추가"
                        else
                            echo "StartupWMClass 이미 존재"
                        fi
                    else
                        echo "데스크톱 파일 없음(스킵): $DESK"
                    fi

                    echo
                    echo "=== [3/4] 아이콘 테마 등록 ==="
                    LAUNCHER="$HOME/.local/bin/kakaotalk"
                    EXE=""
                    if [ -f "$LAUNCHER" ]; then
                        EXE="$(grep -oE 'KAKAO_EXE=\"[^\"]+\"' "$LAUNCHER" | head -1 | sed 's/^KAKAO_EXE=\"//;s/\"$//')"
                        EXE="$(eval echo "$EXE")"
                    fi
                    ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
                    mkdir -p "$ICON_DIR"
                    OK=0
                    if [ -n "$EXE" ] && [ -f "$EXE" ] && command -v wrestool >/dev/null && command -v icotool >/dev/null; then
                        TMP2="$(mktemp -d)"
                        wrestool -x -t 14 "$EXE" -o "$TMP2" 2>/dev/null || true
                        BEST="$(ls "$TMP2"/*.ico 2>/dev/null | head -1)"
                        if [ -n "$BEST" ]; then
                            icotool -x "$BEST" -o "$TMP2" 2>/dev/null || true
                            BIGGEST="$(ls -S "$TMP2"/*.png 2>/dev/null | head -1)"
                            [ -n "$BIGGEST" ] && cp -f "$BIGGEST" "$ICON_DIR/kakaotalk.png" && OK=1
                        fi
                        rm -rf "$TMP2"
                    fi
                    if [ "$OK" = "1" ]; then
                        echo "아이콘 추출 완료: $ICON_DIR/kakaotalk.png"
                    else
                        SVG_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"
                        mkdir -p "$SVG_DIR"
                        cat > "$SVG_DIR/kakaotalk.svg" <<'SVG'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
  <rect width="256" height="256" rx="48" fill="#FEE500"/>
  <path d="M128 56c-44 0-80 28-80 64 0 22 14 41 35 52l-9 32 36-22c6 1 12 2 18 2 44 0 80-28 80-64s-36-64-80-64z" fill="#3C1E1E"/>
</svg>
SVG
                        echo "icoutils 없음 — SVG 폴백 사용 (정확한 아이콘 원하면 'sudo apt install icoutils' 후 보정)"
                    fi
                    [ -f "$DESK" ] && sed -i 's|^Icon=.*$|Icon=kakaotalk|' "$DESK"
                    gtk-update-icon-cache "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
                    update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true

                    echo
                    echo "=== [4/4] 한글 입력 안정화 (XIM ready 폴링) ==="
                    if [ -f "$LAUNCHER" ] && ! grep -q '# popmgr-ime-fix-v1' "$LAUNCHER"; then
                        cp -f "$LAUNCHER" "$LAUNCHER.bak"
                        cat > "$LAUNCHER" <<'EOF'
#!/bin/bash
# popmgr-ime-fix-v1
WIN32_PREFIX="/home/dell/.var/app/com.usebottles.bottles/data/bottles/bottles/KakaoTalk32"
RUNNER="/home/dell/.var/app/com.usebottles.bottles/data/bottles/runners/wine-11.10-staging-amd64"
KAKAO_EXE="$WIN32_PREFIX/drive_c/Program Files/Kakao/KakaoTalk/KakaoTalk.exe"
[ -z "$DISPLAY" ] && export DISPLAY=:1

# 좀비 카카오톡 정리 (single-instance 충돌 방지)
pkill -9 -f "KakaoTalk\.exe" 2>/dev/null
pkill -9 -f "winedbg" 2>/dev/null
for pid in $(pgrep -f wineserver 2>/dev/null); do
    [ -r "/proc/$pid/environ" ] && grep -qz "KakaoTalk32" "/proc/$pid/environ" 2>/dev/null && kill -9 "$pid" 2>/dev/null
done
sleep 0.3

cleanup() {
    pkill -f "KakaoTalk.exe" 2>/dev/null
    exit 0
}
trap cleanup SIGTERM SIGINT

# ibus 살아있고 XIM 등록돼 있으면 그대로, 죽었으면 시작
need_ibus_restart=0
pgrep -x ibus-daemon >/dev/null 2>&1 || need_ibus_restart=1
xprop -root XIM_SERVERS 2>/dev/null | grep -qi "ibus" || need_ibus_restart=1
[ "$need_ibus_restart" = "1" ] && ibus-daemon -dxr >/dev/null 2>&1 &

# XIM 서버 ready 폴링 (최대 3초)
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
    xprop -root XIM_SERVERS 2>/dev/null | grep -qi "ibus" && break
    sleep 0.2
done

xsetroot -cursor_name left_ptr 2>/dev/null

flatpak run \
    --env=DISPLAY="$DISPLAY" \
    --env=XMODIFIERS=@im=ibus \
    --env=QT_IM_MODULE=ibus \
    --env=GTK_IM_MODULE=ibus \
    --env=LANG=ko_KR.UTF-8 \
    --env=LC_ALL=ko_KR.UTF-8 \
    --command=bash com.usebottles.bottles -c "
export WINEPREFIX='$WIN32_PREFIX'
export WINEARCH=win32
xsetroot -cursor_name left_ptr 2>/dev/null
exec '$RUNNER/bin/wine' '$KAKAO_EXE' 2>/dev/null
"
EOF
                        chmod +x "$LAUNCHER"
                        echo "한글 입력 안정화 패치 적용 (백업: $LAUNCHER.bak)"
                    else
                        echo "IME 패치 이미 적용됨"
                    fi

                    echo
                    echo "✓ KakaoTalk 설치 + 모든 보정 완료"
                    echo "  → 앱 메뉴/독에서 카카오톡 아이콘으로 실행하세요"
                    echo "  → 독 즐겨찾기 추가 시 빈 아이콘이면 한 번 빼고 다시 추가"
                "##;
                let _ = _unused;
                let t = Task::perform(async move { runner::run_stream(script_active).await }, AppsMsg::Done);
                (t, None)
            }
            AppsMsg::LaunchKakaotalk => {
                // 1) 좀비 KakaoTalk.exe/wineserver 사전 청소 — 이전 인스턴스가 살아있으면
                //    새 인스턴스가 single-instance 충돌로 winedbg crash됨 ("실행했는데 안 뜸")
                // 2) setsid + nohup으로 popmgr 세션과 완전 분리
                let script = r#"
                    # 좀비 카카오톡 잔여물 정리 (UI 없이 뒤에 살아있는 경우 차단)
                    pkill -9 -f "KakaoTalk\\.exe" 2>/dev/null
                    pkill -9 -f "winedbg" 2>/dev/null
                    # 같은 prefix의 wineserver만 정리 (KakaoTalk32 prefix)
                    for pid in $(pgrep -f "wineserver"); do
                        envdir="/proc/$pid/environ"
                        [ -r "$envdir" ] && grep -qz "KakaoTalk32" "$envdir" 2>/dev/null && kill -9 "$pid" 2>/dev/null
                    done
                    sleep 0.3
                    # 분리 실행
                    setsid -f nohup kakaotalk </dev/null >/dev/null 2>&1 \
                        || (nohup kakaotalk </dev/null >/dev/null 2>&1 & disown)
                    echo "카카오톡 실행 요청 완료 (이전 인스턴스 청소 + 새 인스턴스 분리 시작)"
                "#;
                let t = Task::perform(
                    async move { runner::run_sh(script).await },
                    AppsMsg::Done,
                );
                (t, None)
            }
            AppsMsg::FixKakaotalkDesktop => {
                self.running = Some("바로가기/독 아이콘 보정 중...".into());
                let script = r#"
                    set -e
                    DESK="$HOME/.local/share/applications/kakaotalk.desktop"
                    if [ ! -f "$DESK" ]; then
                        echo "데스크톱 파일이 없습니다: $DESK"
                        exit 1
                    fi
                    if ! grep -q '^StartupWMClass=' "$DESK"; then
                        printf '\nStartupWMClass=kakaotalk.exe\n' >> "$DESK"
                        echo "StartupWMClass=kakaotalk.exe 추가"
                    else
                        echo "StartupWMClass 이미 존재"
                    fi
                    update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
                    echo "독 아이콘 매칭 보정 완료"
                "#;
                let t = Task::perform(async move { runner::run_sh(script).await }, AppsMsg::Done);
                (t, None)
            }
            AppsMsg::FixKakaotalkIcon => {
                self.running = Some("아이콘 추출/설치 중...".into());
                // KakaoTalk.exe에서 아이콘 추출(wrestool/icoutils) → hicolor 테마에 설치
                // 실패 시 폴백으로 임베디드 SVG 사용(노란 말풍선) — 즐겨찾기 빈 칸 방지
                let script = r##"
                    set -e
                    LAUNCHER="$(command -v kakaotalk 2>/dev/null || true)"
                    [ -z "$LAUNCHER" ] && LAUNCHER="$HOME/.local/bin/kakaotalk"
                    EXE="$(grep -oE 'KAKAO_EXE=\"[^\"]+\"' "$LAUNCHER" | head -1 | sed 's/^KAKAO_EXE=\"//;s/\"$//')"
                    EXE="$(eval echo "$EXE")"
                    ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
                    mkdir -p "$ICON_DIR"
                    DST_PNG="$ICON_DIR/kakaotalk.png"
                    OK=0
                    if [ -f "$EXE" ] && command -v wrestool >/dev/null && command -v icotool >/dev/null; then
                        TMP="$(mktemp -d)"
                        wrestool -x -t 14 "$EXE" -o "$TMP" 2>/dev/null || true
                        BEST="$(ls "$TMP"/*.ico 2>/dev/null | head -1)"
                        if [ -n "$BEST" ]; then
                            icotool -x -i 1 "$BEST" -o "$TMP" 2>/dev/null || \
                                icotool -x "$BEST" -o "$TMP" 2>/dev/null || true
                            BIGGEST="$(ls -S "$TMP"/*.png 2>/dev/null | head -1)"
                            if [ -n "$BIGGEST" ]; then
                                cp -f "$BIGGEST" "$DST_PNG"
                                OK=1
                                echo "아이콘 추출 성공: $DST_PNG"
                            fi
                        fi
                        rm -rf "$TMP"
                    fi
                    if [ "$OK" -eq 0 ]; then
                        # 폴백: SVG 임베디드 (노란 말풍선)
                        SVG_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"
                        mkdir -p "$SVG_DIR"
                        cat > "$SVG_DIR/kakaotalk.svg" <<'SVG'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
  <rect width="256" height="256" rx="48" fill="#FEE500"/>
  <path d="M128 56c-44 0-80 28-80 64 0 22 14 41 35 52l-9 32 36-22c6 1 12 2 18 2 44 0 80-28 80-64s-36-64-80-64z" fill="#3C1E1E"/>
</svg>
SVG
                        echo "아이콘 추출 도구 없음(icoutils) — SVG 폴백 설치"
                        echo "정확한 카카오 아이콘을 원하면: sudo apt install icoutils 후 다시 보정"
                    fi
                    # 데스크톱 파일 Icon= 라인이 절대경로일 수 있어 'kakaotalk'으로 정규화
                    DESK="$HOME/.local/share/applications/kakaotalk.desktop"
                    if [ -f "$DESK" ] && ! grep -q '^Icon=kakaotalk$' "$DESK"; then
                        sed -i 's|^Icon=.*$|Icon=kakaotalk|' "$DESK"
                    fi
                    # 캐시 갱신
                    gtk-update-icon-cache "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
                    update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
                    echo "독 즐겨찾기/런처 아이콘 설치 완료 — 독을 재시작하거나 즐겨찾기 다시 추가하세요"
                "##;
                let t = Task::perform(async move { runner::run_sh(script).await }, AppsMsg::Done);
                (t, None)
            }
            AppsMsg::FixKakaotalkIme => {
                self.running = Some("한글 입력 안정화 패치 적용 중...".into());
                // 런처에 ibus XIM ready 폴링 + env 전달 패치 적용
                // 1) ibus가 실행 중이면 -r(replace) 안 함 (다른 앱 입력 깨짐 방지)
                // 2) ibus 죽은 경우만 시작, XIM_SERVERS atom 보일 때까지 폴링(최대 3초)
                // 3) flatpak 내부에 LANG/IM 환경변수 명시적 전달
                let script = r#"
                    set -e
                    LAUNCHER="$HOME/.local/bin/kakaotalk"
                    [ -f "$LAUNCHER" ] || { echo "런처 없음: $LAUNCHER"; exit 1; }

                    # 이미 패치돼 있으면 스킵
                    if grep -q '# popmgr-ime-fix-v1' "$LAUNCHER"; then
                        echo "이미 패치돼 있습니다 (popmgr-ime-fix-v1)"
                        exit 0
                    fi

                    cp -f "$LAUNCHER" "$LAUNCHER.bak"
                    cat > "$LAUNCHER" <<'EOF'
#!/bin/bash
# popmgr-ime-fix-v1
WIN32_PREFIX="/home/dell/.var/app/com.usebottles.bottles/data/bottles/bottles/KakaoTalk32"
RUNNER="/home/dell/.var/app/com.usebottles.bottles/data/bottles/runners/wine-11.10-staging-amd64"
KAKAO_EXE="$WIN32_PREFIX/drive_c/Program Files/Kakao/KakaoTalk/KakaoTalk.exe"

# DISPLAY 보정 — 세션 환경에 따라 :0 또는 :1
[ -z "$DISPLAY" ] && export DISPLAY=:1

cleanup() {
    pkill -f "KakaoTalk.exe" 2>/dev/null
    flatpak run --command=bash com.usebottles.bottles -c "
    export WINEPREFIX='$WIN32_PREFIX'
    '$RUNNER/bin/wineserver' -k 2>/dev/null
    " &>/dev/null
    exit 0
}
trap cleanup SIGTERM SIGINT

# === 한글 입력 안정화 ===
# ibus-daemon이 살아있고 XIM_SERVERS에 ibus가 등록돼 있으면 건드리지 않음
need_ibus_restart=0
pgrep -x ibus-daemon >/dev/null 2>&1 || need_ibus_restart=1
xprop -root XIM_SERVERS 2>/dev/null | grep -qi "ibus" || need_ibus_restart=1

if [ "$need_ibus_restart" = "1" ]; then
    ibus-daemon -dxr >/dev/null 2>&1 &
fi

# XIM 서버가 X atom에 노출될 때까지 최대 3초 폴링 (sleep 0.5 한 번보다 안정적)
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
    xprop -root XIM_SERVERS 2>/dev/null | grep -qi "ibus" && break
    sleep 0.2
done

xsetroot -cursor_name left_ptr 2>/dev/null

# flatpak 샌드박스로 env 명시 전달 (--env 사용)
flatpak run \
    --env=DISPLAY="$DISPLAY" \
    --env=XMODIFIERS=@im=ibus \
    --env=QT_IM_MODULE=ibus \
    --env=GTK_IM_MODULE=ibus \
    --env=LANG=ko_KR.UTF-8 \
    --env=LC_ALL=ko_KR.UTF-8 \
    --command=bash com.usebottles.bottles -c "
export WINEPREFIX='$WIN32_PREFIX'
export WINEARCH=win32
xsetroot -cursor_name left_ptr 2>/dev/null
'$RUNNER/bin/wine' '$KAKAO_EXE' 2>/dev/null
"
EOF
                    chmod +x "$LAUNCHER"
                    echo "런처 패치 완료: $LAUNCHER"
                    echo "백업: $LAUNCHER.bak"
                "#;
                let t = Task::perform(async move { runner::run_sh(script).await }, AppsMsg::Done);
                (t, None)
            }
            AppsMsg::Done(r) => {
                self.running = None;
                let refresh = Task::perform(async { scan_apps().await }, AppsMsg::Refreshed);
                (refresh, Some(r))
            }
        }
    }

    pub fn view(&self) -> Element<'_, AppsMsg> {
        let is_running = self.running.is_some();
        let mut col = column![
            text("앱 관리").size(20),
            Space::with_height(16),
        ];

        if let Some(label) = &self.running {
            col = col.push(running_bar(label)).push(Space::with_height(12));
        }

        // KakaoTalk 카드
        col = col.push(kakaotalk_card(self.status.as_ref(), is_running));
        col = col.push(Space::with_height(20));

        // 프로그램 제거 섹션
        col = col.push(text("프로그램 제거").size(16));
        col = col.push(Space::with_height(8));
        col = col.push(
            text_input("이름으로 검색...", &self.search)
                .on_input(AppsMsg::SearchChanged)
                .padding([8, 10])
                .size(13)
        );
        col = col.push(Space::with_height(8));

        if let Some(st) = &self.status {
            let q = self.search.to_lowercase();
            let filtered: Vec<(usize, &Package)> = st.packages.iter().enumerate()
                .filter(|(_, p)| {
                    q.is_empty()
                        || p.name.to_lowercase().contains(&q)
                        || p.description.to_lowercase().contains(&q)
                })
                .collect();

            if filtered.is_empty() {
                col = col.push(text("검색 결과 없음").size(13).color(C_DIM));
            } else {
                let list = filtered.iter().fold(
                    column![].spacing(4),
                    |c, (i, pkg)| c.push(pkg_row(*i, pkg, is_running)),
                );
                col = col.push(scrollable(list).height(300));
            }

            let marked_count = st.packages.iter().filter(|p| p.marked).count();
            col = col.push(Space::with_height(12));
            let remove_label = format!("선택 항목 제거 ({marked_count})");
            col = col.push(
                row![
                    text(format!("{marked_count}개 선택됨")).size(12).color(C_DIM),
                    Space::with_width(Length::Fill),
                    action_btn("새로고침", AppsMsg::Refresh, !is_running, Color::from_rgb(0.25, 0.25, 0.35)),
                    Space::with_width(8),
                    action_btn(
                        remove_label,
                        AppsMsg::RemoveMarked,
                        !is_running && marked_count > 0,
                        Color::from_rgb(0.75, 0.15, 0.15),
                    ),
                ]
                .align_y(iced::Alignment::Center)
            );
        } else {
            col = col.push(text("스캔 중...").size(13).color(C_DIM));
        }

        scrollable(container(col).padding([4, 0])).into()
    }
}

fn kakaotalk_card(status: Option<&AppsStatus>, disabled: bool) -> Element<'static, AppsMsg> {
    let installed = status.map(|s| s.kakaotalk_installed).unwrap_or(false);
    let status_txt = if installed { "✓ 설치됨" } else { "✗ 미설치" };
    let status_col = if installed { C_OK } else { C_DIM };

    let launcher = status.and_then(|s| s.kakaotalk_launcher.clone()).unwrap_or_default();
    let exe = status.and_then(|s| s.kakaotalk_exe.clone()).unwrap_or_default();
    let desktop = status.and_then(|s| s.kakaotalk_desktop.clone()).unwrap_or_default();
    let wmclass_ok = status.map(|s| s.kakaotalk_wmclass_ok).unwrap_or(false);
    let icon_ok = status.map(|s| s.kakaotalk_icon_ok).unwrap_or(false);
    let ime_patched = status.map(|s| s.kakaotalk_ime_patched).unwrap_or(false);

    let mut left = column![
        text("KakaoTalk (Wine)").size(14).color(Color::from_rgb(0.9, 0.9, 0.95)),
        Space::with_height(3),
        text("eondcom/kakaotalk-wine — Wine 기반 카카오톡 Linux 설치").size(11).color(C_DIM),
        Space::with_height(4),
        text(status_txt).size(12).color(status_col),
    ];

    if installed {
        if !launcher.is_empty() {
            left = left.push(Space::with_height(2));
            left = left.push(text(format!("실행 스크립트: {launcher}")).size(11).color(C_DIM));
        }
        if !exe.is_empty() {
            left = left.push(text(format!("KakaoTalk.exe: {exe}")).size(11).color(C_DIM));
        }
        if !desktop.is_empty() {
            let wm_state = if wmclass_ok { "(WMClass OK)" } else { "(WMClass 없음 — 독 아이콘 매칭 불가)" };
            let col_ = if wmclass_ok { C_DIM } else { C_WARN };
            left = left.push(text(format!("바로가기: {desktop} {wm_state}")).size(11).color(col_));
        }
        let icon_state = if icon_ok { "✓ 아이콘 테마 등록됨" } else { "✗ 아이콘 미등록 — 독 즐겨찾기 빈 칸" };
        let icon_c = if icon_ok { C_DIM } else { C_WARN };
        left = left.push(text(icon_state).size(11).color(icon_c));
        let ime_state = if ime_patched { "✓ 한글 입력 안정화 적용됨 (popmgr-ime-fix-v1)" } else { "⚠ 한글 입력 가끔 안 됨 — IME 안정화 미적용" };
        let ime_c = if ime_patched { C_DIM } else { C_WARN };
        left = left.push(text(ime_state).size(11).color(ime_c));
    }

    let mut right = column![].spacing(6).align_x(iced::Alignment::End);
    // 실행은 OS 앱 메뉴/독에서 — popmgr는 설치(보정 일괄)만 담당
    // 한 버튼으로 통합: 미설치면 풀 설치, 부분 설치면 빠진 항목 보정, 다 OK면 재적용
    let all_ok = installed && wmclass_ok && icon_ok && ime_patched;
    let label = if !installed {
        "카카오톡 설치"
    } else if !all_ok {
        "카카오톡 설치 (보정)"
    } else {
        "카카오톡 재설치"
    };
    right = right.push(action_btn(label, AppsMsg::InstallKakaotalk, !disabled, C_OK));
    if all_ok {
        right = right.push(text("✓ 모든 설정 완료").size(11).color(C_OK));
    }

    card(
        row![
            left.width(Length::Fill),
            right,
        ]
        .align_y(iced::Alignment::Center)
    )
}

fn pkg_row(idx: usize, pkg: &Package, disabled: bool) -> Element<'_, AppsMsg> {
    let bg = if pkg.marked { Color::from_rgb(0.14, 0.05, 0.05) } else { Color::from_rgb(0.1, 0.1, 0.13) };
    let border = if pkg.marked { C_ERR } else { Color::from_rgb(0.2, 0.2, 0.25) };
    let kind_txt = match pkg.kind { PkgKind::Apt => "APT", PkgKind::Flatpak => "Flatpak" };
    let kind_col = match pkg.kind { PkgKind::Apt => Color::from_rgb(0.3, 0.6, 0.9), PkgKind::Flatpak => Color::from_rgb(0.6, 0.4, 0.9) };

    let check_bg = if pkg.marked { C_ERR } else { Color::from_rgb(0.15, 0.15, 0.18) };
    let check_txt = if pkg.marked { "✓" } else { " " };

    let checkbox = container(
        text(check_txt).size(12).color(Color::WHITE)
    )
    .width(20).height(20)
    .style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(check_bg)),
        border: iced::Border { radius: 4.0.into(), color: Color::from_rgb(0.35, 0.35, 0.4), width: 1.5 },
        ..Default::default()
    });

    let row_inner = row![
        checkbox,
        Space::with_width(10),
        column![
            row![
                text(&pkg.name).size(13).color(Color::from_rgb(0.85, 0.85, 0.9)),
                Space::with_width(8),
                text(kind_txt).size(10).color(kind_col),
                Space::with_width(8),
                text(&pkg.version).size(10).color(C_DIM),
            ].align_y(iced::Alignment::Center),
            text(&pkg.description).size(11).color(C_DIM),
        ].width(Length::Fill),
    ]
    .align_y(iced::Alignment::Center);

    iced::widget::button(
        container(row_inner).padding([8, 12]).width(Length::Fill)
    )
    .width(Length::Fill)
    .on_press_maybe(if !disabled { Some(AppsMsg::TogglePkg(idx)) } else { None })
    .style(move |_, _| iced::widget::button::Style {
        background: Some(iced::Background::Color(bg)),
        border: iced::Border { radius: 7.0.into(), color: border, width: 1.0 },
        text_color: Color::WHITE,
        ..Default::default()
    })
    .into()
}

async fn scan_apps() -> AppsStatus {
    let mut packages = Vec::new();

    // APT: 직접 설치된 것만 (not auto)
    let apt = runner::run("bash", &["-c",
        "apt-mark showmanual 2>/dev/null | head -200"
    ]).await;
    for name in apt.output.lines() {
        let name = name.trim();
        if name.is_empty() { continue; }
        let info = runner::run("bash", &["-c",
            &format!("dpkg -l '{name}' 2>/dev/null | grep '^ii' | head -1")
        ]).await;
        let parts: Vec<&str> = info.output.split_whitespace().collect();
        if parts.len() < 5 { continue; }
        let version = parts[2].to_string();
        let description = parts[4..].join(" ");
        packages.push(Package { name: name.to_string(), version, description, kind: PkgKind::Apt, marked: false });
    }

    // Flatpak
    let flat = runner::run("bash", &["-c",
        "flatpak list --app --columns=application,version,name 2>/dev/null"
    ]).await;
    for line in flat.output.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 2 { continue; }
        let name = parts[0].trim().to_string();
        let version = parts[1].trim().to_string();
        let description = parts.get(2).unwrap_or(&"").trim().to_string();
        packages.push(Package { name, version, description, kind: PkgKind::Flatpak, marked: false });
    }

    // KakaoTalk 정보 수집
    // 1) 런처 스크립트 위치 (PATH 우선, 없으면 ~/.local/bin / /usr/local/bin 직접 확인)
    let launcher_lookup = runner::run("bash", &["-c",
        "command -v kakaotalk 2>/dev/null \
         || ls $HOME/.local/bin/kakaotalk 2>/dev/null \
         || ls /usr/local/bin/kakaotalk 2>/dev/null \
         || true"
    ]).await;
    let kakaotalk_launcher = launcher_lookup.output.lines().next()
        .map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    // 2) KakaoTalk.exe 실제 위치 (런처 스크립트에서 KAKAO_EXE 추출)
    let exe_lookup = runner::run("bash", &["-c",
        "for L in $(command -v kakaotalk) $HOME/.local/bin/kakaotalk /usr/local/bin/kakaotalk; do \
            [ -f \"$L\" ] || continue; \
            EXE=$(grep -oE 'KAKAO_EXE=\"[^\"]+\"' \"$L\" | head -1 | sed 's/^KAKAO_EXE=\"//;s/\"$//'); \
            EXE=$(eval echo \"$EXE\"); \
            if [ -n \"$EXE\" ] && [ -f \"$EXE\" ]; then echo \"$EXE\"; break; fi; \
            if [ -n \"$EXE\" ]; then echo \"$EXE (없음)\"; break; fi; \
         done"
    ]).await;
    let kakaotalk_exe = exe_lookup.output.lines().next()
        .map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    // 3) .desktop 파일과 StartupWMClass 존재 여부
    let desk_lookup = runner::run("bash", &["-c",
        "for D in $HOME/.local/share/applications/kakaotalk.desktop /usr/share/applications/kakaotalk.desktop; do \
            if [ -f \"$D\" ]; then echo \"$D\"; break; fi; \
         done"
    ]).await;
    let kakaotalk_desktop = desk_lookup.output.lines().next()
        .map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    let kakaotalk_wmclass_ok = if let Some(d) = &kakaotalk_desktop {
        runner::run("bash", &["-c",
            &format!("grep -q '^StartupWMClass=' '{d}'")
        ]).await.success
    } else { false };

    // 아이콘 테마에 kakaotalk 아이콘이 등록돼 있나?
    let kakaotalk_icon_ok = runner::run("bash", &["-c",
        "ls $HOME/.local/share/icons/hicolor/*/apps/kakaotalk.* \
            /usr/share/icons/hicolor/*/apps/kakaotalk.* \
            $HOME/.local/share/icons/kakaotalk.* 2>/dev/null | head -1"
    ]).await.output.lines().any(|s| !s.trim().is_empty());

    // IME 안정화 패치 적용 여부
    let kakaotalk_ime_patched = if let Some(l) = &kakaotalk_launcher {
        runner::run("bash", &["-c",
            &format!("grep -q 'popmgr-ime-fix-v1' '{l}'")
        ]).await.success
    } else { false };

    let kakaotalk_installed = kakaotalk_launcher.is_some()
        || std::path::Path::new("/opt/kakaotalk/kakaotalk.exe").exists();

    AppsStatus {
        kakaotalk_installed,
        kakaotalk_launcher,
        kakaotalk_exe,
        kakaotalk_desktop,
        kakaotalk_wmclass_ok,
        kakaotalk_icon_ok,
        kakaotalk_ime_patched,
        packages,
    }
}
