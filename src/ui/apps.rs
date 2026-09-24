use super::ime::{TYPE_SCREEN_TITLE, TYPE_BODY, TYPE_CAPTION, TYPE_CHIP, FONT_BOLD, FONT_SEMIBOLD, RADIUS_CHIP};
use iced::{
    widget::{column, container, row, scrollable, text, text_input, Space},
    Color, Element, Length, Task,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use crate::runner::{self, CmdResult};
use super::cosmic_tweaks::{is_target_shortcut, parse_shortcut_entries, write_shortcut_updates};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_BORDER, C_BTN2, C_DIM, C_ERR, C_OK, C_SURFACE, C_SURFACE2, C_TEXT, C_WARN, C_ERR_BG, C_PURPLE};

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
    pub recording: RecordingStatus,
    pub mpv: MpvStatus,
    pub packages: Vec<Package>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlatpakScope { System, User, NotInstalled }

#[derive(Debug, Clone)]
pub struct RecordingStatus {
    gsr_scope: FlatpakScope,
    system_flathub: bool,
    nvidia_extension: Option<String>,
    system_runtime: bool,
    user_runtime: bool,
    obs_scope: FlatpakScope,
    obs_runtime_matches: bool,
    active: bool,
    shortcut_registered: bool,
    output_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct MpvStatus {
    installed: bool,
    vaapi: VaApiStatus,
    default_player: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VaApiStatus { Supported, Unsupported, Unknown }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayerState { Default, InstalledNotDefault, NotInstalled }

const VIDEO_MIME_TYPES: [&str; 9] = [
    "video/mp4",
    "video/x-matroska",
    "video/webm",
    "video/x-msvideo",
    "video/quicktime",
    "video/mpeg",
    "video/x-flv",
    "video/3gpp",
    "video/ogg",
];

fn xdg_mime_default_args() -> Vec<&'static str> {
    let mut args = vec!["default", "mpv.desktop"];
    args.extend(VIDEO_MIME_TYPES);
    args
}

fn mpv_player_state(installed: bool, default_is_mpv: bool) -> PlayerState {
    if !installed {
        PlayerState::NotInstalled
    } else if default_is_mpv {
        PlayerState::Default
    } else {
        PlayerState::InstalledNotDefault
    }
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
    InstallMpv,
    SetMpvDefaultPlayer,
    InstallRecording,
    RegisterRecordingShortcut,
    ToggleRecording,
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
            AppsMsg::InstallRecording => {
                let Some(recording) = self.status.as_ref().map(|status| &status.recording) else {
                    return (Task::none(), Some(CmdResult {
                        success: false,
                        output: "상태를 먼저 불러오는 중입니다.".into(),
                    }));
                };
                let Some(extension) = recording.nvidia_extension.clone() else {
                    return (Task::none(), Some(CmdResult {
                        success: false,
                        output: "호스트 NVIDIA 드라이버 버전을 읽지 못했습니다.".into(),
                    }));
                };
                self.running = Some("GPU Screen Recorder 설치·런타임 맞춤 중...".into());
                let script = recording_install_script(
                    recording.gsr_scope,
                    recording.system_flathub,
                    recording.obs_scope,
                    &extension,
                );
                let t = Task::perform(async move { runner::run_stream(&script).await }, AppsMsg::Done);
                (t, None)
            }
            AppsMsg::InstallMpv => {
                self.running = Some("mpv·VA-API 정보 도구 설치 중...".into());
                let script = "pkexec apt-get install -y mpv vainfo";
                let t = Task::perform(async move { runner::run_stream(script).await }, AppsMsg::Done);
                (t, None)
            }
            AppsMsg::SetMpvDefaultPlayer => {
                self.running = Some("mpv를 기본 동영상 플레이어로 지정 중...".into());
                let t = Task::perform(async { set_mpv_default_player().await }, AppsMsg::Done);
                (t, None)
            }
            AppsMsg::RegisterRecordingShortcut => {
                let result = register_recording_shortcut();
                (Task::none(), Some(result))
            }
            AppsMsg::ToggleRecording => {
                let result = record_toggle_cli_result();
                let refresh = Task::perform(async { scan_apps().await }, AppsMsg::Refreshed);
                (refresh, Some(result))
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
            text("앱 관리").size(TYPE_SCREEN_TITLE).font(FONT_BOLD),
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
        col = col.push(Space::with_height(12));

        // GPU Screen Recorder 카드
        col = col.push(recording_card(self.status.as_ref(), is_running));
        col = col.push(Space::with_height(12));

        // 동영상 플레이어 카드
        col = col.push(mpv_card(self.status.as_ref(), is_running));
        col = col.push(Space::with_height(20));

        // 프로그램 제거 섹션
        col = col.push(text("프로그램 제거").size(eond_ui_theme::TYPE_DIALOG_TITLE).font(FONT_SEMIBOLD));
        col = col.push(Space::with_height(8));
        col = col.push(
            text_input("이름으로 검색...", &self.search)
                .style(eond_ui_theme::iced_theme::text_input::default)
                .on_input(AppsMsg::SearchChanged)
                .padding([8, 10])
                .size(TYPE_BODY)
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
                col = col.push(text("검색 결과 없음").size(TYPE_BODY).color(C_DIM));
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
                    text(format!("{marked_count}개 선택됨")).size(TYPE_CAPTION).color(C_DIM),
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
            col = col.push(text("스캔 중...").size(TYPE_BODY).color(C_DIM));
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
        text("KakaoTalk (Wine)").size(TYPE_BODY).font(FONT_SEMIBOLD).color(C_TEXT),
        Space::with_height(3),
        text("eondcom/kakaotalk-wine — Wine 기반 카카오톡 Linux 설치").size(TYPE_CAPTION).color(C_DIM),
        Space::with_height(4),
        text(status_txt).size(TYPE_CAPTION).color(status_col),
    ];

    if installed {
        if !launcher.is_empty() {
            left = left.push(Space::with_height(2));
            left = left.push(text(format!("실행 스크립트: {launcher}")).size(TYPE_CAPTION).color(C_DIM));
        }
        if !exe.is_empty() {
            left = left.push(text(format!("KakaoTalk.exe: {exe}")).size(TYPE_CAPTION).color(C_DIM));
        }
        if !desktop.is_empty() {
            let wm_state = if wmclass_ok { "(WMClass OK)" } else { "(WMClass 없음 — 독 아이콘 매칭 불가)" };
            let col_ = if wmclass_ok { C_DIM } else { C_WARN };
            left = left.push(text(format!("바로가기: {desktop} {wm_state}")).size(TYPE_CAPTION).color(col_));
        }
        let icon_state = if icon_ok { "● 아이콘 테마 등록됨" } else { "○ 아이콘 미등록 — 독 즐겨찾기 빈 칸" };
        let icon_c = if icon_ok { C_DIM } else { C_WARN };
        left = left.push(text(icon_state).size(TYPE_CAPTION).color(icon_c));
        let ime_state = if ime_patched { "● 한글 입력 안정화 적용됨 (popmgr-ime-fix-v1)" } else { "※ 한글 입력 가끔 안 됨 — IME 안정화 미적용" };
        let ime_c = if ime_patched { C_DIM } else { C_WARN };
        left = left.push(text(ime_state).size(TYPE_CAPTION).color(ime_c));
        left = left.push(Space::with_height(4));
        left = left.push(
            text("X 버튼은 종료가 아니라 트레이 숨김입니다 (COSMIC엔 Wine 트레이가 안 보임). 창이 사라졌으면 '창 보이기', 끝내려면 '완전 종료'.")
                .size(TYPE_CHIP).color(C_DIM),
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
        right = right.push(text("● 모든 설정 완료").size(TYPE_CAPTION).color(C_OK));
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
        text(title).size(TYPE_BODY).font(FONT_SEMIBOLD).color(C_TEXT),
        Space::with_height(3),
        text("에이전트 개발용 IDE — 공식 .deb 를 받아 설치하고 런처·독까지 등록한다").size(TYPE_CAPTION).color(C_DIM),
        Space::with_height(4),
        text(status_txt).size(TYPE_CAPTION).color(status_col),
    ];

    if deb {
        left = left.push(Space::with_height(2));
        left = left.push(text("업데이트·제거는 apt 로 관리됩니다 (stablyai/orca 공식 패키지)").size(TYPE_CAPTION).color(C_DIM));
    } else if let Some(path) = &appimage {
        left = left.push(Space::with_height(2));
        left = left.push(text(format!("AppImage: {path}")).size(TYPE_CAPTION).color(C_DIM));
    }

    if !desktop.is_empty() {
        left = left.push(text(format!("바로가기: {desktop}")).size(TYPE_CAPTION).color(C_DIM));
    }

    let icon_state = if icon_ok { "● 아이콘 테마 등록됨" } else { "○ 아이콘 미등록 — 독 즐겨찾기 빈 칸" };
    let icon_c = if icon_ok { C_DIM } else { C_WARN };
    left = left.push(text(icon_state).size(TYPE_CAPTION).color(icon_c));

    let dock_state = if dock_ok { "● 독 즐겨찾기 등록됨" } else { "○ 독 미등록" };
    let dock_c = if dock_ok { C_DIM } else { C_WARN };
    left = left.push(text(dock_state).size(TYPE_CAPTION).color(dock_c));

    if registered {
        left = left.push(Space::with_height(4));
        left = left.push(
            text("터미널에서 'orca' 를 치면 GNOME 스크린리더가 실행됩니다 (이름 충돌). IDE 는 독 아이콘으로 여세요.")
                .size(TYPE_CHIP).color(C_DIM),
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
        right = right.push(text("● 모든 설정 완료").size(TYPE_CAPTION).color(C_OK));
    }

    card(
        row![
            left.width(Length::Fill),
            right,
        ]
        .align_y(iced::Alignment::Center)
    )
}

fn recording_card(status: Option<&AppsStatus>, disabled: bool) -> Element<'static, AppsMsg> {
    let recording = status.map(|status| &status.recording);
    let gsr_scope = recording.map(|s| s.gsr_scope).unwrap_or(FlatpakScope::NotInstalled);
    let gsr_text = match gsr_scope {
        FlatpakScope::System => "GSR: 시스템 범위 — 정상",
        FlatpakScope::User => "GSR: 사용자 범위 — 캡처 불가, 시스템으로 재설치 필요",
        FlatpakScope::NotInstalled => "GSR: 미설치",
    };
    let gsr_color = match gsr_scope {
        FlatpakScope::System => C_OK,
        FlatpakScope::User => C_WARN,
        FlatpakScope::NotInstalled => C_DIM,
    };
    let flathub = if recording.is_some_and(|s| s.system_flathub) { "있음" } else { "없음" };
    let extension = recording
        .and_then(|s| s.nvidia_extension.as_deref())
        .unwrap_or("호스트 NVIDIA 드라이버 없음");
    let system_runtime = if recording.is_some_and(|s| s.system_runtime) { "있음" } else { "없음" };
    let user_runtime = if recording.is_some_and(|s| s.user_runtime) { "있음" } else { "없음" };
    let obs = match recording.map(|s| s.obs_scope).unwrap_or(FlatpakScope::NotInstalled) {
        FlatpakScope::NotInstalled => "OBS: 미설치",
        _ if recording.is_some_and(|s| s.obs_runtime_matches) => "OBS NVENC: 런타임 일치",
        _ => "OBS NVENC: 런타임 불일치",
    };
    let active = recording.is_some_and(|s| s.active);
    let shortcut = if recording.is_some_and(|s| s.shortcut_registered) { "등록됨" } else { "미등록" };
    let output = recording
        .map(|s| s.output_dir.display().to_string())
        .unwrap_or_else(|| recording_output_dir().display().to_string());
    let left = column![
        text("화면 녹화 (GPU Screen Recorder · NVENC)").size(TYPE_BODY).font(FONT_SEMIBOLD).color(C_TEXT),
        Space::with_height(3),
        text(gsr_text).size(TYPE_CAPTION).color(gsr_color),
        text(format!("시스템 flathub 원격: {flathub}")).size(TYPE_CAPTION).color(C_DIM),
        text(format!("NVIDIA 런타임 {extension}: system {system_runtime} · user {user_runtime}"))
            .size(TYPE_CAPTION).color(C_DIM),
        text(obs).size(TYPE_CAPTION).color(C_DIM),
        text(if active { "녹화: 진행 중" } else { "녹화: 정지" }).size(TYPE_CAPTION)
            .color(if active { C_OK } else { C_DIM }),
        text(format!("Ctrl+Shift+6: {shortcut} · 출력: {output}")).size(TYPE_CAPTION).color(C_DIM),
        Space::with_height(3),
        text("설치 시 시스템 인증(polkit) 창이 뜹니다.").size(TYPE_CHIP).color(C_WARN),
    ];
    let mut right = column![].spacing(6).align_x(iced::Alignment::End);
    right = right.push(action_btn("설치/런타임 맞추기", AppsMsg::InstallRecording, !disabled, C_OK));
    right = right.push(action_btn(
        "단축키 등록 (Ctrl+Shift+6)",
        AppsMsg::RegisterRecordingShortcut,
        !disabled,
        C_BLUE,
    ));
    right = right.push(action_btn(
        if active { "지금 녹화 정지" } else { "지금 녹화 시작" },
        AppsMsg::ToggleRecording,
        !disabled,
        C_BTN2,
    ));
    card(row![left.width(Length::Fill), right].align_y(iced::Alignment::Center))
}

fn mpv_card(status: Option<&AppsStatus>, disabled: bool) -> Element<'static, AppsMsg> {
    let mpv = status.map(|status| &status.mpv);
    let installed = mpv.is_some_and(|status| status.installed);
    let default_player = mpv.and_then(|status| status.default_player.as_deref());
    let default_is_mpv = default_player == Some("mpv.desktop");
    let player_state = mpv_player_state(installed, default_is_mpv);
    let (status_text, status_color) = match player_state {
        PlayerState::Default => ("[OK] mpv 기본 플레이어로 설정됨", C_OK),
        PlayerState::InstalledNotDefault => ("[권장] mpv 설치됨 — 기본 플레이어 아님", C_WARN),
        PlayerState::NotInstalled => ("[!] mpv 미설치", C_ERR),
    };
    let vaapi = mpv.map(|status| status.vaapi).unwrap_or(VaApiStatus::Unknown);
    let vaapi_text = match vaapi {
        VaApiStatus::Supported => "VA-API 디코딩: 지원됨 (VAEntrypointVLD)",
        VaApiStatus::Unsupported => "VA-API 디코딩: 지원 정보 없음",
        VaApiStatus::Unknown => "VA-API 디코딩: 확인 불가 (vainfo 미설치)",
    };
    let vaapi_color = match vaapi {
        VaApiStatus::Supported => C_OK,
        VaApiStatus::Unsupported => C_WARN,
        VaApiStatus::Unknown => C_DIM,
    };
    let default_text = match default_player {
        Some("mpv.desktop") => "현재 기본 플레이어: mpv.desktop".to_string(),
        Some(player) => format!("현재: {player}"),
        None => "현재 기본 플레이어: 확인 불가".to_string(),
    };
    let left = column![
        text("동영상 플레이어 (mpv · VA-API)").size(TYPE_BODY).font(FONT_SEMIBOLD).color(C_TEXT),
        Space::with_height(3),
        text(status_text).size(TYPE_CAPTION).color(status_color),
        text(vaapi_text).size(TYPE_CAPTION).color(vaapi_color),
        text(default_text).size(TYPE_CAPTION).color(C_DIM),
    ];
    let mut right = column![].spacing(6).align_x(iced::Alignment::End);
    if !installed {
        right = right.push(action_btn("mpv 설치", AppsMsg::InstallMpv, !disabled, C_OK));
    }
    right = right.push(action_btn(
        "기본 플레이어로 지정",
        AppsMsg::SetMpvDefaultPlayer,
        !disabled && installed,
        C_BLUE,
    ));
    card(row![left.width(Length::Fill), right].align_y(iced::Alignment::Center))
}

fn pkg_row(idx: usize, pkg: &Package, disabled: bool) -> Element<'_, AppsMsg> {
    let bg = if pkg.marked { C_ERR_BG } else { C_SURFACE };
    let border = if pkg.marked { C_ERR } else { C_BORDER };
    let kind_txt = match pkg.kind { PkgKind::Apt => "APT", PkgKind::Flatpak => "Flatpak" };
    let kind_col = match pkg.kind { PkgKind::Apt => C_BLUE, PkgKind::Flatpak => C_PURPLE };

    let check_bg = if pkg.marked { C_ERR } else { C_SURFACE2 };
    let check_txt = if pkg.marked { "●" } else { " " };

    let checkbox = container(
        text(check_txt).size(TYPE_CAPTION).color(Color::WHITE)
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
                text(&pkg.name).size(TYPE_BODY).color(C_TEXT),
                Space::with_width(8),
                text(kind_txt).size(TYPE_CHIP).color(kind_col),
                Space::with_width(8),
                text(&pkg.version).size(TYPE_CHIP).color(C_DIM),
            ].align_y(iced::Alignment::Center),
            text(&pkg.description).size(TYPE_CAPTION).color(C_DIM),
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
        border: iced::Border { radius: RADIUS_CHIP.into(), color: border, width: 1.0 },
        text_color: C_TEXT,
        ..Default::default()
    })
    .into()
}

