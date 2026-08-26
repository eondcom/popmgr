use iced::{
    widget::{column, container, row, scrollable, text, text_input, Space},
    Color, Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_BORDER, C_BTN2, C_DIM, C_ERR, C_OK, C_SURFACE, C_SURFACE2, C_TEXT, C_WARN};

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
    pub orca_deb_installed: bool,
    pub orca_appimage: Option<String>,
    pub orca_version: Option<String>,
    pub orca_desktop: Option<String>,
    pub orca_icon_ok: bool,
    pub orca_dock_ok: bool,
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
    ShowKakaotalk,
    QuitKakaotalk,
    ForceKillKakaotalk,
    FixKakaotalkDesktop,
    FixKakaotalkIcon,
    FixKakaotalkIme,
    InstallOrca,
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
            AppsMsg::InstallOrca => {
                self.running = Some("Orca 런처·독 등록 중...".into());
                // 새로 깐 리눅스에서 이 버튼 하나로 끝나야 한다.
                // 1순위는 공식 .deb (stablyai/orca GitHub 릴리스). dpkg 가 런처·아이콘을
                // 알아서 등록하고 이후 업데이트·제거도 apt 로 된다.
                // 네트워크가 막혔거나 이미 AppImage 를 받아둔 경우를 위해 AppImage 경로도 남긴다.
                let script = r#"
set -u
APPDIR="$HOME/Applications"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor"
FAV="$HOME/.config/cosmic/com.system76.CosmicAppList/v1/favorites"
REL="https://github.com/stablyai/orca/releases"
DESK_LOCAL="$APPS/orca-ide.desktop"
CACHE="$HOME/.cache/popmgr"

DEB_INSTALLED=0
if dpkg -s orca-ide >/dev/null 2>&1; then
    DEB_INSTALLED=1
    echo "=== orca-ide 패키지 설치됨 ($(dpkg-query -W -f='${Version}' orca-ide 2>/dev/null)) ==="
fi

if [ "$DEB_INSTALLED" = "0" ]; then
    echo "=== [deb 1/3] 최신 릴리스 확인 ==="
    # latest-linux.yml 에 버전과 sha512 가 함께 들어 있어 URL 을 하드코딩하지 않아도 된다.
    YML=$(curl -fsSL --max-time 60 "$REL/latest/download/latest-linux.yml" 2>/dev/null || true)
    VER=$(printf '%s\n' "$YML" | grep '^version:' | head -1 | awk '{print $2}' | tr -d '\r')
    if [ -n "$VER" ]; then
        echo "최신 버전: $VER"
        DEBFILE="orca-ide_${VER}_amd64.deb"
        mkdir -p "$CACHE"
        echo "=== [deb 2/3] 다운로드 ($DEBFILE, 약 155MB) ==="
        if curl -fL --max-time 1800 -o "$CACHE/$DEBFILE" "$REL/download/v${VER}/${DEBFILE}"; then
            SUM_B64=$(printf '%s\n' "$YML" | grep -A1 "url: $DEBFILE" | grep 'sha512:' \
                      | head -1 | awk '{print $2}' | tr -d '\r')
            if [ -n "$SUM_B64" ]; then
                WANT=$(printf '%s' "$SUM_B64" | base64 -d 2>/dev/null | xxd -p -c 999)
                GOT=$(sha512sum "$CACHE/$DEBFILE" | awk '{print $1}')
                if [ "$WANT" = "$GOT" ]; then
                    echo "sha512 검증 통과"
                else
                    echo "★ sha512 불일치 — 받은 파일을 버립니다"
                    rm -f "$CACHE/$DEBFILE"
                fi
            fi
            if [ -f "$CACHE/$DEBFILE" ]; then
                echo "=== [deb 3/3] 설치 (관리자 비밀번호 창이 뜹니다) ==="
                if pkexec apt-get install -y "$CACHE/$DEBFILE"; then
                    rm -f "$CACHE/$DEBFILE"
                    dpkg -s orca-ide >/dev/null 2>&1 && DEB_INSTALLED=1
                else
                    echo "설치가 취소되었거나 실패했습니다 — AppImage 방식으로 넘어갑니다"
                fi
            fi
        else
            echo "다운로드 실패 — AppImage 방식으로 넘어갑니다"
        fi
    else
        echo "릴리스 정보를 가져오지 못했습니다 — AppImage 방식으로 넘어갑니다"
    fi
fi

if [ "$DEB_INSTALLED" = "1" ]; then
    # dpkg 가 /usr/share/applications/orca-ide.desktop 과 아이콘을 이미 설치했다.
    # 같은 desktop ID 가 시스템과 홈 양쪽에 있으면 COSMIC 런처에 앱이 두 번 뜬다
    # (COSMIC 은 중복 제거를 하지 않는다). 그래서 홈 쪽 사본을 지운다.
    if [ -f "$DESK_LOCAL" ]; then
        rm -f "$DESK_LOCAL"
        echo "로컬 중복 바로가기 제거: $DESK_LOCAL (시스템 패키지 것을 사용)"
    fi
    update-desktop-database "$APPS" 2>/dev/null || true
else

echo "=== [1/6] Orca AppImage 찾기 ==="
FOUND=""
for C in "$APPDIR"/orca*.AppImage "$APPDIR"/Orca*.AppImage \
         "$HOME/Downloads"/orca*.AppImage "$HOME/Downloads"/Orca*.AppImage \
         "$HOME/.local/bin"/orca*.AppImage "$HOME"/orca*.AppImage; do
    [ -f "$C" ] || continue
    FOUND="$C"; break
done
if [ -z "$FOUND" ]; then
    echo "설치할 것을 찾지 못했습니다."
    echo "  - 자동 설치(.deb)가 실패했고, 받아둔 AppImage 도 없습니다."
    echo "  - 인터넷 연결을 확인한 뒤 다시 눌러보세요."
    echo "  - 수동으로 받으려면: $REL/latest"
    echo "    (orca-ide_*_amd64.deb 를 받아 더블클릭하거나,"
    echo "     orca-linux.AppImage 를 ~/Downloads 에 두고 다시 누르세요)"
    exit 1
fi
echo "찾음: $FOUND"

echo "=== [2/6] ~/Applications 로 정리 ==="
mkdir -p "$APPDIR"
TARGET="$APPDIR/$(basename "$FOUND")"
if [ "$FOUND" != "$TARGET" ]; then
    mv -f "$FOUND" "$TARGET" && echo "이동: $TARGET"
else
    echo "이미 제자리: $TARGET"
fi
chmod +x "$TARGET"

echo "=== [3/6] 아이콘·버전 추출 ==="
TMP=$(mktemp -d)
# 내장 .desktop 을 먼저 뽑아 버전을 읽는다. AppImage 를 매번 다시 여는 것은 느리므로
# 여기서 읽은 값을 우리 .desktop 에 적어두고, 이후 스캔은 그 줄만 grep 한다.
( cd "$TMP" && "$TARGET" --appimage-extract "*.desktop" >/dev/null 2>&1 ) || true
VER=$(grep -h '^X-AppImage-Version=' "$TMP"/squashfs-root/*.desktop 2>/dev/null | head -1 | cut -d= -f2-)
[ -n "$VER" ] && echo "버전: $VER"
( cd "$TMP" && "$TARGET" --appimage-extract "usr/share/icons/*" >/dev/null 2>&1 ) || true
N=0
for SRC in "$TMP"/squashfs-root/usr/share/icons/hicolor/*/apps/orca-ide.png; do
    [ -f "$SRC" ] || continue
    DIM=$(basename "$(dirname "$(dirname "$SRC")")")
    mkdir -p "$ICONS/$DIM/apps"
    cp -L "$SRC" "$ICONS/$DIM/apps/orca-ide.png" && N=$((N+1))
done
rm -rf "$TMP"
echo "아이콘 ${N}개 설치"
[ "$N" -eq 0 ] && echo "경고: 아이콘 추출 실패 — 런처에 기본 아이콘으로 보일 수 있습니다"

echo "=== [4/6] 런처 등록 (.desktop) ==="
mkdir -p "$APPS"
cat > "$APPS/orca-ide.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Orca
GenericName=Agentic IDE
Comment=Next-gen IDE for parallel agentic development
Exec=$TARGET --no-sandbox %U
Icon=orca-ide
Terminal=false
StartupNotify=true
StartupWMClass=orca
Categories=Development;
Keywords=orca;ide;terminal;agent;coding;개발;터미널;
MimeType=x-scheme-handler/orca;
X-AppImage-Version=$VER
EOF
chmod 644 "$APPS/orca-ide.desktop"
update-desktop-database "$APPS" 2>/dev/null || true
gtk-update-icon-cache -f -t "$ICONS" 2>/dev/null || true
echo "등록: $APPS/orca-ide.desktop"

fi

echo "=== [5/6] 독(Dock) 즐겨찾기 등록 ==="
DOCK_CHANGED=0
if [ -f "$FAV" ]; then
    if grep -q '"orca-ide"' "$FAV"; then
        echo "이미 독에 등록됨"
    else
        cp "$FAV" "$FAV.popmgr-bak"
        sed -i 's/^\]$/    "orca-ide",\n]/' "$FAV"
        if grep -q '"orca-ide"' "$FAV"; then
            echo "독 즐겨찾기에 추가 (백업: $FAV.popmgr-bak)"
            DOCK_CHANGED=1
        else
            cp "$FAV.popmgr-bak" "$FAV"
            echo "경고: 독 설정 형식을 알 수 없어 건너뜀"
        fi
    fi
else
    echo "COSMIC 독 설정이 없어 건너뜀"
fi

echo "=== [6/6] 패널 반영 ==="
if [ "$DOCK_CHANGED" = "1" ] && pgrep -x cosmic-panel >/dev/null 2>&1; then
    # cosmic-app-list 만 kill 하면 cosmic-session 이 되살리지 않아 독이 빈 채로 남는다.
    # 반드시 cosmic-panel 을 재시작해야 applet 이 함께 복구된다.
    pkill -x cosmic-panel 2>/dev/null || true
    sleep 3
    if pgrep -x cosmic-panel >/dev/null 2>&1; then
        echo "패널 재시작 완료 — 독에 Orca 아이콘이 보입니다"
    else
        echo "경고: 패널이 자동 복구되지 않았습니다. 로그아웃 후 다시 로그인하세요."
    fi
else
    echo "패널 재시작 불필요"
fi

echo
if [ "$DEB_INSTALLED" = "1" ]; then
    echo "완료: apt 패키지(orca-ide)로 설치했습니다. 런처와 독에서 실행하세요."
    echo "업데이트·제거는 apt 로 하면 됩니다."
else
    echo "완료: AppImage 를 런처와 독에 등록했습니다."
fi
echo "참고: 터미널에서 'orca' 를 치면 GNOME 스크린리더가 실행됩니다 (이름 충돌)."
"#;
                let t = Task::perform(async move { runner::run_stream(script).await }, AppsMsg::Done);
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
                    echo "=== [5/9] d3d builtin DLL 복사 (대화창 흰/검 창 차단) ==="
                    SYS32="$PREFIX/drive_c/windows/system32"
                    WINE_DLLS="$RUNNER_DIR/lib/wine/i386-windows"
                    for dll in d3d9 d3d10 d3d10core d3d11 dxgi; do
                        if [ -f "$WINE_DLLS/${dll}.dll" ]; then
                            cp -n "$WINE_DLLS/${dll}.dll" "$SYS32/" 2>/dev/null && echo "복사: ${dll}.dll" || true
                        fi
                    done

                    echo
                    echo "=== [6/9] 한글 폰트 실파일 + 폰트 레지스트리 (한글 □ 깨짐 차단) ==="
                    # flatpak 샌드박스 안에서는 호스트의 /usr/share/fonts 가 보이지 않는다(/run/host/fonts 로 마운트됨).
                    # 그래서 Fonts 폴더에 심볼릭 링크를 넣으면 Wine 이 폰트를 못 읽어 한글이 전부 □ 로 깨진다.
                    # 반드시 실파일로 복사한다. (기존 링크도 실파일로 교체)
                    FONTDIR="$PREFIX/drive_c/windows/Fonts"
                    mkdir -p "$FONTDIR"
                    # 새로 깐 시스템에는 한글 폰트가 없을 수 있다(한국어 언어 지원을 추가해야 들어온다).
                    # 없으면 복사 단계가 통째로 건너뛰어져 한글이 다시 □ 로 깨지므로 먼저 설치한다.
                    if [ ! -f /usr/share/fonts/truetype/nanum/NanumGothic.ttf ]; then
                        echo "호스트에 나눔 폰트 없음 — 설치 시도 (fonts-nanum)"
                        pkexec sh -c 'apt-get install -y fonts-nanum fonts-noto-cjk 2>&1 | tail -3' \
                            || echo "! 자동 설치 실패 — 'sudo apt install fonts-nanum' 후 이 설치를 다시 실행하세요"
                        fc-cache -f >/dev/null 2>&1 || true
                    fi
                    for src in \
                        /usr/share/fonts/truetype/nanum/NanumGothic.ttf \
                        /usr/share/fonts/truetype/nanum/NanumGothicBold.ttf \
                        /usr/share/fonts/truetype/nanum/NanumMyeongjo.ttf \
                        /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc \
                        /usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc
                    do
                        [ -f "$src" ] || continue
                        dst="$FONTDIR/$(basename "$src")"
                        if [ -L "$dst" ] || [ ! -f "$dst" ]; then
                            rm -f "$dst"
                            cp "$src" "$dst" && echo "폰트 복사: $(basename "$src")"
                        fi
                    done
                    if [ ! -f "$FONTDIR/NanumGothic.ttf" ]; then
                        echo "! 나눔 폰트 없음 — 'sudo apt install fonts-nanum' 후 다시 실행하세요"
                    fi

                    # 폰트 이름 치환 — 파일 저장 대화상자 등 Wine 공용 UI 의 한글 □ 깨짐 차단.
                    #
                    # WindowMetrics(LOGFONT)는 실제로 필요한 MenuFont/IconFont 둘만 바꾼다.
                    #  - MenuFont: 메뉴·문맥메뉴 한글
                    #  - IconFont: 파일 대화상자의 파일/폴더 목록 한글
                    # 나머지 넷(CaptionFont/MessageFont/StatusFont/SmCaptionFont)은 이득이 적어
                    # (캡션은 WM 이 그리고, 메시지박스/상태바는 드물다) 건드리지 않는다.
                    #
                    # 참고: 카카오톡은 시작 시 c0000409 로 죽는 일이 간헐적으로 있는데,
                    # LOGFONT 적용 여부와 무관하다. 3회씩 측정해 원본 2/1, 적용 2/1 로 같았다.
                    # (강제 종료 직후 재시작할 때 잘 나며, 다시 실행하면 뜬다)
                    # LOGFONTW(92B): lfHeight=8, lfWeight=400, lfCharSet=DEFAULT, lfFaceName="NanumGothic"
                    LF8="080000000000000000000000000000009001000000000001000000004e0061006e0075006d0047006f007400680069006300000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
                    flatpak run --command=bash com.usebottles.bottles -c "
                        export WINEPREFIX='$PREFIX'
                        export WINEARCH=win32
                        export WINEDEBUG=-all
                        W='$RUNNER_DIR/bin/wine'
                        SUB='HKLM\\Software\\Microsoft\\Windows NT\\CurrentVersion\\FontSubstitutes'
                        for f in 'MS Shell Dlg' 'MS Shell Dlg 2' 'Tahoma' 'Segoe UI' 'Verdana' 'MS Sans Serif' 'Microsoft Sans Serif' 'Malgun Gothic' 'Gulim' 'GulimChe' 'Dotum' 'DotumChe'; do
                            \"\$W\" reg add \"\$SUB\" /v \"\$f\" /t REG_SZ /d NanumGothic /f >/dev/null 2>&1
                        done
                        for f in 'Batang' 'BatangChe' 'Gungsuh'; do
                            \"\$W\" reg add \"\$SUB\" /v \"\$f\" /t REG_SZ /d NanumMyeongjo /f >/dev/null 2>&1
                        done
                        WM='HKCU\\Control Panel\\Desktop\\WindowMetrics'
                        for v in MenuFont IconFont; do
                            \"\$W\" reg add \"\$WM\" /v \"\$v\" /t REG_BINARY /d $LF8 /f >/dev/null 2>&1
                        done
                        '$RUNNER_DIR/bin/wineserver' -w 2>/dev/null
                    " || true
                    echo "● 폰트 치환 + 메뉴/목록 폰트 적용 (저장 대화상자·메뉴·폴더명 한글)"

                    echo
                    echo "=== [7/9] 사용자 런처 (popmgr-ime-fix-v7) ==="
                    cat > "$HOME/.local/bin/kakaotalk" <<'LAUNCHER_EOF'
#!/bin/bash
# popmgr-ime-fix-v11 — Bottles KakaoTalk32 + 시스템 IM 자동 감지 + 소프트웨어 GL(검은 화면 우회)
#                       + 트레이 숨김 복원 리페인트 + Wine systray 창 unmap(포커스 깜빡임 차단)
WIN32_PREFIX="$HOME/.var/app/com.usebottles.bottles/data/bottles/bottles/KakaoTalk32"
RUNNER="$HOME/.var/app/com.usebottles.bottles/data/bottles/runners/wine-11.10-staging-amd64"
KAKAO_EXE="$WIN32_PREFIX/drive_c/Program Files/Kakao/KakaoTalk/KakaoTalk.exe"
[ -z "$DISPLAY" ] && export DISPLAY=:1

# 이미 실행 중이면 메인 윈도우(visible >100px)만 활성화 — 1x1 helper만 남은 좀비 케이스 차단
if pgrep -f "KakaoTalk\.exe" >/dev/null 2>&1; then
    main_wid=""
    if command -v xdotool >/dev/null; then
        # 메인 윈도우는 이름이 정확히 "KakaoTalk" — 클래스 매칭은 채팅창을 메인으로 오인함
        for w in $(xdotool search --name "^KakaoTalk$" 2>/dev/null); do
            width=$(xdotool getwindowgeometry --shell "$w" 2>/dev/null | grep ^WIDTH | cut -d= -f2)
            [ -n "$width" ] && [ "$width" -gt 100 ] 2>/dev/null && { main_wid="$w"; break; }
        done
    fi
    if [ -n "$main_wid" ]; then
        xdotool windowmap "$main_wid" 2>/dev/null
        # Wine은 트레이 숨김 창을 map만 하면 검은 창으로 뜸 — 최소화→복원으로 전체 리페인트 강제
        xdotool windowminimize "$main_wid" 2>/dev/null
        sleep 0.7
        xdotool windowactivate "$main_wid" 2>/dev/null
        xdotool windowraise "$main_wid" 2>/dev/null
        exit 0
    fi
    # 좀비 → 청소 후 새로 띄움
    pkill -9 -f "KakaoTalk\.exe" 2>/dev/null
    pkill -9 -f "winedbg" 2>/dev/null
    for pid in $(pgrep -f wineserver 2>/dev/null); do
        [ -r "/proc/$pid/environ" ] && grep -qz "KakaoTalk32" "/proc/$pid/environ" 2>/dev/null && kill -9 "$pid" 2>/dev/null
    done
    sleep 1
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

# 첫 표시 검은 창 방지 — 창이 나타나면 한 번 최소화→복원해 리페인트
(
    for _ in $(seq 1 60); do
        sleep 2
        w=$(xdotool search --name "^KakaoTalk$" 2>/dev/null | head -1)
        [ -n "$w" ] || continue
        xwininfo -id "$w" 2>/dev/null | grep -q IsViewable || continue
        sleep 1
        xdotool windowminimize "$w" 2>/dev/null
        sleep 0.7
        xdotool windowactivate "$w" 2>/dev/null
        xdotool windowraise "$w" 2>/dev/null
        break
    done
) >/dev/null 2>&1 &

# Wine standalone systray 창 숨김 — 창 전환 시 포커스 깜빡임 차단
# COSMIC 은 XEmbed 트레이(_NET_SYSTEM_TRAY_S0)를 제공하지 않아, Wine explorer 가
# 224x28 짜리 자체 트레이 창을 띄운다. 이 창이 _NET_WM_WINDOW_TYPE_NORMAL 이라
# WM 의 포커스 후보에 들어가고, 대화창을 닫아 포커스가 재배치될 때 포커스를 채간다.
# 레지스트리 ShowSystray=N 으로 트레이 자체를 끄는 방법은 쓰지 않는다.
# (그 상태에서 카카오톡 크래시를 봤는데, 카카오톡에는 시작 시 간헐적 크래시가 따로 있어
#  인과를 확정하지는 못했다. 굳이 트레이 등록을 깰 이유가 없다.)
# 트레이 기능은 그대로 두고 창만 unmap 한다 — COSMIC 에선 어차피 안 보이는 창.
(
    while pgrep -f "KakaoTalk\.exe" >/dev/null 2>&1; do
        sleep 3
        for w in $(xdotool search --class "explorer.exe" 2>/dev/null); do
            # 이름 있는 창(진짜 explorer 창)은 건드리지 않음
            [ -n "$(xdotool getwindowname "$w" 2>/dev/null)" ] && continue
            h=$(xdotool getwindowgeometry --shell "$w" 2>/dev/null | grep ^HEIGHT | cut -d= -f2)
            [ -n "$h" ] && [ "$h" -lt 60 ] 2>/dev/null || continue
            xwininfo -id "$w" 2>/dev/null | grep -q IsViewable && xdotool windowunmap "$w" 2>/dev/null
        done
    done
) >/dev/null 2>&1 &

exec flatpak run \
    --env=DISPLAY="$DISPLAY" \
    --env=XMODIFIERS="${XMODIFIERS:-@im=$SYS_IM}" \
    --env=QT_IM_MODULE="${QT_IM_MODULE:-$SYS_IM}" \
    --env=GTK_IM_MODULE="${GTK_IM_MODULE:-$SYS_IM}" \
    --env=LANG="${LANG:-ko_KR.UTF-8}" \
    --env=LC_ALL="${LC_ALL:-ko_KR.UTF-8}" \
    --env=__EGL_VENDOR_LIBRARY_DIRS="/usr/lib/x86_64-linux-gnu/GL/glvnd/egl_vendor.d:/app/lib/i386-linux-gnu/GL/glvnd/egl_vendor.d:/usr/lib/x86_64-linux-gnu/GL/default/glvnd/egl_vendor.d" \
    --env=__GLX_VENDOR_LIBRARY_NAME="${__GLX_VENDOR_LIBRARY_NAME:-mesa}" \
    --env=LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-1}" \
    --env=MESA_LOADER_DRIVER_OVERRIDE="${MESA_LOADER_DRIVER_OVERRIDE:-llvmpipe}" \
    --command=bash com.usebottles.bottles -c "
export WINEPREFIX='$WIN32_PREFIX'
export WINEARCH=win32
xsetroot -cursor_name left_ptr 2>/dev/null
'$RUNNER/bin/wine' '$KAKAO_EXE'
"
LAUNCHER_EOF
                    chmod +x "$HOME/.local/bin/kakaotalk"
                    echo "● $HOME/.local/bin/kakaotalk"

                    echo
                    echo "=== [8/9] 사용자 desktop + 아이콘 ==="
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
                    echo "=== [9/9] 데스크톱/아이콘 캐시 갱신 ==="
                    update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
                    gtk-update-icon-cache "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

                    echo
                    echo "● KakaoTalk 검증 환경 설치 완료"
                    echo "  → 앱 메뉴/독에서 카오톡 클릭 → 정상 실행 + 한글 입력 안정"
                    echo "  → 검은/흰 대화창 없음 (builtin d3d DLL 적용)"
                    echo "  → 한 번 띄운 후 다시 클릭하면 윈도우 활성화 (트레이 없어도 OK)"
                    echo "  → 저장 대화상자·메뉴·폴더명 한글 정상 (폰트 실파일 + 치환 + MenuFont/IconFont)"
                    echo "  → Wine 트레이 창 숨김으로 창 전환 시 포커스 깜빡임 차단"
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
                    echo "● KakaoTalk 설치 + 모든 보정 완료"
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
                    # [.]/[w] 패턴: bash -c cmdline 자기매칭 방지
                    pkill -9 -f "KakaoTalk[.]exe" 2>/dev/null
                    pkill -9 -f "[w]inedbg" 2>/dev/null
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
            AppsMsg::ShowKakaotalk => {
                self.running = Some("카카오톡 창 불러오는 중...".into());
                // X 버튼은 종료가 아니라 트레이 숨김인데 COSMIC은 Wine 트레이를 못 보여줌.
                // 숨겨진(unmap) 메인 윈도우를 xdotool로 다시 매핑·활성화한다.
                // 실행 중이 아니면 런처를 새로 띄운다 (런처 자체에 활성화/좀비청소 로직 있음).
                let script = r#"
                    if pgrep -f "KakaoTalk[.]exe" >/dev/null 2>&1; then
                        main_wid=""
                        for w in $(xdotool search --name "^KakaoTalk$" 2>/dev/null); do
                            width=$(xdotool getwindowgeometry --shell "$w" 2>/dev/null | grep ^WIDTH | cut -d= -f2)
                            [ -n "$width" ] && [ "$width" -gt 100 ] 2>/dev/null && { main_wid="$w"; break; }
                        done
                        if [ -n "$main_wid" ]; then
                            xdotool windowmap "$main_wid" 2>/dev/null
                            # map만 하면 Wine이 리페인트를 안 해 검은 창 — 최소화→복원으로 강제
                            xdotool windowminimize "$main_wid" 2>/dev/null
                            sleep 0.7
                            xdotool windowactivate "$main_wid" 2>/dev/null
                            xdotool windowraise "$main_wid" 2>/dev/null
                            echo "숨겨져 있던 카카오톡 창을 다시 표시했습니다."
                            exit 0
                        fi
                    fi
                    setsid -f nohup kakaotalk </dev/null >/dev/null 2>&1 \
                        || (nohup kakaotalk </dev/null >/dev/null 2>&1 & disown)
                    echo "카카오톡 실행 요청 완료"
                "#;
                let t = Task::perform(async move { runner::run_sh(script).await }, AppsMsg::Done);
                (t, None)
            }
            AppsMsg::QuitKakaotalk => {
                self.running = Some("카카오톡 완전 종료 중...".into());
                // TERM으로 정상 종료 유도 후 남으면 KILL. 같은 prefix의 wineserver까지 정리.
                let script = r#"
                    # 패턴에 [.]/[w]를 쓰는 이유: bash -c 로 실행되면 스크립트 본문이 cmdline에 남아
                    # 평범한 패턴은 pkill이 자기 자신을 죽임(자기매칭)
                    if ! pgrep -f "KakaoTalk[.]exe" >/dev/null 2>&1; then
                        echo "카카오톡이 실행 중이 아닙니다."
                    else
                        pkill -f "KakaoTalk[.]exe" 2>/dev/null
                        sleep 2
                        pgrep -f "KakaoTalk[.]exe" >/dev/null 2>&1 && pkill -9 -f "KakaoTalk[.]exe" 2>/dev/null
                        echo "카카오톡 종료 완료"
                    fi
                    pkill -9 -f "[w]inedbg" 2>/dev/null
                    for pid in $(pgrep -f wineserver 2>/dev/null); do
                        [ -r "/proc/$pid/environ" ] && grep -qz "KakaoTalk32" "/proc/$pid/environ" 2>/dev/null && kill -9 "$pid" 2>/dev/null
                    done
                    exit 0
                "#;
                let t = Task::perform(async move { runner::run_sh(script).await }, AppsMsg::Done);
                (t, None)
            }
            AppsMsg::ForceKillKakaotalk => {
                self.running = Some("카카오톡 강제 kill 중...".into());
                // 정상 종료를 기다리지 않고 KakaoTalk32 prefix 관련 프로세스를 즉시 SIGKILL.
                let script = r#"
                    killed=0
                    if pgrep -f "KakaoTalk[.]exe" >/dev/null 2>&1; then
                        pkill -9 -f "KakaoTalk[.]exe" 2>/dev/null || true
                        killed=1
                    fi
                    pkill -9 -f "[w]inedbg" 2>/dev/null || true
                    for pid in $(pgrep -f wineserver 2>/dev/null); do
                        if [ -r "/proc/$pid/environ" ] && grep -qz "KakaoTalk32" "/proc/$pid/environ" 2>/dev/null; then
                            kill -9 "$pid" 2>/dev/null || true
                            killed=1
                        fi
                    done
                    if [ "$killed" -eq 1 ]; then
                        echo "카카오톡 관련 프로세스를 강제 종료했습니다."
                    else
                        echo "강제 종료할 카카오톡 프로세스가 없습니다."
                    fi
                    exit 0
                "#;
                let t = Task::perform(async move { runner::run_sh(script).await }, AppsMsg::Done);
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
        col = col.push(Space::with_height(12));

        // Orca 카드
        col = col.push(orca_card(self.status.as_ref(), is_running));
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
                    action_btn("새로고침", AppsMsg::Refresh, !is_running, C_BTN2),
                    Space::with_width(8),
                    action_btn(
                        remove_label,
                        AppsMsg::RemoveMarked,
                        !is_running && marked_count > 0,
                        C_ERR,
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
    let status_txt = if installed { "● 설치됨" } else { "○ 미설치" };
    let status_col = if installed { C_OK } else { C_DIM };

    let launcher = status.and_then(|s| s.kakaotalk_launcher.clone()).unwrap_or_default();
    let exe = status.and_then(|s| s.kakaotalk_exe.clone()).unwrap_or_default();
    let desktop = status.and_then(|s| s.kakaotalk_desktop.clone()).unwrap_or_default();
    let wmclass_ok = status.map(|s| s.kakaotalk_wmclass_ok).unwrap_or(false);
    let icon_ok = status.map(|s| s.kakaotalk_icon_ok).unwrap_or(false);
    let ime_patched = status.map(|s| s.kakaotalk_ime_patched).unwrap_or(false);

    let mut left = column![
        text("KakaoTalk (Wine)").size(14).color(C_TEXT),
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
        let icon_state = if icon_ok { "● 아이콘 테마 등록됨" } else { "○ 아이콘 미등록 — 독 즐겨찾기 빈 칸" };
        let icon_c = if icon_ok { C_DIM } else { C_WARN };
        left = left.push(text(icon_state).size(11).color(icon_c));
        let ime_state = if ime_patched { "● 한글 입력 안정화 적용됨 (popmgr-ime-fix-v1)" } else { "※ 한글 입력 가끔 안 됨 — IME 안정화 미적용" };
        let ime_c = if ime_patched { C_DIM } else { C_WARN };
        left = left.push(text(ime_state).size(11).color(ime_c));
        left = left.push(Space::with_height(4));
        left = left.push(
            text("X 버튼은 종료가 아니라 트레이 숨김입니다 (COSMIC엔 Wine 트레이가 안 보임). 창이 사라졌으면 '창 보이기', 끝내려면 '완전 종료'.")
                .size(10).color(C_DIM),
        );
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
    if installed {
        right = right.push(action_btn("창 보이기", AppsMsg::ShowKakaotalk, !disabled, C_BLUE));
        right = right.push(action_btn("완전 종료", AppsMsg::QuitKakaotalk, !disabled, C_ERR));
        right = right.push(action_btn("강제 kill", AppsMsg::ForceKillKakaotalk, !disabled, C_ERR));
    }
    if all_ok {
        right = right.push(text("● 모든 설정 완료").size(11).color(C_OK));
    }

    card(
        row![
            left.width(Length::Fill),
            right,
        ]
        .align_y(iced::Alignment::Center)
    )
}

fn orca_card(status: Option<&AppsStatus>, disabled: bool) -> Element<'static, AppsMsg> {
    let appimage = status.and_then(|s| s.orca_appimage.clone());
    let version = status.and_then(|s| s.orca_version.clone()).unwrap_or_default();
    let desktop = status.and_then(|s| s.orca_desktop.clone()).unwrap_or_default();
    let icon_ok = status.map(|s| s.orca_icon_ok).unwrap_or(false);
    let dock_ok = status.map(|s| s.orca_dock_ok).unwrap_or(false);

    let deb = status.map(|s| s.orca_deb_installed).unwrap_or(false);
    let has_appimage = appimage.is_some();
    let registered = deb || !desktop.is_empty();

    let status_txt = if deb {
        "● 설치됨 (apt 패키지 orca-ide)"
    } else if !desktop.is_empty() {
        "● 런처 등록됨 (AppImage)"
    } else if has_appimage {
        "○ AppImage 있음 — 런처 미등록"
    } else {
        "○ 미설치 — 누르면 공식 릴리스에서 받아 설치합니다"
    };
    let status_col = if registered { C_OK } else if has_appimage { C_WARN } else { C_DIM };

    let kind = if deb { "deb" } else { "AppImage" };
    let title = if version.is_empty() {
        "Orca".to_string()
    } else {
        format!("Orca {version} ({kind})")
    };

    let mut left = column![
        text(title).size(14).color(C_TEXT),
        Space::with_height(3),
        text("에이전트 개발용 IDE — 공식 .deb 를 받아 설치하고 런처·독까지 등록한다").size(11).color(C_DIM),
        Space::with_height(4),
        text(status_txt).size(12).color(status_col),
    ];

    if deb {
        left = left.push(Space::with_height(2));
        left = left.push(text("업데이트·제거는 apt 로 관리됩니다 (stablyai/orca 공식 패키지)").size(11).color(C_DIM));
    } else if let Some(path) = &appimage {
        left = left.push(Space::with_height(2));
        left = left.push(text(format!("AppImage: {path}")).size(11).color(C_DIM));
    }

    if !desktop.is_empty() {
        left = left.push(text(format!("바로가기: {desktop}")).size(11).color(C_DIM));
    }

    let icon_state = if icon_ok { "● 아이콘 테마 등록됨" } else { "○ 아이콘 미등록 — 독 즐겨찾기 빈 칸" };
    let icon_c = if icon_ok { C_DIM } else { C_WARN };
    left = left.push(text(icon_state).size(11).color(icon_c));

    let dock_state = if dock_ok { "● 독 즐겨찾기 등록됨" } else { "○ 독 미등록" };
    let dock_c = if dock_ok { C_DIM } else { C_WARN };
    left = left.push(text(dock_state).size(11).color(dock_c));

    if registered {
        left = left.push(Space::with_height(4));
        left = left.push(
            text("터미널에서 'orca' 를 치면 GNOME 스크린리더가 실행됩니다 (이름 충돌). IDE 는 독 아이콘으로 여세요.")
                .size(10).color(C_DIM),
        );
    }

    let all_ok = registered && icon_ok && dock_ok;
    let label = if !registered {
        "Orca 설치"
    } else if !all_ok {
        "Orca 설치 (보정)"
    } else {
        "Orca 재설치"
    };

    let mut right = column![].spacing(6).align_x(iced::Alignment::End);
    right = right.push(action_btn(label, AppsMsg::InstallOrca, !disabled, C_OK));
    if all_ok {
        right = right.push(text("● 모든 설정 완료").size(11).color(C_OK));
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
    let bg = if pkg.marked { Color { r: 0.996, g: 0.925, b: 0.933, a: 1.0 } } else { C_SURFACE };
    let border = if pkg.marked { C_ERR } else { C_BORDER };
    let kind_txt = match pkg.kind { PkgKind::Apt => "APT", PkgKind::Flatpak => "Flatpak" };
    let kind_col = match pkg.kind { PkgKind::Apt => C_BLUE, PkgKind::Flatpak => Color::from_rgb(0.55, 0.36, 0.96) };

    let check_bg = if pkg.marked { C_ERR } else { C_SURFACE2 };
    let check_txt = if pkg.marked { "●" } else { " " };

    let checkbox = container(
        text(check_txt).size(12).color(Color::WHITE)
    )
    .width(20).height(20)
    .style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(check_bg)),
        border: iced::Border { radius: 4.0.into(), color: C_BORDER, width: 1.5 },
        ..Default::default()
    });

    let row_inner = row![
        checkbox,
        Space::with_width(10),
        column![
            row![
                text(&pkg.name).size(13).color(C_TEXT),
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
        text_color: C_TEXT,
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
        "LC_ALL=C flatpak list --app --columns=application,version,name 2>/dev/null"
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

    // Orca 정보 수집
    // 공식 .deb 로 깔렸으면 dpkg 가 런처·아이콘을 이미 등록해 둔다.
    // AppImage 는 .desktop 을 스스로 설치하지 않으므로 파일 존재와 등록을 따로 본다.
    let orca_deb_installed = runner::run("bash", &["-c",
        "dpkg -s orca-ide >/dev/null 2>&1"
    ]).await.success;

    let orca_lookup = runner::run("bash", &["-c",
        "for C in $HOME/Applications/orca*.AppImage $HOME/Applications/Orca*.AppImage \
                  $HOME/Downloads/orca*.AppImage $HOME/Downloads/Orca*.AppImage \
                  $HOME/.local/bin/orca*.AppImage $HOME/orca*.AppImage; do \
            [ -f \"$C\" ] && { echo \"$C\"; break; }; \
         done"
    ]).await;
    let orca_appimage = orca_lookup.output.lines().next()
        .map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    // .deb 는 /usr/share, 수동 등록은 ~/.local/share 에 놓인다. 둘 다 본다.
    let orca_desk_lookup = runner::run("bash", &["-c",
        "for D in /usr/share/applications/orca-ide.desktop \
                  $HOME/.local/share/applications/orca-ide.desktop; do \
            [ -f \"$D\" ] && { echo \"$D\"; break; }; \
         done"
    ]).await;
    let orca_desktop = orca_desk_lookup.output.lines().next()
        .map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    // deb 면 dpkg 가 정본이다. AppImage 면 설치 때 .desktop 에 적어둔 줄을 읽는다
    // (AppImage 를 열어 버전을 뽑는 것은 느리다).
    let orca_version = if orca_deb_installed {
        runner::run("bash", &["-c",
            "dpkg-query -W -f='${Version}' orca-ide 2>/dev/null"
        ]).await.output.lines().next()
            .map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    } else if orca_desktop.is_some() {
        runner::run("bash", &["-c",
            "grep -h '^X-AppImage-Version=' $HOME/.local/share/applications/orca-ide.desktop \
             2>/dev/null | head -1 | cut -d= -f2-"
        ]).await.output.lines().next()
            .map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    } else { None };

    let orca_icon_ok = runner::run("bash", &["-c",
        "ls $HOME/.local/share/icons/hicolor/*/apps/orca-ide.png \
            /usr/share/icons/hicolor/*/apps/orca-ide.png 2>/dev/null | head -1"
    ]).await.output.lines().any(|s| !s.trim().is_empty());

    let orca_dock_ok = runner::run("bash", &["-c",
        "grep -q '\"orca-ide\"' \
         $HOME/.config/cosmic/com.system76.CosmicAppList/v1/favorites 2>/dev/null"
    ]).await.success;

    AppsStatus {
        kakaotalk_installed,
        kakaotalk_launcher,
        kakaotalk_exe,
        kakaotalk_desktop,
        kakaotalk_wmclass_ok,
        kakaotalk_icon_ok,
        kakaotalk_ime_patched,
        orca_deb_installed,
        orca_appimage,
        orca_version,
        orca_desktop,
        orca_icon_ok,
        orca_dock_ok,
        packages,
    }
}