const GSR_APP: &str = "com.dec05eba.gpu_screen_recorder";
const OBS_APP: &str = "com.obsproject.Studio";

fn flatpak_scope(list: &str, app: &str) -> FlatpakScope {
    let mut user = false;
    for line in list.lines() {
        let mut columns = line.split_whitespace();
        if columns.next() != Some(app) {
            continue;
        }
        match columns.next() {
            Some("system") => return FlatpakScope::System,
            Some("user") => user = true,
            _ => {}
        }
    }
    if user { FlatpakScope::User } else { FlatpakScope::NotInstalled }
}

fn flatpak_in_scope(list: &str, app: &str, scope: &str) -> bool {
    list.lines().any(|line| {
        let mut columns = line.split_whitespace();
        columns.next() == Some(app) && columns.next() == Some(scope)
    })
}

fn nvidia_runtime_extension(version: &str) -> Option<String> {
    let version = version.trim();
    if version.is_empty() {
        return None;
    }
    Some(format!("org.freedesktop.Platform.GL.nvidia-{}", version.replace('.', "-")))
}

fn recording_output_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("Videos/Recordings")
}

fn shortcuts_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| {
        home.join(".config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom")
    })
}

fn recording_shortcut_registered(path: &Path) -> bool {
    std::fs::read_to_string(path).ok().is_some_and(|content| {
        parse_shortcut_entries(&content).iter().any(|entry| is_target_shortcut(entry, "6"))
    })
}

fn recording_install_script(
    gsr_scope: FlatpakScope,
    system_flathub: bool,
    obs_scope: FlatpakScope,
    extension: &str,
) -> String {
    let mut script = String::from("set -e\n");
    if gsr_scope == FlatpakScope::User {
        script.push_str(&format!("flatpak uninstall --user -y --noninteractive {GSR_APP}\n"));
    }
    if !system_flathub {
        script.push_str("flatpak remote-add --system --if-not-exists flathub ");
        script.push_str("https://dl.flathub.org/repo/flathub.flatpakrepo\n");
    }
    script.push_str(&format!(
        "flatpak install --system -y --noninteractive flathub {GSR_APP} {extension}\n"
    ));
    if obs_scope == FlatpakScope::User {
        script.push_str(&format!(
            "flatpak install --user -y --noninteractive flathub {extension}\n"
        ));
    }
    script
}

fn newest_file(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.filter_map(Result::ok).filter_map(|entry| {
        let metadata = entry.metadata().ok()?;
        if metadata.is_file() {
            Some((metadata.modified().ok()?, entry.path()))
        } else {
            None
        }
    }).max_by_key(|(modified, _)| *modified).map(|(_, path)| path)
}

// `-ro`는 리플레이(`-r`) 모드 전용이라 일반 녹화에 쓰면 파일이 전혀 생성되지 않는다(2026-09-14 실측).
// 일반 녹화는 `-o <파일>`이 필수(man: "Required except when outputting to stdout").
fn recording_start_args(output_file: &Path) -> Vec<String> {
    vec![
        "flatpak".into(), "run".into(), "--command=gpu-screen-recorder".into(), GSR_APP.into(),
        "-w".into(), "portal".into(), "-restore-portal-session".into(), "yes".into(),
        "-f".into(), "60".into(), "-k".into(), "h264".into(), "-encoder".into(), "gpu".into(),
        "-fallback-cpu-encoding".into(), "no".into(), "-a".into(), "default_output".into(),
        "-ac".into(), "opus".into(), "-c".into(), "mp4".into(), "-cursor".into(), "yes".into(),
        "-o".into(), output_file.display().to_string(),
    ]
}

fn recording_output_file(output_dir: &Path) -> PathBuf {
    let ts = chrono_like_timestamp();
    output_dir.join(format!("Recording_{ts}.mp4"))
}

/// `chrono` 의존성 없이 로컬 타임스탬프 문자열을 만든다(다른 탭들의 관례와 동일).
fn chrono_like_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    civil_datetime_string(now)
}

/// UNIX 초(UTC) → "YYYY-MM-DD_HH-MM-SS". 순수 함수라 테스트가 고정 입력으로 검증한다.
fn civil_datetime_string(secs: u64) -> String {
    let days = secs / 86400;
    let secs_of_day = secs % 86400;
    // 1970-01-01 기준 날짜 계산 (civil_from_days, Howard Hinnant 알고리즘)
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let (h, min, s) = (secs_of_day / 3600, (secs_of_day % 3600) / 60, secs_of_day % 60);
    format!("{y:04}-{m:02}-{d:02}_{h:02}-{min:02}-{s:02}")
}

fn notify(summary: &str, body: &str) {
    let _ = Command::new("notify-send").args([summary, body]).status();
}

fn record_toggle_cli_result() -> CmdResult {
    // 실제 바이너리 이름은 15자("gpu-screen-reco")로 잘려 `-x`가 절대 매칭되지 않는다(2026-09-14 실측).
    // `-f`로 실행 명령행 전체를 보되 ` -w`를 붙여 flatpak 래퍼(`--command=... com.dec05eba...`)는 제외한다.
    const PGREP_PATTERN: &str = "gpu-screen-recorder -w";
    let active = Command::new("pgrep").args(["-f", PGREP_PATTERN]).status()
        .is_ok_and(|status| status.success());
    let output_dir = recording_output_dir();
    if active {
        let stopped = Command::new("pkill").args(["-INT", "-f", PGREP_PATTERN])
            .status().is_ok_and(|status| status.success());
        if !stopped {
            return CmdResult { success: false, output: "녹화 정지 신호 전송 실패".into() };
        }
        std::thread::sleep(Duration::from_secs(1));
        let filename = newest_file(&output_dir).and_then(|path| {
            path.file_name().map(|name| name.to_string_lossy().into_owned())
        }).unwrap_or_else(|| "저장 파일을 찾지 못했습니다".into());
        notify("녹화 저장됨", &filename);
        return CmdResult { success: true, output: format!("녹화 저장됨: {filename}") };
    }
    let apps = Command::new("flatpak").args(["list", "--app", "--columns=application,installation"])
        .output().map(|output| String::from_utf8_lossy(&output.stdout).into_owned()).unwrap_or_default();
    match flatpak_scope(&apps, GSR_APP) {
        FlatpakScope::System => {}
        FlatpakScope::User => {
            notify("녹화 시작 불가", "GPU Screen Recorder를 시스템 범위로 재설치하세요.");
            return CmdResult { success: false, output: "GSR가 사용자 범위에 설치되어 캡처할 수 없습니다.".into() };
        }
        FlatpakScope::NotInstalled => {
            notify("녹화 시작 불가", "GPU Screen Recorder를 먼저 시스템 범위로 설치하세요.");
            return CmdResult { success: false, output: "GPU Screen Recorder가 설치되지 않았습니다.".into() };
        }
    }
    if let Err(error) = std::fs::create_dir_all(&output_dir) {
        return CmdResult { success: false, output: format!("녹화 폴더 생성 실패: {error}") };
    }
    let output_file = recording_output_file(&output_dir);
    let args = recording_start_args(&output_file);
    let started = Command::new("setsid").args(&args).stdin(Stdio::null()).stdout(Stdio::null())
        .stderr(Stdio::null()).spawn();
    match started {
        Ok(_) => {
            notify("녹화 시작", "Ctrl+Shift+6 으로 정지");
            CmdResult { success: true, output: "녹화를 시작했습니다.".into() }
        }
        Err(error) => CmdResult { success: false, output: format!("녹화 시작 실패: {error}") },
    }
}

pub fn record_toggle_cli() -> i32 {
    if record_toggle_cli_result().success { 0 } else { 1 }
}

fn register_recording_shortcut() -> CmdResult {
    let Some(path) = shortcuts_config_path() else {
        return CmdResult { success: false, output: "홈 디렉터리를 찾을 수 없습니다.".into() };
    };
    let executable = match std::env::current_exe() {
        Ok(path) if path.is_absolute() => path,
        Ok(_) => return CmdResult { success: false, output: "popmgr 절대 경로를 찾지 못했습니다.".into() },
        Err(error) => return CmdResult { success: false, output: format!("popmgr 경로 확인 실패: {error}") },
    };
    let command = format!("{} --record-toggle", executable.display());
    if let Err(error) = write_shortcut_updates(&path, &[("6", command.clone())]) {
        return CmdResult { success: false, output: format!("단축키 저장 실패: {error}") };
    }
    match std::fs::read_to_string(&path).and_then(|content| {
        std::fs::write(&path, add_recording_shortcut_description(&content, &command))
    }) {
        Ok(()) => CmdResult {
            success: true,
            output: "Ctrl+Shift+6 녹화 토글 단축키를 등록했습니다: popmgr 화면 녹화 토글".into(),
        },
        Err(error) => CmdResult { success: false, output: format!("단축키 설명 저장 실패: {error}") },
    }
}

fn add_recording_shortcut_description(content: &str, command: &str) -> String {
    let entry = format!("(modifiers: [Ctrl, Shift], key: \"6\"): Spawn(\"{command}\")");
    let described = format!(
        "(modifiers: [Ctrl, Shift], key: \"6\", description: Some(\"popmgr 화면 녹화 토글\")): \\
         Spawn(\"{command}\")"
    );
    content.replace(&entry, &described)
}

async fn set_mpv_default_player() -> CmdResult {
    let args = xdg_mime_default_args();
    let set_result = runner::run("xdg-mime", &args).await;
    let mut failed_mime_types = Vec::new();
    for mime_type in VIDEO_MIME_TYPES {
        let result = runner::run("xdg-mime", &["query", "default", mime_type]).await;
        if result.output.trim() != "mpv.desktop" {
            failed_mime_types.push(mime_type);
        }
    }
    if failed_mime_types.is_empty() && set_result.success {
        CmdResult {
            success: true,
            output: "mpv를 모든 동영상 MIME 타입의 기본 플레이어로 지정했습니다.".into(),
        }
    } else if failed_mime_types.is_empty() {
        CmdResult {
            success: false,
            output: format!("기본 플레이어 지정 명령 실패:\n{}", set_result.output.trim()),
        }
    } else {
        CmdResult {
            success: false,
            output: format!(
                "mpv.desktop으로 지정되지 않은 MIME 타입: {}",
                failed_mime_types.join(", "),
            ),
        }
    }
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

    let flatpak_scopes = runner::run("flatpak", &[
        "list", "--app", "--columns=application,installation",
    ]).await.output;
    let gsr_scope = flatpak_scope(&flatpak_scopes, GSR_APP);
    let obs_scope = flatpak_scope(&flatpak_scopes, OBS_APP);
    let system_flathub = runner::run("flatpak", &[
        "remotes", "--system", "--columns=name",
    ]).await.output.lines().any(|name| name.trim() == "flathub");
    let nvidia_extension = std::fs::read_to_string("/sys/module/nvidia/version")
        .ok().and_then(|version| nvidia_runtime_extension(&version));
    let runtime_scopes = runner::run("flatpak", &[
        "list", "--runtime", "--columns=application,installation",
    ]).await.output;
    let system_runtime = nvidia_extension.as_deref()
        .is_some_and(|extension| flatpak_in_scope(&runtime_scopes, extension, "system"));
    let user_runtime = nvidia_extension.as_deref()
        .is_some_and(|extension| flatpak_in_scope(&runtime_scopes, extension, "user"));
    let obs_runtime_matches = match obs_scope {
        FlatpakScope::System => system_runtime,
        FlatpakScope::User => user_runtime,
        FlatpakScope::NotInstalled => false,
    };
    // `-x`는 15자로 잘린 프로세스 이름에 매칭되지 않는다 — `-f`로 실행 명령행을 본다.
    let active = runner::run("pgrep", &["-f", "gpu-screen-recorder -w"]).await.success;
    let shortcut_registered = shortcuts_config_path().is_some_and(|path| {
        recording_shortcut_registered(&path)
    });
    let recording = RecordingStatus {
        gsr_scope,
        system_flathub,
        nvidia_extension,
        system_runtime,
        user_runtime,
        obs_scope,
        obs_runtime_matches,
        active,
        shortcut_registered,
        output_dir: recording_output_dir(),
    };

    let installed = runner::run("which", &["mpv"]).await.success;
    let vainfo_installed = runner::run("which", &["vainfo"]).await.success;
    let vaapi = if vainfo_installed {
        let vainfo = runner::run("vainfo", &[]).await;
        if vainfo.output.contains("VAEntrypointVLD") {
            VaApiStatus::Supported
        } else {
            VaApiStatus::Unsupported
        }
    } else {
        VaApiStatus::Unknown
    };
    let default_player = runner::run("xdg-mime", &["query", "default", "video/mp4"]).await
        .output.lines().next().map(|line| line.trim().to_string()).filter(|line| !line.is_empty());
    let mpv = MpvStatus { installed, vaapi, default_player };

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
        recording,
        mpv,
        packages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::cosmic_tweaks::replace_shortcut_entries;

    const SEVEN_SHORTCUTS: &str = r#"{
    (modifiers: [Ctrl], key: "F1"): Spawn("one"),
    (modifiers: [Ctrl], key: "F2"): Spawn("two"),
    (modifiers: [Ctrl], key: "F3"): Spawn("three"),
    (modifiers: [Ctrl], key: "F4"): Spawn("four"),
    (modifiers: [Ctrl], key: "F5"): Spawn("five"),
    (modifiers: [Ctrl], key: "F6"): Spawn("six"),
    (modifiers: [Ctrl], key: "F7"): Spawn("seven"),
}"#;

    #[test]
    fn video_mime_types_have_nine_unique_entries() {
        let unique: std::collections::HashSet<_> = VIDEO_MIME_TYPES.iter().collect();
        assert_eq!(VIDEO_MIME_TYPES.len(), 9);
        assert_eq!(unique.len(), VIDEO_MIME_TYPES.len());
    }

    #[test]
    fn xdg_mime_default_args_include_every_video_mime_and_one_desktop_entry() {
        let args = xdg_mime_default_args();
        assert_eq!(args.iter().filter(|arg| **arg == "mpv.desktop").count(), 1);
        for mime_type in VIDEO_MIME_TYPES {
            assert!(args.contains(&mime_type), "missing {mime_type}");
        }
    }

    #[test]
    fn mpv_player_state_is_not_installed_without_mpv() {
        assert_eq!(mpv_player_state(false, false), PlayerState::NotInstalled);
    }

    #[test]
    fn mpv_player_state_recommends_default_when_mpv_is_not_selected() {
        assert_eq!(mpv_player_state(true, false), PlayerState::InstalledNotDefault);
    }

    #[test]
    fn mpv_player_state_is_default_when_mpv_is_selected() {
        assert_eq!(mpv_player_state(true, true), PlayerState::Default);
    }

    #[test]
    fn determines_gsr_install_scope_from_flatpak_list_fixtures() {
        assert_eq!(flatpak_scope("com.dec05eba.gpu_screen_recorder\tsystem", GSR_APP), FlatpakScope::System);
        assert_eq!(flatpak_scope("com.dec05eba.gpu_screen_recorder\tuser", GSR_APP), FlatpakScope::User);
        assert_eq!(flatpak_scope("com.obsproject.Studio\tuser", GSR_APP), FlatpakScope::NotInstalled);
    }

    #[test]
    fn converts_host_nvidia_version_to_runtime_extension() {
        assert_eq!(
            nvidia_runtime_extension("580.173.02\n").as_deref(),
            Some("org.freedesktop.Platform.GL.nvidia-580-173-02"),
        );
    }

    #[test]
    fn install_fix_script_orders_user_uninstall_remote_system_install_and_obs_runtime() {
        let extension = "org.freedesktop.Platform.GL.nvidia-580-173-02";
        let script = recording_install_script(FlatpakScope::User, false, FlatpakScope::User, extension);
        let uninstall = script.find("flatpak uninstall --user -y --noninteractive").unwrap();
        let remote = script.find("flatpak remote-add --system --if-not-exists flathub").unwrap();
        let system = script.find("flatpak install --system -y --noninteractive flathub").unwrap();
        let user = script.rfind("flatpak install --user -y --noninteractive flathub").unwrap();
        assert!(uninstall < remote && remote < system && system < user);
        assert!(script.contains(&format!("{GSR_APP} {extension}")));
        assert!(script.contains(&format!("flathub {extension}")));
    }

    #[test]
    fn toggle_start_command_contains_every_required_gsr_flag() {
        let command = recording_start_args(Path::new("/tmp/Recordings/Recording_x.mp4")).join(" ");
        for flag in [
            "--command=gpu-screen-recorder", "-w portal", "-restore-portal-session yes",
            "-f 60", "-k h264", "-encoder gpu", "-fallback-cpu-encoding no",
            "-a default_output", "-ac opus", "-c mp4", "-cursor yes",
            "-o /tmp/Recordings/Recording_x.mp4",
        ] {
            assert!(command.contains(flag), "missing {flag}");
        }
        // `-ro`는 리플레이 모드 전용이라 일반 녹화에서는 파일이 생성되지 않는다(회귀 방지).
        assert!(!command.contains("-ro "), "must not use -ro for regular recording: {command}");
    }

    #[test]
    fn recording_output_file_has_mp4_extension_under_the_given_directory() {
        let dir = Path::new("/tmp/Recordings");
        let file = recording_output_file(dir);
        assert_eq!(file.parent(), Some(dir));
        assert_eq!(file.extension().and_then(|e| e.to_str()), Some("mp4"));
        assert!(file.file_name().unwrap().to_str().unwrap().starts_with("Recording_"));
    }

    #[test]
    fn civil_datetime_string_matches_known_unix_time() {
        // 2026-09-14 02:59:51 UTC = 1789354791 (date -u 로 실측 대조)
        assert_eq!(civil_datetime_string(1789354791), "2026-09-14_02-59-51");
    }

    #[test]
    fn pgrep_pattern_matches_the_actual_gsr_process_command_line() {
        // 2026-09-14 실측(포털 캡처 8초 녹화): bwrap 이 실행하는 실제 gsr 프로세스의
        // /proc/<pid>/comm 은 "gpu-screen-reco"(15자 절단, pgrep -x 로는 매칭 불가)이지만
        // /proc/<pid>/cmdline 은 "gpu-screen-recorder -w portal ..." 그대로다.
        // flatpak 실행 래퍼(`flatpak run --command=gpu-screen-recorder com.dec05eba...`)에는
        // "gpu-screen-recorder" 뒤에 " -w" 가 오지 않으므로 -f 패턴이 래퍼를 오매칭하지 않는다.
        let real_gsr_cmdline =
            "gpu-screen-recorder -w portal -restore-portal-session yes -f 30 -o /tmp/x.mp4";
        let flatpak_wrapper =
            "flatpak run --command=gpu-screen-recorder com.dec05eba.gpu_screen_recorder -w portal";
        const PATTERN: &str = "gpu-screen-recorder -w";
        assert!(real_gsr_cmdline.contains(PATTERN));
        assert!(!flatpak_wrapper.contains(PATTERN));
    }

    #[test]
    fn adds_ctrl_shift_six_without_reordering_or_reformatting_existing_shortcuts() {
        let updated = replace_shortcut_entries(
            SEVEN_SHORTCUTS,
            &[("6", "/absolute/popmgr --record-toggle".into())],
        );
        let described = add_recording_shortcut_description(&updated, "/absolute/popmgr --record-toggle");
        assert!(described.starts_with(&SEVEN_SHORTCUTS[..SEVEN_SHORTCUTS.len() - 2]));
        assert!(described.contains("key: \"6\", description: Some(\"popmgr 화면 녹화 토글\")"));
        assert_eq!(parse_shortcut_entries(&described).len(), 8);
    }

    #[test]
    fn newest_file_selection_picks_latest_file_in_temp_directory() {
        let root = std::env::temp_dir().join(format!("popmgr-recording-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let older = root.join("older.mp4");
        let newer = root.join("newer.mp4");
        std::fs::write(&older, "old").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&newer, "new").unwrap();
        assert_eq!(newest_file(&root).as_deref(), Some(newer.as_path()));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
