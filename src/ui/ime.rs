use iced::{
    widget::{button, column, container, row, scrollable, text, Space},
    Color, Element, Length, Task,
};
use crate::runner::{self, CmdResult};

// 참고: https://github.com/Hostingglobal-Tech/cosmic-os-korean
// /etc/environment에 설정하는 환경변수

#[derive(Debug, Clone, PartialEq)]
pub enum ImeKind {
    Ibus,
    Fcitx5,
    Kime,
}

impl ImeKind {
    fn label(&self) -> &str {
        match self {
            ImeKind::Ibus   => "ibus",
            ImeKind::Fcitx5 => "fcitx5",
            ImeKind::Kime   => "kime",
        }
    }
    fn pkg(&self) -> &[&'static str] {
        match self {
            ImeKind::Ibus   => &["ibus", "ibus-hangul"],
            ImeKind::Fcitx5 => &[
                "fcitx5", "fcitx5-hangul",
                "fcitx5-frontend-gtk3", "fcitx5-frontend-gtk4",
                "fcitx5-frontend-qt5", "fcitx5-frontend-qt6",
            ],
            ImeKind::Kime   => &[],  // GitHub Release에서 설치
        }
    }
    fn env_lines(&self) -> &[(&'static str, &'static str)] {
        match self {
            ImeKind::Ibus => &[
                ("GTK_IM_MODULE", "ibus"),
                ("QT_IM_MODULE", "ibus"),
                ("XMODIFIERS", "@im=ibus"),
                ("INPUT_METHOD", "ibus"),
            ],
            ImeKind::Fcitx5 => &[
                ("GTK_IM_MODULE", "fcitx"),
                ("QT_IM_MODULE", "fcitx"),
                ("XMODIFIERS", "@im=fcitx"),
                ("INPUT_METHOD", "fcitx5"),
            ],
            ImeKind::Kime => &[
                ("GTK_IM_MODULE", "kime"),
                ("QT_IM_MODULE", "kime"),
                ("XMODIFIERS", "@im=kime"),
                ("INPUT_METHOD", "kime"),
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShellInitConflict {
    pub path: String,        // ex: /home/dell/.profile
    pub lines: Vec<String>,  // 충돌하는 export 라인들 (원본 그대로)
}

#[derive(Debug, Clone)]
pub struct JetBrainsVmOptions {
    pub name: String,         // ex: IntelliJIdea2026.1
    pub path: String,         // 절대경로 (idea64.vmoptions 등)
    pub has_xtoolkit: bool,
    pub has_recreate_xim: bool,
}

/// fcitx5 전역 설정(~/.config/fcitx5/config)의 한/영 상태 정책.
/// fcitx5 기본값(ShareInputState=No, ActiveByDefault=False)은 창(입력 컨텍스트)마다
/// 상태를 따로 기억하고 새 컨텍스트를 영문으로 시작하므로, 다른 창에 갔다 오거나
/// 데몬이 재시작되면 "영문만 입력되는" 증상이 된다.
#[derive(Debug, Clone, PartialEq)]
pub struct Fcitx5Behavior {
    pub share_input_state: String, // "All" 이어야 모든 창이 한/영 상태를 공유
    pub active_by_default: bool,   // true 여야 새 창이 한글로 시작
    pub shift_alt_trigger: bool,   // AltTriggerKeys=Shift_L: 왼쪽 Shift 단독 탭이 영문 전환
}

impl Fcitx5Behavior {
    fn is_ok(&self) -> bool {
        self.share_input_state == "All" && self.active_by_default && !self.shift_alt_trigger
    }
}

/// 툴킷별 fcitx5 IM 모듈. GTK_IM_MODULE/QT_IM_MODULE=fcitx 인데 모듈 패키지가 없으면
/// 그 툴킷 앱은 fcitx5에 붙지 못해 한글 입력이 안 되거나 불안정하다.
const FCITX5_FRONTEND_PKGS: &[(&str, &str)] = &[
    ("fcitx5-frontend-gtk3", "GTK3 (Chrome, Firefox 등)"),
    ("fcitx5-frontend-gtk4", "GTK4"),
    ("fcitx5-frontend-qt5",  "Qt5 (VLC, OBS 등)"),
    ("fcitx5-frontend-qt6",  "Qt6"),
];

#[derive(Debug, Clone)]
pub struct ImeStatus {
    pub installed_ibus: bool,
    pub installed_fcitx5: bool,
    pub installed_kime: bool,
    pub active: Option<ImeKind>,
    pub daemon_running: Option<ImeKind>,
    pub env_match: bool,
    // 추가 진단: 활성 IME와 불일치하는 사용자 셸 init 파일의 export 라인들
    pub shell_init_conflicts: Vec<ShellInitConflict>,
    // 누출된 GTK_IM_MODULE_FILE (snap 등) — Some이면 비정상
    pub snap_im_module_file: Option<String>,
    // 발견된 JetBrains IDE vmoptions (IME 옵션 누락 여부 포함)
    pub jetbrains_ides: Vec<JetBrainsVmOptions>,
    // 절전 복귀 시 IME 데몬을 자동 재시작하는 system-sleep 훅 설치 여부
    pub resume_hook_installed: bool,
    // LibreOffice가 Wayland에서 fcitx GTK 모듈을 실제로 연결했는지와
    // 사용자 범위 X11 호환 런처 설치 여부
    pub libreoffice_installed: bool,
    pub libreoffice_running_wayland: bool,
    pub libreoffice_fcitx_module_loaded: bool,
    pub libreoffice_compat_installed: bool,
    // fcitx5 전역 설정의 한/영 상태 정책 (fcitx5 활성일 때만 Some)
    pub fcitx5_behavior: Option<Fcitx5Behavior>,
    // fcitx5 활성인데 미설치인 툴킷 프론트엔드(IM 모듈) 패키지
    pub fcitx5_missing_frontends: Vec<&'static str>,
}

#[derive(Debug, Clone)]
pub enum ImeMsg {
    Refresh,
    Refreshed(ImeStatus),
    AutoReconnect(ImeKind),
    Select(ImeKind),
    Install(ImeKind),
    Apply,
    Watchdog,
    Noop,
    CleanShellInits,
    PatchJetBrains,
    InstallResumeHook,
    UninstallResumeHook,
    InstallLibreOfficeCompat,
    UninstallLibreOfficeCompat,
    FixFcitx5Behavior,
    InstallFcitx5Frontends,
    RestartDaemon,
    Done(CmdResult),
}

pub struct ImeState {
    pub status: Option<ImeStatus>,
    pub selected: ImeKind,
    pub running: Option<String>,
    /// 시작 시 자동 재연결은 1회만 (매 스캔마다 데몬을 재시작하는 루프 방지)
    auto_reconnected: bool,
}

impl ImeState {
    pub fn new() -> Self {
        Self {
            status: None,
            selected: ImeKind::Kime,
            running: None,
            auto_reconnected: false,
        }
    }

    pub fn update(&mut self, msg: ImeMsg) -> (Task<ImeMsg>, Option<CmdResult>) {
        match msg {
            ImeMsg::Refresh => {
                let task = Task::perform(async { scan_ime_status().await }, ImeMsg::Refreshed);
                (task, None)
            }
            ImeMsg::Refreshed(s) => {
                if let Some(ref active) = s.active {
                    self.selected = active.clone();
                }
                // 시작 시 1회: 데몬이 죽어 있을 때만 활성 IME를 자동 기동한다.
                // 실행 중인 데몬을 재시작하면 기존 X11/XIM 클라이언트(ChatGPT 등)의
                // 입력 컨텍스트가 끊겨 재실행 전까지 한/영 전환이 멈출 수 있다.
                let reconnect_task = if !self.auto_reconnected {
                    self.auto_reconnected = true;
                    let kind_opt = startup_daemon_to_start(
                        s.active.as_ref(),
                        s.daemon_running.as_ref(),
                    );
                    if let Some(k) = kind_opt {
                        Task::perform(
                            async move {
                                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                                k
                            },
                            ImeMsg::AutoReconnect,
                        )
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                };
                self.status = Some(s);
                (reconnect_task, None)
            }
            ImeMsg::AutoReconnect(kind) => {
                let cmd = daemon_restart_cmd(&kind);
                let dbus_keys = kind.env_lines().iter().map(|(k,_)| *k).collect::<Vec<_>>().join(" ");
                let full = format!("{cmd}; dbus-update-activation-environment --systemd {dbus_keys} 2>/dev/null");
                let t = Task::perform(
                    async move { runner::run_sh(&full).await },
                    |_| ImeMsg::Refresh,
                );
                (t, None)
            }
            ImeMsg::Select(k) => {
                self.selected = k;
                (Task::none(), None)
            }
            // 30초 주기 워치독: 활성 IME 데몬이 죽어 있으면 자동 재기동
            ImeMsg::Watchdog => {
                let kind = self.status.as_ref()
                    .and_then(|s| s.active.clone())
                    .unwrap_or_else(|| self.selected.clone());
                let bin: &'static str = match &kind {
                    ImeKind::Kime => "kime",
                    ImeKind::Ibus => "ibus-daemon",
                    ImeKind::Fcitx5 => "fcitx5",
                };
                let t = Task::perform(
                    async move {
                        let alive = runner::run("pgrep", &["-x", bin]).await.success;
                        (kind, alive)
                    },
                    |(k, alive)| if alive { ImeMsg::Noop } else { ImeMsg::AutoReconnect(k) },
                );
                (t, None)
            }
            ImeMsg::Noop => (Task::none(), None),
            ImeMsg::Install(k) => {
                let pkgs = k.pkg().to_vec();
                if pkgs.is_empty() {
                    let res = CmdResult {
                        success: false,
                        output: "kime는 GitHub Releases에서 직접 설치:\nhttps://github.com/Riey/kime/releases".into(),
                    };
                    return (Task::none(), Some(res));
                }
                self.running = Some(format!("{} 설치 중...", k.label()));
                let script = format!("pkexec apt-get install -y {}", pkgs.join(" "));
                let task = Task::perform(
                    async move { runner::run_sh(&script).await },
                    ImeMsg::Done,
                );
                (task, None)
            }
            ImeMsg::Apply => {
                self.running = Some(format!("{} 적용 중...", self.selected.label()));
                let kind = self.selected.clone();
                let task = Task::perform(
                    async move { apply_ime(kind).await },
                    ImeMsg::Done,
                );
                (task, None)
            }
            ImeMsg::CleanShellInits => {
                let conflicts = self.status.as_ref()
                    .map(|s| s.shell_init_conflicts.clone())
                    .unwrap_or_default();
                let active = self.status.as_ref().and_then(|s| s.active.clone())
                    .unwrap_or(self.selected.clone());
                self.running = Some("셸 init 파일 정리 중...".into());
                let task = Task::perform(
                    async move { clean_shell_inits(conflicts, active).await },
                    ImeMsg::Done,
                );
                (task, None)
            }
            ImeMsg::PatchJetBrains => {
                let ides = self.status.as_ref()
                    .map(|s| s.jetbrains_ides.clone())
                    .unwrap_or_default();
                self.running = Some("JetBrains vmoptions 패치 중...".into());
                let task = Task::perform(
                    async move { patch_jetbrains_vmoptions(ides).await },
                    ImeMsg::Done,
                );
                (task, None)
            }
            ImeMsg::InstallResumeHook => {
                self.running = Some("Resume 훅 설치 중...".into());
                let task = Task::perform(
                    async { install_resume_hook().await },
                    ImeMsg::Done,
                );
                (task, None)
            }
            ImeMsg::UninstallResumeHook => {
                self.running = Some("Resume 훅 제거 중...".into());
                let task = Task::perform(
                    async { uninstall_resume_hook().await },
                    ImeMsg::Done,
                );
                (task, None)
            }
            ImeMsg::InstallLibreOfficeCompat => {
                self.running = Some("LibreOffice 한글 입력 호환 모드 설치 중...".into());
                let task = Task::perform(
                    async { install_libreoffice_compat().await },
                    ImeMsg::Done,
                );
                (task, None)
            }
            ImeMsg::UninstallLibreOfficeCompat => {
                self.running = Some("LibreOffice 한글 입력 호환 모드 제거 중...".into());
                let task = Task::perform(
                    async { uninstall_libreoffice_compat().await },
                    ImeMsg::Done,
                );
                (task, None)
            }
            ImeMsg::FixFcitx5Behavior => {
                self.running = Some("fcitx5 한/영 상태 유지 설정 적용 중...".into());
                let task = Task::perform(
                    async { fix_fcitx5_behavior().await },
                    ImeMsg::Done,
                );
                (task, None)
            }
            ImeMsg::InstallFcitx5Frontends => {
                let pkgs = self.status.as_ref()
                    .map(|s| s.fcitx5_missing_frontends.clone())
                    .unwrap_or_default();
                self.running = Some("fcitx5 프론트엔드 설치 중...".into());
                let task = Task::perform(
                    async move { install_fcitx5_frontends(pkgs).await },
                    ImeMsg::Done,
                );
                (task, None)
            }
            // pkexec 없이 데몬만 재시작 ("적용"은 /etc/environment 재작성까지 하므로 매번 비밀번호를 묻는다)
            ImeMsg::RestartDaemon => {
                let kind = self.status.as_ref()
                    .and_then(|s| s.active.clone())
                    .unwrap_or_else(|| self.selected.clone());
                self.running = Some(format!("{} 재시작 중...", kind.label()));
                let task = Task::perform(
                    async move { restart_ime_daemon(kind).await },
                    ImeMsg::Done,
                );
                (task, None)
            }
            ImeMsg::Done(r) => {
                self.running = None;
                let refresh = Task::perform(async { scan_ime_status().await }, ImeMsg::Refreshed);
                (refresh, Some(r))
            }
        }
    }

    pub fn view(&self) -> Element<'_, ImeMsg> {
        let is_running = self.running.is_some();

        let mut col = column![
            text("한글 입력기 (IME)").size(20),
            Space::with_height(6),
            text("참고: cosmic-os-korean 패치 권장 — COSMIC에서는 kime가 가장 안정적입니다.")
                .size(11)
                .color(C_DIM),
            Space::with_height(16),
        ];

        if let Some(label) = &self.running {
            col = col
                .push(running_bar(label))
                .push(Space::with_height(12));
        }

        let imes = [
            (ImeKind::Ibus,   self.status.as_ref().map(|s| s.installed_ibus).unwrap_or(false)),
            (ImeKind::Fcitx5, self.status.as_ref().map(|s| s.installed_fcitx5).unwrap_or(false)),
            (ImeKind::Kime,   self.status.as_ref().map(|s| s.installed_kime).unwrap_or(false)),
        ];

        for (kind, installed) in imes {
            let active = self.selected == kind;
            col = col
                .push(ime_row(kind, installed, active, is_running))
                .push(Space::with_height(6));
        }

        if let Some(st) = &self.status {
            col = col.push(Space::with_height(12));
            let env_txt = if st.env_match { "[OK] /etc/environment 일치" } else { "[!] /etc/environment 불일치" };
            let env_col = if st.env_match { C_OK } else { C_ERR };
            col = col.push(text(env_txt).size(12).color(env_col));
            col = col.push(Space::with_height(4));
            let daemon_txt = match &st.daemon_running {
                Some(k) => format!("[실행] {} 데몬 실행 중", k.label()),
                None    => "[중지] 데몬 미실행".into(),
            };
            let daemon_col = if st.daemon_running.is_some() { C_OK } else { C_WARN };
            col = col.push(text(daemon_txt).size(12).color(daemon_col));

            // ── fcitx5 전용 진단 ──────────────────────────────
            if let Some(ref beh) = st.fcitx5_behavior {
                col = col.push(Space::with_height(14));
                col = col.push(fcitx5_behavior_card(beh, is_running));
            }
            if !st.fcitx5_missing_frontends.is_empty() {
                col = col.push(Space::with_height(14));
                col = col.push(fcitx5_frontend_card(&st.fcitx5_missing_frontends, is_running));
            }

            col = col.push(Space::with_height(14));
            col = col.push(resume_hook_card(st.resume_hook_installed, is_running));

            if st.libreoffice_installed {
                col = col.push(Space::with_height(14));
                col = col.push(libreoffice_ime_card(st, is_running));
            }

            // ── 추가 진단 ─────────────────────────────────────
            if !st.shell_init_conflicts.is_empty() {
                col = col.push(Space::with_height(14));
                col = col.push(shell_conflict_card(&st.shell_init_conflicts, is_running));
            }
            if let Some(ref leak) = st.snap_im_module_file {
                col = col.push(Space::with_height(10));
                col = col.push(snap_leak_card(leak));
            }
            if !st.jetbrains_ides.is_empty() {
                let needs_patch = st.jetbrains_ides.iter()
                    .any(|i| !i.has_xtoolkit || !i.has_recreate_xim);
                if needs_patch {
                    col = col.push(Space::with_height(10));
                    col = col.push(jetbrains_card(&st.jetbrains_ides, is_running));
                }
            }
        }

        col = col.push(Space::with_height(20));
        col = col.push(
            container(
                row![
                    Space::with_width(Length::Fill),
                    action_btn("IME 재시작", ImeMsg::RestartDaemon, !is_running, C_DIM),
                    Space::with_width(8),
                    action_btn("적용", ImeMsg::Apply, !is_running, C_BLUE),
                ]
            )
        );

        scrollable(
            container(col).padding([4, 0])
        )
        .into()
    }
}

fn startup_daemon_to_start(
    active: Option<&ImeKind>,
    daemon_running: Option<&ImeKind>,
) -> Option<ImeKind> {
    if daemon_running.is_none() {
        active.cloned()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{patch_libreoffice_desktop, startup_daemon_to_start, ImeKind, LO_COMPAT_MARKER};

    #[test]
    fn startup_keeps_a_running_daemon_alive() {
        assert_eq!(
            startup_daemon_to_start(Some(&ImeKind::Fcitx5), Some(&ImeKind::Fcitx5)),
            None,
        );
    }

    #[test]
    fn startup_starts_the_active_daemon_when_missing() {
        assert_eq!(
            startup_daemon_to_start(Some(&ImeKind::Fcitx5), None),
            Some(ImeKind::Fcitx5),
        );
    }

    #[test]
    fn libreoffice_desktop_patch_updates_every_exec_entry() {
        let original = "[Desktop Entry]\nExec=libreoffice --writer %U\n[Desktop Action New]\nExec=libreoffice --writer\n";
        let patched = patch_libreoffice_desktop(original);
        assert!(patched.starts_with(LO_COMPAT_MARKER));
        assert_eq!(
            patched.matches(
                "Exec=env -u WAYLAND_DISPLAY GDK_BACKEND=x11 GTK_IM_MODULE=xim libreoffice"
            ).count(),
            2,
        );
    }

    #[test]
    fn libreoffice_desktop_patch_does_not_double_prefix() {
        let original = "Exec=env -u WAYLAND_DISPLAY GDK_BACKEND=x11 GTK_IM_MODULE=xim libreoffice --writer %U\n";
        let patched = patch_libreoffice_desktop(original);
        assert_eq!(patched.matches("GDK_BACKEND=x11").count(), 1);
    }
}

fn ime_row(kind: ImeKind, installed: bool, selected: bool, disabled: bool) -> Element<'static, ImeMsg> {
    let sel_color = if selected { C_SEL_BG } else { C_SURFACE };
    let border_color = if selected { C_BLUE } else { C_BORDER };

    let status_txt = if installed { "설치됨" } else { "미설치" };
    let status_col = if installed { C_OK } else { C_DIM };

    let install_btn: Element<'static, ImeMsg> = if !installed {
        action_btn("설치", ImeMsg::Install(kind.clone()), !disabled, C_GREEN)
    } else {
        Space::with_width(0).into()
    };

    let radio_bg = if selected { C_BLUE } else { C_SURFACE };
    let radio_border = if selected { C_BLUE } else { C_DIM };
    let radio = container(Space::new(10, 10))
        .width(14).height(14)
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(radio_bg)),
            border: iced::Border { radius: 7.0.into(), color: radio_border, width: 2.0 },
            ..Default::default()
        });

    let label_str = kind.label().to_string();
    let select_msg = ImeMsg::Select(kind);
    let row_inner = row![
        radio,
        Space::with_width(10),
        column![
            text(label_str).size(14).color(if selected { C_BLUE } else { C_TEXT }),
            text(status_txt).size(11).color(status_col),
        ],
        Space::with_width(Length::Fill),
        install_btn,
    ]
    .align_y(iced::Alignment::Center);

    button(
        container(row_inner).padding([12, 16])
    )
    .width(Length::Fill)
    .on_press(select_msg)
    .style(move |_, _| iced::widget::button::Style {
        background: Some(iced::Background::Color(sel_color)),
        border: iced::Border { radius: 12.0.into(), color: border_color, width: if selected { 1.5 } else { 1.0 } },
        text_color: C_TEXT,
        ..Default::default()
    })
    .into()
}

async fn scan_ime_status() -> ImeStatus {
    let installed_ibus   = pkg_installed("ibus").await && pkg_installed("ibus-hangul").await;
    let installed_fcitx5 = pkg_installed("fcitx5").await && pkg_installed("fcitx5-hangul").await;
    let installed_kime   = which_exists("kime").await;

    let active = read_active_ime().await;
    let daemon_running = running_ime_daemon().await;

    let env_match = if let Some(ref a) = active {
        check_env_match(a).await
    } else {
        false
    };

    // 진단: 셸 init 파일에 활성 IME와 모순되는 export 가 있는지
    let shell_init_conflicts = if let Some(ref a) = active {
        scan_shell_init_conflicts(a).await
    } else {
        Vec::new()
    };

    // 진단: 현재 환경의 GTK_IM_MODULE_FILE 누출 (snap 등)
    let snap_im_module_file = detect_snap_im_leak();

    // 진단: 발견된 JetBrains IDE vmoptions
    let jetbrains_ides = find_jetbrains_vmoptions().await;

    // 진단: 절전 복귀 IME 재시작 훅 설치 여부
    let resume_hook_installed = detect_resume_hook();

    // 진단: fcitx5 전역 설정의 한/영 상태 정책 + 툴킷 프론트엔드 누락
    let (fcitx5_behavior, fcitx5_missing_frontends) = if active == Some(ImeKind::Fcitx5) {
        let content = tokio::fs::read_to_string(fcitx5_config_path()).await.unwrap_or_default();
        let mut missing = Vec::new();
        for (pkg, _) in FCITX5_FRONTEND_PKGS {
            if !pkg_installed(pkg).await {
                missing.push(*pkg);
            }
        }
        (Some(parse_fcitx5_behavior(&content)), missing)
    } else {
        (None, Vec::new())
    };

    let libreoffice_installed = which_exists("libreoffice").await;
    let (libreoffice_running_wayland, libreoffice_fcitx_module_loaded) =
        detect_running_libreoffice_ime();
    let libreoffice_compat_installed = detect_libreoffice_compat();

    ImeStatus {
        installed_ibus, installed_fcitx5, installed_kime,
        active, daemon_running, env_match,
        shell_init_conflicts, snap_im_module_file, jetbrains_ides,
        resume_hook_installed,
        libreoffice_installed, libreoffice_running_wayland,
        libreoffice_fcitx_module_loaded, libreoffice_compat_installed,
        fcitx5_behavior, fcitx5_missing_frontends,
    }
}

const IME_KEYS: &[&str] = &[
    "GTK_IM_MODULE", "QT_IM_MODULE", "XMODIFIERS",
    "SDL_IM_MODULE", "GLFW_IM_MODULE", "INPUT_METHOD",
];

fn shell_init_paths() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    [
        ".profile", ".bash_profile", ".bashrc",
        ".zprofile", ".zshrc",
    ].iter().map(|f| format!("{home}/{f}")).collect()
}

async fn scan_shell_init_conflicts(active: &ImeKind) -> Vec<ShellInitConflict> {
    let expected: std::collections::HashMap<&str, &str> = active.env_lines()
        .iter().cloned().collect();

    let mut out = Vec::new();
    for path in shell_init_paths() {
        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut bad_lines = Vec::new();
        for raw in content.lines() {
            let line = raw.trim_start();
            if line.starts_with('#') { continue; }
            // export KEY=VAL  또는  KEY=VAL
            let body = line.strip_prefix("export ").unwrap_or(line);
            let Some(eq) = body.find('=') else { continue };
            let key = &body[..eq];
            if !IME_KEYS.contains(&key) { continue; }
            let val = body[eq+1..].trim().trim_matches('"').trim_matches('\'');
            let expected_val = expected.get(key);
            // 활성 IME의 기대값과 다르면 충돌로 간주
            if expected_val.map(|e| *e != val).unwrap_or(false) {
                bad_lines.push(raw.to_string());
            }
        }
        if !bad_lines.is_empty() {
            out.push(ShellInitConflict { path, lines: bad_lines });
        }
    }
    out
}

// ── Resume 복구 훅 ─────────────────────────────────────────────
// systemd-sleep post 훅: 절전 복귀 시 활성 IME 데몬을 재시작해
// Wayland im 소켓 / DBus 채널이 끊긴 상태를 자동 복구한다.
// 유휴 비용 0 (resume 이벤트 시 1회 실행).

const RESUME_HOOK_PATH: &str = "/etc/systemd/system-sleep/zz-popmgr-ime-restart";

// v2 변경점 (v1은 DISPLAY=:0 하드코딩 + WAYLAND_DISPLAY/DBUS 미설정이라
// 세션이 :1/wayland-1 인 머신에서는 재시작된 데몬이 세션에 연결되지 못했음):
//  - 재시작 대상 데몬의 /proc/PID/environ 에서 실제 세션 env를 읽고,
//    소켓 존재 확인으로 검증 후 실패 시 소켓 탐색으로 폴백.
//  - systemd-sleep 훅은 완료까지 resume을 블록하므로 setsid로 완전 분리 후
//    백그라운드에서 1초 대기(컴포지터 안정화) 후 재시작.
const RESUME_HOOK_SCRIPT: &str = r#"#!/bin/sh
# popmgr-resume-ime-hook: v2
# Restart active IME daemon after wake to recover Korean input.
# Discovers the real session env (WAYLAND_DISPLAY / DISPLAY / DBUS) at run
# time from the running daemon's /proc environ + socket probing, instead of
# hardcoding DISPLAY=:0 (v1 bug: broke whenever the session was :1/wayland-1).

[ "$1" = "post" ] || exit 0
case "$2" in
  suspend|hibernate|hybrid-sleep|suspend-then-hibernate) ;;
  *) exit 0 ;;
esac

# proc_env <pid> <VAR>: read one variable from a process's environment
proc_env() {
  tr '\0' '\n' < "/proc/$1/environ" 2>/dev/null | sed -n "s/^$2=//p" | head -n 1
}

for d in /run/user/[0-9]*; do
  uid="${d##*/}"
  [ "$uid" -ge 1000 ] 2>/dev/null || continue
  user="$(getent passwd "$uid" | cut -d: -f1)"
  [ -n "$user" ] || continue

  for daemon in fcitx5 ibus-daemon kime; do
    pid="$(pgrep -x -o -u "$uid" "$daemon" 2>/dev/null)" || continue
    case "$daemon" in
      fcitx5)      cmd="fcitx5 -d --replace" ;;
      ibus-daemon) cmd="ibus-daemon -drxR" ;;
      kime)        cmd="pkill -x kime; sleep 0.2; setsid kime" ;;
    esac

    # WAYLAND_DISPLAY: daemon env 우선, 소켓 검증 실패 시 런타임 디렉토리 탐색
    # (wayland-1-renderD128 같은 부속 소켓/락파일은 제외)
    wl="$(proc_env "$pid" WAYLAND_DISPLAY)"
    if [ -n "$wl" ] && [ ! -S "$d/$wl" ]; then wl=""; fi
    if [ -z "$wl" ]; then
      for s in "$d"/wayland-*; do
        n="${s##*/}"
        case "$n" in wayland-*[!0-9]*) continue ;; esac
        if [ -S "$s" ]; then wl="$n"; break; fi
      done
    fi

    # DISPLAY: daemon env 우선, X 소켓 검증 실패 시 /tmp/.X11-unix 탐색
    disp="$(proc_env "$pid" DISPLAY)"
    case "$disp" in
      :*) num="${disp#:}"; num="${num%%.*}"
          [ -S "/tmp/.X11-unix/X$num" ] || disp="" ;;
      *)  disp="" ;;
    esac
    if [ -z "$disp" ]; then
      for x in /tmp/.X11-unix/X[0-9]*; do
        if [ -S "$x" ]; then disp=":${x##*/X}"; break; fi
      done
    fi

    # D-Bus 세션 버스: daemon env 우선, 없으면 XDG 표준 사용자 버스 소켓
    dbus="$(proc_env "$pid" DBUS_SESSION_BUS_ADDRESS)"
    if [ -z "$dbus" ] && [ -S "$d/bus" ]; then dbus="unix:path=$d/bus"; fi

    set -- XDG_RUNTIME_DIR="$d"
    [ -n "$wl" ]   && set -- "$@" WAYLAND_DISPLAY="$wl"
    [ -n "$disp" ] && set -- "$@" DISPLAY="$disp"
    [ -n "$dbus" ] && set -- "$@" DBUS_SESSION_BUS_ADDRESS="$dbus"

    # setsid로 완전 분리 + 1초 대기 후 재시작: systemd는 모든 훅이 끝나야
    # resume을 완료하므로 훅 자체는 즉시 반환해야 한다.
    runuser -u "$user" -- env "$@" \
      sh -c "setsid sh -c 'sleep 1; $cmd' </dev/null >/dev/null 2>&1 &"
    break
  done
done
"#;

fn detect_resume_hook() -> bool {
    // 현재 버전 마커까지 일치해야 "설치됨"으로 본다.
    // 구버전(v1: DISPLAY=:0 하드코딩)은 미설치로 표시해 재설치를 유도.
    std::fs::read_to_string(RESUME_HOOK_PATH)
        .map(|c| c.contains("# popmgr-resume-ime-hook: v2"))
        .unwrap_or(false)
}

async fn install_resume_hook() -> CmdResult {
    let tmp = "/tmp/popmgr-resume-ime-hook.sh";
    if let Err(e) = tokio::fs::write(tmp, RESUME_HOOK_SCRIPT).await {
        return CmdResult { success: false, output: format!("임시 파일 쓰기 실패: {e}") };
    }
    // 디렉토리가 없는 배포판이 있음 (Pop!_OS 등) → mkdir -p 후 install
    let dir = std::path::Path::new(RESUME_HOOK_PATH).parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/etc/systemd/system-sleep".to_string());
    let cmd = format!(
        "pkexec sh -c 'mkdir -p {dir} && install -m 755 -o root -g root {tmp} {dst}'",
        dir = dir,
        tmp = tmp,
        dst = RESUME_HOOK_PATH,
    );
    let r = runner::run_sh(&cmd).await;
    let _ = tokio::fs::remove_file(tmp).await;
    if r.success {
        CmdResult {
            success: true,
            output: format!(
                "Resume 훅 설치 완료: {RESUME_HOOK_PATH}\n\n다음 절전 복귀부터 활성 IME 데몬이 자동 재시작됩니다."
            ),
        }
    } else {
        r
    }
}

async fn uninstall_resume_hook() -> CmdResult {
    let cmd = format!("pkexec rm -f {RESUME_HOOK_PATH}");
    let r = runner::run_sh(&cmd).await;
    if r.success {
        CmdResult { success: true, output: format!("Resume 훅 제거: {RESUME_HOOK_PATH}") }
    } else {
        r
    }
}


// ── IME 데몬 재시작 ──────────────────────────────────────────

fn daemon_bin(kind: &ImeKind) -> &'static str {
    match kind {
        ImeKind::Kime   => "kime",
        ImeKind::Ibus   => "ibus-daemon",
        ImeKind::Fcitx5 => "fcitx5",
    }
}

/// setsid 로 popmgr 프로세스와 완전히 분리해 띄운다 (popmgr 종료 시 함께 죽지 않도록).
fn daemon_restart_cmd(kind: &ImeKind) -> &'static str {
    match kind {
        ImeKind::Kime   => "pkill -x kime 2>/dev/null; sleep 0.3; setsid kime </dev/null >/dev/null 2>&1 &",
        ImeKind::Ibus   => "pkill -x ibus-daemon 2>/dev/null; sleep 0.3; setsid ibus-daemon -drxR </dev/null >/dev/null 2>&1 &",
        ImeKind::Fcitx5 => "pkill -x fcitx5 2>/dev/null; sleep 0.3; setsid fcitx5 -d --replace </dev/null >/dev/null 2>&1 &",
    }
}

async fn restart_ime_daemon(kind: ImeKind) -> CmdResult {
    let bin = daemon_bin(&kind);
    if !which_exists(bin).await {
        return CmdResult {
            success: false,
            output: format!("{} 미설치: '{}' 바이너리를 찾을 수 없습니다.", kind.label(), bin),
        };
    }
    runner::run_sh(daemon_restart_cmd(&kind)).await;
    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    if runner::run("pgrep", &["-x", bin]).await.success {
        CmdResult {
            success: true,
            output: format!(
                "{} 데몬 재시작 완료.\n주의: X11(XIM) 앱(Wine, Java 등)은 입력 컨텍스트가 끊기므로 창을 다시 포커스하거나 재실행해야 할 수 있습니다.",
                kind.label()
            ),
        }
    } else {
        CmdResult {
            success: false,
            output: format!(
                "{} 데몬이 시작되지 않았습니다.\n터미널에서 직접 `{}` 실행 후 에러 메시지를 확인해주세요.",
                kind.label(), bin
            ),
        }
    }
}

// ── fcitx5 한/영 상태 유지 설정 ─────────────────────────────

fn fcitx5_config_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/nonexistent"))
        .join("fcitx5")
        .join("config")
}

/// fcitx5 설정 파일에서 한/영 상태 정책만 읽는다. 키가 없으면 fcitx5 기본값으로 본다.
fn parse_fcitx5_behavior(content: &str) -> Fcitx5Behavior {
    let mut share = "No".to_string();
    let mut active = false;
    // fcitx5는 빈 키 목록을 부모 섹션의 `AltTriggerKeys=` 로, 비어 있지 않으면
    // `[Hotkey/AltTriggerKeys]` 하위 섹션으로 저장한다. 둘 다 없으면 기본값(Shift_L).
    let mut has_alt_trigger_keys = false;
    let mut shift_alt_trigger = false;
    let mut section = String::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
            if section == "Hotkey/AltTriggerKeys" {
                has_alt_trigger_keys = true;
            }
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match section.as_str() {
            "Behavior" => match k {
                "ShareInputState" => share = v.to_string(),
                "ActiveByDefault" => active = v.eq_ignore_ascii_case("true"),
                _ => {}
            },
            "Hotkey" if k == "AltTriggerKeys" => {
                has_alt_trigger_keys = true;
                shift_alt_trigger |= v == "Shift_L";
            }
            "Hotkey/AltTriggerKeys" => {
                shift_alt_trigger |= v == "Shift_L";
            }
            _ => {}
        }
    }
    if !has_alt_trigger_keys {
        shift_alt_trigger = true;
    }
    Fcitx5Behavior { share_input_state: share, active_by_default: active, shift_alt_trigger }
}

struct IniSection {
    header: Option<String>, // None = 첫 섹션 헤더 앞부분
    lines: Vec<String>,
}

fn set_ini_key(sections: &mut Vec<IniSection>, section: &str, key: &str, value: &str) {
    let idx = match sections.iter().position(|s| s.header.as_deref() == Some(section)) {
        Some(i) => i,
        None => {
            // 새 섹션은 파일 끝에, 앞 내용과 빈 줄로 구분
            if let Some(last) = sections.last_mut() {
                if last.lines.last().map(|l| !l.trim().is_empty()).unwrap_or(false) {
                    last.lines.push(String::new());
                }
            }
            sections.push(IniSection { header: Some(section.to_string()), lines: Vec::new() });
            sections.len() - 1
        }
    };
    let new_line = format!("{key}={value}");
    let sec = &mut sections[idx];
    let mut found = false;
    sec.lines.retain(|line| {
        if line.trim().split_once('=').map(|(existing, _)| existing.trim() == key).unwrap_or(false) {
            if found {
                false
            } else {
                found = true;
                true
            }
        } else {
            true
        }
    });
    if found {
        let existing = sec.lines.iter_mut().find(|line| {
            line.trim().split_once('=').map(|(existing, _)| existing.trim() == key).unwrap_or(false)
        }).expect("retained first matching key");
        *existing = new_line;
    } else {
        // 섹션 끝의 빈 줄(다음 섹션과의 구분) 앞에 넣는다
        let mut at = sec.lines.len();
        while at > 0 && sec.lines[at - 1].trim().is_empty() {
            at -= 1;
        }
        sec.lines.insert(at, new_line);
    }
}

/// 설정 파일 내용을 한/영 상태 유지 정책으로 고친 새 내용을 돌려준다.
/// 관련 키만 바꾸고 나머지 줄(주석·다른 단축키)은 그대로 둔다.
fn patch_fcitx5_behavior(content: &str) -> String {
    let mut sections = vec![IniSection { header: None, lines: Vec::new() }];
    for raw in content.lines() {
        let t = raw.trim();
        if t.starts_with('[') && t.ends_with(']') {
            sections.push(IniSection { header: Some(t[1..t.len() - 1].to_string()), lines: Vec::new() });
        } else {
            sections.last_mut().unwrap().lines.push(raw.to_string());
        }
    }
    // 왼쪽 Shift 단독 탭 → 영문 전환 (AltTriggerKeys=Shift_L)만 제거한다.
    // 다른 AltTriggerKeys는 보존하고, 목록 키는 fcitx5 형식대로 다시 번호를 매긴다.
    let mut retained_alt_trigger_keys = false;
    for section in &mut sections {
        if section.header.as_deref() != Some("Hotkey/AltTriggerKeys") {
            continue;
        }
        let mut entry_index = 0usize;
        section.lines.retain_mut(|line| {
            let Some((key, value)) = line.trim().split_once('=') else {
                return true;
            };
            if key.trim().parse::<usize>().is_err() {
                return true;
            }
            if value.trim() == "Shift_L" {
                return false;
            }
            *line = format!("{entry_index}={}", value.trim());
            entry_index += 1;
            true
        });
        retained_alt_trigger_keys |= entry_index > 0;
    }
    if retained_alt_trigger_keys {
        sections.retain(|section| section.header.as_deref() != Some("Hotkey/AltTriggerKeys") || !section.lines.is_empty());
    } else {
        // 섹션을 지우기만 하면 fcitx5가 기본값(Shift_L)으로 되돌리므로 빈 키를 명시한다.
        sections.retain(|section| section.header.as_deref() != Some("Hotkey/AltTriggerKeys"));
        set_ini_key(&mut sections, "Hotkey", "AltTriggerKeys", "");
    }
    // 모든 창이 한/영 상태를 공유하고, 새 창은 한글로 시작
    set_ini_key(&mut sections, "Behavior", "ShareInputState", "All");
    set_ini_key(&mut sections, "Behavior", "ActiveByDefault", "True");

    let mut out = String::new();
    for s in &sections {
        if let Some(h) = &s.header {
            out.push('[');
            out.push_str(h);
            out.push_str("]\n");
        }
        for l in &s.lines {
            out.push_str(l);
            out.push('\n');
        }
    }
    if !content.is_empty() && !content.ends_with('\n') {
        out.pop();
    }
    out
}

fn write_fcitx5_config(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let permissions = match std::fs::metadata(path) {
        Ok(metadata) => metadata.permissions(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::Permissions::from_mode(0o600)
        }
        Err(error) => return Err(error),
    };
    let tmp = path.with_extension("popmgr-tmp");
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        std::fs::set_permissions(&tmp, permissions)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// 실행 중인 fcitx5가 실제로 쓰는 ShareInputState 값을 D-Bus 로 읽는다.
async fn read_live_fcitx5_share_state() -> Option<String> {
    let script = "dbus-send --session --print-reply --dest=org.fcitx.Fcitx5 /controller \
        org.fcitx.Fcitx.Controller1.GetConfig string:fcitx://config/global 2>/dev/null \
        | grep -A1 '\"ShareInputState\"' | grep -o 'variant *string \"[^\"]*\"' | head -1 \
        | sed 's/.*\"\\(.*\\)\"/\\1/'";
    let r = runner::run_sh(script).await;
    let v = r.output.trim().to_string();
    if r.success && !v.is_empty() { Some(v) } else { None }
}

/// ~/.config/fcitx5/config 를 고치고, 실행 중인 fcitx5에는 재시작 없이 반영한다.
/// (재시작하면 XIM 클라이언트의 입력 컨텍스트가 끊기고 모든 창의 상태가 리셋된다.)
pub async fn fix_fcitx5_behavior() -> CmdResult {
    let path = fcitx5_config_path();
    let original = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    let patched = patch_fcitx5_behavior(&original);
    let mut out = String::new();

    if patched != original {
        if let Some(dir) = path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(dir).await {
                return CmdResult { success: false, output: format!("설정 디렉토리 생성 실패 ({}): {e}", dir.display()) };
            }
        }
        if let Err(e) = write_fcitx5_config(&path, &patched) {
            return CmdResult { success: false, output: format!("설정 쓰기 실패 ({}): {e}", path.display()) };
        }
        out.push_str(&format!("{} 갱신\n", path.display()));
    } else {
        out.push_str(&format!("{} 은 이미 올바른 설정\n", path.display()));
    }

    if runner::run("pgrep", &["-x", "fcitx5"]).await.success {
        let r = runner::run("fcitx5-remote", &["-r"]).await;
        if !r.success {
            out.push_str(&format!("[!] fcitx5-remote -r (설정 재로드) 실패: {}\n", r.output.trim()));
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            match read_live_fcitx5_share_state().await {
                Some(v) if v == "All" => out.push_str("[확인] 실행 중인 fcitx5에 반영됨: ShareInputState=All\n"),
                Some(v) => out.push_str(&format!("[!] 실행 중인 fcitx5는 아직 ShareInputState={v} — 'IME 재시작'을 눌러주세요\n")),
                None => out.push_str("[?] 실행 중인 fcitx5의 설정을 D-Bus로 확인하지 못했습니다\n"),
            }
        }
    } else {
        out.push_str("fcitx5 미실행: 다음 시작부터 적용됩니다\n");
    }

    out.push_str(
        "\n- 창을 오가거나 데몬이 재시작돼도 한/영 상태가 유지되고, 새 창은 한글로 시작합니다.\n\
         - 왼쪽 Shift 단독 탭으로 영문 전환되는 동작은 껐습니다 (전환은 한/영·Shift+Space 등 트리거 키로).",
    );
    CmdResult { success: true, output: out }
}

async fn install_fcitx5_frontends(pkgs: Vec<&'static str>) -> CmdResult {
    if pkgs.is_empty() {
        return CmdResult { success: true, output: "설치할 fcitx5 프론트엔드가 없습니다.".into() };
    }
    let script = format!("pkexec apt-get install -y {}", pkgs.join(" "));
    let r = runner::run_sh(&script).await;
    if r.success {
        CmdResult {
            success: true,
            output: format!(
                "fcitx5 프론트엔드 설치 완료: {}\n해당 툴킷 앱은 재실행해야 IM 모듈을 읽습니다.",
                pkgs.join(", ")
            ),
        }
    } else {
        r
    }
}

fn fcitx5_behavior_card(beh: &Fcitx5Behavior, disabled: bool) -> Element<'static, ImeMsg> {
    let ok = beh.is_ok();
    let title = if ok {
        "[OK] fcitx5 한/영 상태 유지 설정".to_string()
    } else {
        "[!] fcitx5: 창 전환·재시작 때 영문으로 초기화되는 설정".to_string()
    };
    let title_col = if ok { C_OK } else { C_WARN };

    let mut body = column![
        text(title).size(13).color(title_col),
        Space::with_height(4),
    ];
    if ok {
        body = body.push(
            text("모든 창이 한/영 상태를 공유하고 새 창은 한글로 시작합니다. 왼쪽 Shift 단독 탭 영문 전환은 꺼져 있습니다.")
                .size(11).color(C_DIM),
        );
    } else {
        body = body.push(
            text("fcitx5 기본값은 창마다 한/영 상태를 따로 기억하고 새 창을 영문으로 시작합니다. 다른 창에 갔다 오거나 데몬이 재시작되면 영문만 입력되는 원인입니다.")
                .size(11).color(C_DIM),
        );
        body = body.push(Space::with_height(6));
        let mark = |good: bool| if good { "[OK]" } else { "[!]" };
        body = body.push(
            text(format!(
                "{} 입력 상태 공유 ShareInputState={} → All\n{} 기본 활성 ActiveByDefault={} → True\n{} 왼쪽 Shift 단독 탭 영문 전환(AltTriggerKeys=Shift_L) → 제거",
                mark(beh.share_input_state == "All"), beh.share_input_state,
                mark(beh.active_by_default), if beh.active_by_default { "True" } else { "False" },
                mark(!beh.shift_alt_trigger),
            ))
            .size(11).color(C_WARN),
        );
        body = body.push(Space::with_height(6));
        body = body.push(
            text("~/.config/fcitx5/config 를 고치고 재시작 없이 즉시 반영합니다 (fcitx5-remote -r).")
                .size(11).color(C_DIM),
        );
        body = body.push(Space::with_height(8));
        body = body.push(row![
            Space::with_width(Length::Fill),
            action_btn("고치기", ImeMsg::FixFcitx5Behavior, !disabled, C_BLUE),
        ]);
    }
    container(body)
        .width(Length::Fill)
        .padding([12, 14])
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(
                if ok { C_OK_BG }
                else  { C_WARN_BG }
            )),
            border: iced::Border {
                radius: 8.0.into(),
                color: if ok { C_OK } else { C_WARN },
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
}

fn fcitx5_frontend_card(missing: &[&'static str], disabled: bool) -> Element<'static, ImeMsg> {
    let items = missing.iter().map(|pkg| {
        let desc = FCITX5_FRONTEND_PKGS.iter()
            .find(|(p, _)| p == pkg)
            .map(|(_, d)| *d)
            .unwrap_or("");
        format!("- {pkg}  ({desc})")
    }).collect::<Vec<_>>().join("\n");
    let body = column![
        text("[!] fcitx5 툴킷 프론트엔드(IM 모듈) 미설치").size(13).color(C_WARN),
        Space::with_height(4),
        text("GTK_IM_MODULE/QT_IM_MODULE=fcitx 로 지정돼 있지만 해당 툴킷의 fcitx5 모듈이 없어, 그 툴킷으로 만든 앱에서는 한글 입력이 안 되거나 불안정합니다.")
            .size(11).color(C_DIM),
        Space::with_height(6),
        text(items).size(11).color(C_WARN),
        Space::with_height(8),
        row![
            Space::with_width(Length::Fill),
            action_btn("설치 (pkexec)", ImeMsg::InstallFcitx5Frontends, !disabled, C_BLUE),
        ],
    ];
    container(body)
        .width(Length::Fill)
        .padding([12, 14])
        .style(|_| iced::widget::container::Style {
            background: Some(iced::Background::Color(C_WARN_BG)),
            border: iced::Border { radius: 8.0.into(), color: C_WARN, width: 1.0 },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod fcitx5_behavior_tests {
    use super::{parse_fcitx5_behavior, patch_fcitx5_behavior, write_fcitx5_config};
    use std::os::unix::fs::PermissionsExt;

    fn temp_config_path(name: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time after Unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "popmgr-fcitx5-behavior-{}-{unique}",
            std::process::id(),
        ));
        std::fs::create_dir_all(&dir).expect("create temporary test directory");
        dir.join(name)
    }

    // 실제 노트북의 ~/.config/fcitx5/config 요약 (fcitx5 5.1.7 기본값 그대로)
    const REAL: &str = "[Hotkey]\n\
# 트리거 키를 반복해서 누를 때 열거하기\n\
EnumerateWithTriggerKeys=True\n\
# 입력기 전환\n\
EnumerateForwardKeys=\n\
\n\
[Hotkey/TriggerKeys]\n\
0=Control+space\n\
1=Shift+space\n\
2=Hangul\n\
\n\
[Hotkey/AltTriggerKeys]\n\
0=Shift_L\n\
\n\
[Hotkey/PrevPage]\n\
0=Up\n\
\n\
[Behavior]\n\
# 기본적으로 활성화\n\
ActiveByDefault=False\n\
# 입력 상태 공유\n\
ShareInputState=No\n\
# 페이지 크기 기본값\n\
DefaultPageSize=5\n";

    #[test]
    fn defaults_when_config_is_missing() {
        let b = parse_fcitx5_behavior("");
        assert_eq!(b.share_input_state, "No");
        assert!(!b.active_by_default);
        assert!(b.shift_alt_trigger, "AltTriggerKeys 미지정 = fcitx5 기본값 Shift_L");
        assert!(!b.is_ok());
    }

    #[test]
    fn parses_the_real_default_config_as_broken() {
        let b = parse_fcitx5_behavior(REAL);
        assert_eq!(b.share_input_state, "No");
        assert!(!b.active_by_default);
        assert!(b.shift_alt_trigger);
        assert!(!b.is_ok());
    }

    #[test]
    fn patch_fixes_policy_and_keeps_everything_else() {
        let patched = patch_fcitx5_behavior(REAL);
        assert!(parse_fcitx5_behavior(&patched).is_ok(), "{patched}");
        // 관련 키만 바뀜
        assert_eq!(patched.matches("ShareInputState=All").count(), 1);
        assert_eq!(patched.matches("ActiveByDefault=True").count(), 1);
        assert!(!patched.contains("ShareInputState=No"));
        assert!(!patched.contains("ActiveByDefault=False"));
        assert!(!patched.contains("[Hotkey/AltTriggerKeys]"));
        assert!(patched.contains("[Hotkey]\n# 트리거 키를 반복해서 누를 때 열거하기\nEnumerateWithTriggerKeys=True"));
        assert!(patched.contains("EnumerateForwardKeys=\nAltTriggerKeys=\n\n[Hotkey/TriggerKeys]\n0=Control+space\n1=Shift+space\n2=Hangul\n"));
        assert!(patched.contains("[Hotkey/PrevPage]\n0=Up\n"));
        assert!(patched.contains("# 페이지 크기 기본값\nDefaultPageSize=5\n"));
    }

    #[test]
    fn patch_is_idempotent() {
        let once = patch_fcitx5_behavior(REAL);
        assert_eq!(patch_fcitx5_behavior(&once), once);
    }

    #[test]
    fn patch_creates_sections_for_an_empty_file() {
        let patched = patch_fcitx5_behavior("");
        assert_eq!(patched, "[Hotkey]\nAltTriggerKeys=\n\n[Behavior]\nShareInputState=All\nActiveByDefault=True\n");
        assert!(parse_fcitx5_behavior(&patched).is_ok());
    }

    #[test]
    fn empty_alt_trigger_key_means_no_shift_toggle() {
        let content = "[Hotkey]\nAltTriggerKeys=\n\n[Behavior]\nShareInputState=All\nActiveByDefault=True\n";
        let b = parse_fcitx5_behavior(content);
        assert!(!b.shift_alt_trigger);
        assert!(b.is_ok());
    }

    #[test]
    fn parses_shift_l_alt_trigger_entry() {
        let content = "[Hotkey/AltTriggerKeys]\n0=Shift_L\n";
        assert!(parse_fcitx5_behavior(content).shift_alt_trigger);
    }

    #[test]
    fn parses_shift_l_among_other_alt_trigger_entries() {
        let content = "[Hotkey/AltTriggerKeys]\n0=Shift_L\n1=Shift_R\n";
        assert!(parse_fcitx5_behavior(content).shift_alt_trigger);
    }

    #[test]
    fn does_not_treat_shift_r_as_shift_l_alt_trigger() {
        let content = "[Hotkey/AltTriggerKeys]\n0=Shift_R\n";
        assert!(!parse_fcitx5_behavior(content).shift_alt_trigger);
    }

    #[test]
    fn does_not_treat_control_shift_l_as_shift_l_alt_trigger() {
        let content = "[Hotkey/AltTriggerKeys]\n0=Control+Shift_L\n";
        assert!(!parse_fcitx5_behavior(content).shift_alt_trigger);
    }

    #[test]
    fn patch_replaces_first_duplicate_key_and_removes_the_rest() {
        let content = "[Behavior]\nShareInputState=No\nShareInputState=Program\nActiveByDefault=False\n";
        let patched = patch_fcitx5_behavior(content);
        assert_eq!(patched.matches("ShareInputState=").count(), 1);
        assert!(patched.contains("ShareInputState=All\n"));
    }

    #[test]
    fn patch_is_independent_of_section_order() {
        let content = "[Behavior]\nActiveByDefault=False\nShareInputState=No\n\n[Hotkey/AltTriggerKeys]\n0=Shift_R\n";
        let patched = patch_fcitx5_behavior(content);
        assert!(parse_fcitx5_behavior(&patched).is_ok(), "{patched}");
        assert!(patched.contains("[Hotkey/AltTriggerKeys]\n0=Shift_R\n"));
    }

    #[test]
    fn patch_without_trailing_newline_is_idempotent() {
        let content = "[Hotkey/AltTriggerKeys]\n0=Shift_L\n1=Shift_R\n[Behavior]\nShareInputState=No\nActiveByDefault=False";
        let once = patch_fcitx5_behavior(content);
        assert!(!once.ends_with('\n'));
        assert_eq!(patch_fcitx5_behavior(&once), once);
    }

    #[test]
    fn write_fcitx5_config_preserves_existing_permissions() {
        let path = temp_config_path("config");
        std::fs::write(&path, "old").expect("write existing config");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("set existing config permissions");
        write_fcitx5_config(&path, "new").expect("replace config");
        assert_eq!(std::fs::metadata(&path).expect("config metadata").permissions().mode() & 0o777, 0o600);
        std::fs::remove_dir_all(path.parent().expect("temporary directory")).expect("remove temporary directory");
    }

    #[test]
    fn write_fcitx5_config_uses_0600_for_new_file() {
        let path = temp_config_path("config");
        write_fcitx5_config(&path, "new").expect("write config");
        assert_eq!(std::fs::metadata(&path).expect("config metadata").permissions().mode() & 0o777, 0o600);
        std::fs::remove_dir_all(path.parent().expect("temporary directory")).expect("remove temporary directory");
    }

    #[test]
    fn write_fcitx5_config_writes_the_given_content() {
        let path = temp_config_path("config");
        let content = "[Behavior]\nShareInputState=All\n";
        write_fcitx5_config(&path, content).expect("write config");
        assert_eq!(std::fs::read_to_string(&path).expect("read config"), content);
        std::fs::remove_dir_all(path.parent().expect("temporary directory")).expect("remove temporary directory");
    }

    #[test]
    fn program_scope_is_not_enough() {
        let content = "[Hotkey]\nAltTriggerKeys=\n\n[Behavior]\nShareInputState=Program\nActiveByDefault=True\n";
        assert!(!parse_fcitx5_behavior(content).is_ok());
    }
}

fn detect_snap_im_leak() -> Option<String> {
    let v = std::env::var("GTK_IM_MODULE_FILE").ok()?;
    // /snap/ 또는 ~/snap/ 에서 온 cache는 시스템 GTK 모듈을 가리지 못해 한글 깨짐
    if v.contains("/snap/") || v.contains("/.snap/") {
        Some(v)
    } else {
        None
    }
}

// ── LibreOffice Wayland/fcitx 호환 런처 ────────────────────────
// COSMIC Wayland에서 LibreOffice(GTK3)가 GTK_IM_MODULE=fcitx를 상속하고도
// im-fcitx5.so를 로드하지 않는 경우가 있다. 시스템 desktop 파일을 수정하면
// 패키지 업데이트에 덮어써지므로, 같은 desktop ID의 사용자 사본에서만
// WAYLAND_DISPLAY를 제거해 X11을 확실히 선택하고 GTK의 XIM 경로를 사용한다.
// GDK_BACKEND=x11만 지정하면 COSMIC에서 여전히 gdk-wayland로 뜨는 경우가 있다.
const LO_COMPAT_MARKER_PREFIX: &str = "# popmgr-libreoffice-ime-compat:";
const LO_COMPAT_MARKER: &str = "# popmgr-libreoffice-ime-compat: v2";
const LO_COMPAT_EXEC_PREFIX: &str =
    "env -u WAYLAND_DISPLAY GDK_BACKEND=x11 GTK_IM_MODULE=xim ";
const LO_DESKTOP_NAMES: &[&str] = &[
    "libreoffice-startcenter.desktop",
    "libreoffice-writer.desktop",
    "libreoffice-calc.desktop",
    "libreoffice-impress.desktop",
    "libreoffice-draw.desktop",
    "libreoffice-math.desktop",
];

fn libreoffice_user_app_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    std::path::PathBuf::from(home).join(".local/share/applications")
}

fn detect_libreoffice_compat() -> bool {
    let path = libreoffice_user_app_dir().join("libreoffice-writer.desktop");
    std::fs::read_to_string(path)
        .map(|c| c.lines().any(|line| line == LO_COMPAT_MARKER))
        .unwrap_or(false)
}

fn is_popmgr_libreoffice_desktop(content: &str) -> bool {
    content.lines().any(|line| line.starts_with(LO_COMPAT_MARKER_PREFIX))
}

fn detect_running_libreoffice_ime() -> (bool, bool) {
    let Ok(entries) = std::fs::read_dir("/proc") else { return (false, false) };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) { continue; }
        let proc_dir = entry.path();
        let comm = std::fs::read_to_string(proc_dir.join("comm")).unwrap_or_default();
        if comm.trim() != "soffice.bin" { continue; }

        let env = std::fs::read(proc_dir.join("environ")).unwrap_or_default();
        let running_wayland = env.split(|b| *b == 0).any(|item|
            item.starts_with(b"WAYLAND_DISPLAY=") && item.len() > b"WAYLAND_DISPLAY=".len()
        );
        let maps = std::fs::read_to_string(proc_dir.join("maps")).unwrap_or_default();
        return (running_wayland, maps.contains("im-fcitx5.so"));
    }
    (false, false)
}

fn patch_libreoffice_desktop(original: &str) -> String {
    let mut out = String::with_capacity(original.len() + 128);
    out.push_str(LO_COMPAT_MARKER);
    out.push('\n');
    for line in original.lines() {
        if let Some(command) = line.strip_prefix("Exec=") {
            if command.starts_with(LO_COMPAT_EXEC_PREFIX) {
                out.push_str(line);
            } else {
                out.push_str("Exec=");
                out.push_str(LO_COMPAT_EXEC_PREFIX);
                out.push_str(command);
            }
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

async fn install_libreoffice_compat() -> CmdResult {
    let user_dir = libreoffice_user_app_dir();
    if let Err(e) = tokio::fs::create_dir_all(&user_dir).await {
        return CmdResult { success: false, output: format!("사용자 앱 디렉터리 생성 실패: {e}") };
    }

    let mut installed = Vec::new();
    let mut skipped = Vec::new();
    for name in LO_DESKTOP_NAMES {
        let source = std::path::Path::new("/usr/share/applications").join(name);
        let target = user_dir.join(name);
        if let Ok(existing) = tokio::fs::read_to_string(&target).await {
            if !is_popmgr_libreoffice_desktop(&existing) {
                skipped.push(format!("{name} (기존 사용자 설정 보존)"));
                continue;
            }
        }
        let original = match tokio::fs::read_to_string(&source).await {
            Ok(c) => c,
            Err(e) => {
                skipped.push(format!("{name} (시스템 파일 읽기 실패: {e})"));
                continue;
            }
        };
        if let Err(e) = tokio::fs::write(&target, patch_libreoffice_desktop(&original)).await {
            skipped.push(format!("{name} (쓰기 실패: {e})"));
        } else {
            installed.push(*name);
        }
    }

    let _ = runner::run("update-desktop-database", &[user_dir.to_string_lossy().as_ref()]).await;
    let success = !installed.is_empty() && skipped.is_empty();
    let mut output = if installed.is_empty() {
        "설치된 LibreOffice 호환 런처가 없습니다.".to_string()
    } else {
        format!(
            "LibreOffice 한글 입력 호환 모드 설치: {}개\n\n열려 있는 LibreOffice를 모두 닫고 다시 실행하세요.",
            installed.len()
        )
    };
    if !skipped.is_empty() {
        output.push_str("\n\n처리하지 못한 항목:\n- ");
        output.push_str(&skipped.join("\n- "));
    }
    CmdResult { success, output }
}

async fn uninstall_libreoffice_compat() -> CmdResult {
    let user_dir = libreoffice_user_app_dir();
    let mut removed = 0usize;
    let mut failures = Vec::new();
    for name in LO_DESKTOP_NAMES {
        let target = user_dir.join(name);
        let Ok(existing) = tokio::fs::read_to_string(&target).await else { continue };
        if !is_popmgr_libreoffice_desktop(&existing) { continue; }
        match tokio::fs::remove_file(&target).await {
            Ok(_) => removed += 1,
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }
    let _ = runner::run("update-desktop-database", &[user_dir.to_string_lossy().as_ref()]).await;
    let mut output = format!("LibreOffice 한글 입력 호환 런처 {removed}개 제거.");
    if !failures.is_empty() {
        output.push_str("\n실패:\n- ");
        output.push_str(&failures.join("\n- "));
    }
    CmdResult { success: failures.is_empty(), output }
}

async fn find_jetbrains_vmoptions() -> Vec<JetBrainsVmOptions> {
    let home = std::env::var("HOME").unwrap_or_default();
    let root = format!("{home}/.config/JetBrains");
    let mut out = Vec::new();
    let mut entries = match tokio::fs::read_dir(&root).await {
        Ok(e) => e,
        Err(_) => return out,
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        // ex: IntelliJIdea2026.1, GoLand2025.3, PyCharm2025.2
        if !name.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false) {
            continue;
        }
        let dir = entry.path();
        let mut dir_entries = match tokio::fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        while let Ok(Some(f)) = dir_entries.next_entry().await {
            let fname = f.file_name().to_string_lossy().to_string();
            if !fname.ends_with(".vmoptions") { continue; }
            let path = f.path().to_string_lossy().to_string();
            let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
            let has_xtoolkit = content.lines().any(|l|
                l.trim().starts_with("-Dawt.toolkit.name=XToolkit"));
            let has_recreate_xim = content.lines().any(|l|
                l.trim().starts_with("-Drecreate.x11.input.method=true"));
            out.push(JetBrainsVmOptions {
                name: name.clone(),
                path,
                has_xtoolkit,
                has_recreate_xim,
            });
        }
    }
    out
}

async fn clean_shell_inits(conflicts: Vec<ShellInitConflict>, active: ImeKind) -> CmdResult {
    if conflicts.is_empty() {
        return CmdResult { success: true, output: "정리할 충돌 라인 없음.".into() };
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut log = String::new();
    for conflict in &conflicts {
        let backup = format!("{}.popmgr-backup-{}", conflict.path, now);
        if let Err(e) = tokio::fs::copy(&conflict.path, &backup).await {
            log.push_str(&format!("[실패] {} 백업: {}\n", conflict.path, e));
            continue;
        }
        let original = match tokio::fs::read_to_string(&conflict.path).await {
            Ok(c) => c,
            Err(e) => {
                log.push_str(&format!("[실패] {} 읽기: {}\n", conflict.path, e));
                continue;
            }
        };
        let bad_set: std::collections::HashSet<&str> =
            conflict.lines.iter().map(|s| s.as_str()).collect();
        let mut new_content = String::new();
        let marker = format!("# popmgr-disabled ({}): conflict with active IME = {}",
            chrono_like_now(now), active.label());
        for line in original.lines() {
            if bad_set.contains(line) {
                new_content.push_str(&marker);
                new_content.push('\n');
                new_content.push_str("# ");
                new_content.push_str(line);
                new_content.push('\n');
            } else {
                new_content.push_str(line);
                new_content.push('\n');
            }
        }
        if let Err(e) = tokio::fs::write(&conflict.path, new_content).await {
            log.push_str(&format!("[실패] {} 쓰기: {}\n", conflict.path, e));
            continue;
        }
        log.push_str(&format!("[OK] {} ({}줄 비활성, 백업: {})\n",
            conflict.path, conflict.lines.len(), backup));
    }
    CmdResult { success: true, output: log }
}

fn chrono_like_now(secs: u64) -> String {
    // 외부 chrono 없이 단순 UTC 포맷 (분 단위까지)
    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    // 1970-01-01 기준 누적 일 → 년월일 환산 (대충, 백업 식별용으로 충분)
    let mut y = 1970u32;
    let mut d = days as u32;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let dy = if leap { 366 } else { 365 };
        if d < dy { break; }
        d -= dy;
        y += 1;
    }
    let months = [31u32,28,31,30,31,30,31,31,30,31,30,31];
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mut mo = 0u32;
    let mut day = d + 1;
    while mo < 12 {
        let dm = if mo == 1 && leap { 29 } else { months[mo as usize] };
        if day <= dm { break; }
        day -= dm;
        mo += 1;
    }
    format!("{:04}-{:02}-{:02} {:02}:{:02}", y, mo+1, day, h, m)
}

async fn patch_jetbrains_vmoptions(ides: Vec<JetBrainsVmOptions>) -> CmdResult {
    if ides.is_empty() {
        return CmdResult { success: true, output: "발견된 JetBrains vmoptions 없음.".into() };
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut log = String::new();
    for ide in &ides {
        if ide.has_xtoolkit && ide.has_recreate_xim {
            log.push_str(&format!("[스킵] {} — 이미 설정됨\n", ide.name));
            continue;
        }
        let backup = format!("{}.popmgr-backup-{}", ide.path, now);
        if let Err(e) = tokio::fs::copy(&ide.path, &backup).await {
            log.push_str(&format!("[실패] {} 백업: {}\n", ide.path, e));
            continue;
        }
        let mut content = tokio::fs::read_to_string(&ide.path).await.unwrap_or_default();
        if !content.ends_with('\n') { content.push('\n'); }
        let mut added = Vec::new();
        if !ide.has_xtoolkit {
            content.push_str("# popmgr: Korean IME via X11 XIM (avoids native Wayland IM hangs)\n");
            content.push_str("-Dawt.toolkit.name=XToolkit\n");
            added.push("-Dawt.toolkit.name=XToolkit");
        }
        if !ide.has_recreate_xim {
            content.push_str("# popmgr: rebuild X input method context after IME daemon restarts\n");
            content.push_str("-Drecreate.x11.input.method=true\n");
            added.push("-Drecreate.x11.input.method=true");
        }
        if let Err(e) = tokio::fs::write(&ide.path, content).await {
            log.push_str(&format!("[실패] {} 쓰기: {}\n", ide.path, e));
            continue;
        }
        log.push_str(&format!("[OK] {} — 추가: {} (백업: {})\n",
            ide.name, added.join(", "), backup));
    }
    CmdResult { success: true, output: log }
}

async fn pkg_installed(pkg: &str) -> bool {
    let r = runner::run("dpkg", &["-l", pkg]).await;
    r.output.lines().any(|l| l.starts_with("ii"))
}

async fn which_exists(cmd: &str) -> bool {
    runner::run("which", &[cmd]).await.success
}

async fn read_active_ime() -> Option<ImeKind> {
    let r = runner::run("cat", &["/etc/environment"]).await;
    for line in r.output.lines() {
        if line.starts_with("GTK_IM_MODULE=") {
            let val = line.split('=').nth(1).unwrap_or("").trim_matches('"');
            return match val {
                "ibus"   => Some(ImeKind::Ibus),
                "fcitx"  => Some(ImeKind::Fcitx5),
                "kime"   => Some(ImeKind::Kime),
                _        => None,
            };
        }
    }
    None
}

async fn running_ime_daemon() -> Option<ImeKind> {
    for (cmd, kind) in [("ibus-daemon", ImeKind::Ibus), ("fcitx5", ImeKind::Fcitx5), ("kime", ImeKind::Kime)] {
        let r = runner::run("pgrep", &["-x", cmd]).await;
        if r.success { return Some(kind); }
    }
    None
}

async fn check_env_match(kind: &ImeKind) -> bool {
    let r = runner::run("cat", &["/etc/environment"]).await;
    let expected = kind.env_lines();
    expected.iter().all(|(k, v)| {
        r.output.lines().any(|l| {
            let line = l.trim();
            line == &format!("{k}={v}") || line == &format!("{k}=\"{v}\"")
        })
    })
}

async fn apply_ime(kind: ImeKind) -> CmdResult {
    // 선택한 IME 바이너리가 실제로 설치되어 있는지 먼저 확인
    let daemon_bin = match &kind {
        ImeKind::Kime   => "kime",
        ImeKind::Ibus   => "ibus-daemon",
        ImeKind::Fcitx5 => "fcitx5",
    };
    if !which_exists(daemon_bin).await {
        return CmdResult {
            success: false,
            output: format!(
                "{} 미설치: '{}' 바이너리를 찾을 수 없습니다.\n먼저 설치 버튼을 눌러주세요.",
                kind.label(), daemon_bin
            ),
        };
    }

    // 경쟁 데몬 종료
    let others: &[&str] = match &kind {
        ImeKind::Ibus   => &["fcitx5", "kime"],
        ImeKind::Fcitx5 => &["ibus-daemon", "kime"],
        ImeKind::Kime   => &["ibus-daemon", "fcitx5"],
    };
    for d in others {
        runner::run("pkill", &["-x", d]).await;
    }

    // /etc/environment 업데이트 (pkexec tee 방식 — python3 불필요)
    let env_lines = kind.env_lines();
    let keys: Vec<&str> = env_lines.iter().map(|(k, _)| *k).collect();
    let assignments: Vec<String> = env_lines.iter().map(|(k, v)| format!("{k}={v}")).collect();

    // 기존 키 제거 후 새 값 추가하는 awk 스크립트
    let key_pattern = keys.iter().map(|k| format!("^{}=", k)).collect::<Vec<_>>().join("|");
    let new_lines = assignments.join("\n");
    let script = format!(
        "pkexec bash -c \"awk '!/^({pattern})/' /etc/environment > /tmp/env.tmp && echo {new_lines_q} >> /tmp/env.tmp && cp /tmp/env.tmp /etc/environment\"",
        pattern = key_pattern,
        new_lines_q = shell_quote(&new_lines),
    );
    let r = runner::run_sh(&script).await;
    if !r.success {
        return r;
    }

    // dbus / systemd-user 환경변수 import (D-Bus 활성화로 띄우는 앱에만 효과)
    // 주의: 컴포지터(COSMIC)가 launcher로 띄우는 앱은 컴포지터 시작 시점의 env를 상속하므로
    //       완전한 적용을 위해서는 로그아웃/로그인이 필요함.
    let dbus_keys = keys.join(" ");
    let session_script = format!(
        "dbus-update-activation-environment --systemd --all 2>/dev/null; \
         systemctl --user import-environment {dbus_keys} 2>/dev/null",
        dbus_keys = dbus_keys,
    );
    runner::run_sh(&session_script).await;

    // fcitx5: 창 전환·데몬 재시작 때 영문으로 초기화되지 않도록 전역 설정을 먼저 보장
    let behavior_note = if kind == ImeKind::Fcitx5 {
        let r = fix_fcitx5_behavior().await;
        if r.success {
            "\n- fcitx5 한/영 상태 유지 설정 (ShareInputState=All, ActiveByDefault=True)".to_string()
        } else {
            format!("\n- [!] fcitx5 설정 갱신 실패: {}", r.output.lines().next().unwrap_or(""))
        }
    } else {
        String::new()
    };

    // 선택한 IME 데몬 재시작 (detach 후 실제 실행 확인)
    runner::run_sh(daemon_restart_cmd(&kind)).await;

    // 데몬이 실제로 살아있는지 검증
    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    let pgrep_target = match &kind {
        ImeKind::Kime   => "kime",
        ImeKind::Ibus   => "ibus-daemon",
        ImeKind::Fcitx5 => "fcitx5",
    };
    let daemon_alive = runner::run("pgrep", &["-x", pgrep_target]).await.success;

    // systemd user service autostart 설정
    let (enable, disable): (&[&str], &[&str]) = match &kind {
        ImeKind::Ibus   => (&["ibus.service"], &["fcitx5.service"]),
        ImeKind::Fcitx5 => (&["fcitx5.service"], &["ibus.service"]),
        ImeKind::Kime   => (&[], &["ibus.service", "fcitx5.service"]),
    };
    for svc in disable {
        runner::run("systemctl", &["--user", "disable", "--now", svc]).await;
    }
    for svc in enable {
        runner::run("systemctl", &["--user", "enable", "--now", svc]).await;
    }

    if !daemon_alive {
        return CmdResult {
            success: false,
            output: format!(
                "/etc/environment는 업데이트했지만 {} 데몬이 시작되지 않았습니다.\n터미널에서 직접 `{}` 실행 후 에러 메시지를 확인해주세요.",
                kind.label(), pgrep_target
            ),
        };
    }

    CmdResult {
        success: true,
        output: format!(
            "{} 적용 완료.\n- /etc/environment 업데이트\n- 데몬 시작 확인됨{}\n\n[중요] /etc/environment는 로그인 시점에 한 번만 읽힙니다.\n현재 실행 중인 COSMIC 세션의 앱들에 완전히 적용하려면 로그아웃 후 다시 로그인하세요.\n(D-Bus 활성화로 띄우는 앱에는 부분적으로 즉시 반영됨)",
            kind.label(), behavior_note
        ),
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn shell_conflict_card<'a>(
    conflicts: &'a [ShellInitConflict],
    disabled: bool,
) -> Element<'a, ImeMsg> {
    let mut col = column![
        text("[!] 셸 init 파일이 활성 IME와 충돌").size(13).color(C_ERR),
        Space::with_height(4),
        text("아래 파일들이 활성 IME와 다른 값을 강제 export 합니다. 새 셸을 열면 한글 입력이 깨질 수 있습니다.")
            .size(11).color(C_DIM),
        Space::with_height(8),
    ];
    for c in conflicts {
        col = col.push(text(format!("· {}", c.path)).size(12).color(C_TEXT));
        for line in &c.lines {
            col = col.push(text(format!("    {}", line)).size(10).color(C_DIM));
        }
    }
    col = col.push(Space::with_height(8));
    col = col.push(
        row![
            Space::with_width(Length::Fill),
            action_btn("정리 (백업 후 주석화)", ImeMsg::CleanShellInits, !disabled, C_WARN),
        ]
    );
    container(col)
        .width(Length::Fill)
        .padding([12, 14])
        .style(|_| iced::widget::container::Style {
            background: Some(iced::Background::Color(C_ERR_BG)),
            border: iced::Border { radius: 8.0.into(), color: C_ERR, width: 1.0 },
            ..Default::default()
        })
        .into()
}

fn resume_hook_card(installed: bool, disabled: bool) -> Element<'static, ImeMsg> {
    let (title, title_col, desc, btn_label, btn_msg, btn_col) = if installed {
        (
            "[OK] 절전 복귀 IME 자동 복구 활성".to_string(),
            C_OK,
            "절전에서 깨어나면 system-sleep 훅이 실행 중인 IME 데몬을 자동 재시작합니다.\n위치: /etc/systemd/system-sleep/zz-popmgr-ime-restart".to_string(),
            "훅 제거",
            ImeMsg::UninstallResumeHook,
            C_DIM,
        )
    } else {
        (
            "[권장] 절전 복귀 자동 복구 훅 미설치".to_string(),
            C_WARN,
            "절전에서 깨어나면 IME 채널이 끊겨 한글 입력이 안 되는 증상을 자동으로 복구합니다.\nsystemd-sleep post 훅을 설치합니다 (유휴 CPU 0, resume 시 1회만 실행).".to_string(),
            "훅 설치 (pkexec)",
            ImeMsg::InstallResumeHook,
            C_BLUE,
        )
    };

    let body = column![
        text(title).size(13).color(title_col),
        Space::with_height(4),
        text(desc).size(11).color(C_DIM),
        Space::with_height(8),
        row![
            Space::with_width(Length::Fill),
            action_btn(btn_label, btn_msg, !disabled, btn_col),
        ],
    ];
    container(body)
        .width(Length::Fill)
        .padding([12, 14])
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(
                if installed { C_OK_BG }
                else         { C_WARN_BG }
            )),
            border: iced::Border {
                radius: 8.0.into(),
                color: if installed { C_OK } else { C_WARN },
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
}

fn libreoffice_ime_card(st: &ImeStatus, disabled: bool) -> Element<'static, ImeMsg> {
    let broken_now = st.libreoffice_running_wayland && !st.libreoffice_fcitx_module_loaded;
    let compat_installed = st.libreoffice_compat_installed;
    let (title, title_col, desc, btn_label, btn_msg, btn_col) = if st.libreoffice_compat_installed {
        (
            "[OK] LibreOffice 한글 입력 호환 모드 설치됨",
            C_OK,
            "LibreOffice를 X11/XIM 입력 경로로 실행합니다. 해제하면 기본 Wayland 실행 방식으로 돌아갑니다.",
            "호환 모드 해제",
            ImeMsg::UninstallLibreOfficeCompat,
            C_DIM,
        )
    } else if broken_now {
        (
            "[!] LibreOffice가 fcitx 입력 모듈을 연결하지 못함",
            C_ERR,
            "현재 LibreOffice는 Wayland로 실행됐지만 fcitx 입력 컨텍스트를 만들지 못했습니다. X11/XIM 호환 모드는 Wayland를 우회해 fcitx XIM 서버에 직접 연결합니다.",
            "한글 입력 호환 모드 설치",
            ImeMsg::InstallLibreOfficeCompat,
            C_BLUE,
        )
    } else {
        (
            "[i] LibreOffice 한글 입력 호환 모드",
            C_BLUE,
            "LibreOffice에서만 한/영 전환이 안 될 때 사용자 범위 런처에 X11 입력 경로를 적용합니다. 설치 후 열려 있는 LibreOffice를 모두 닫고 다시 실행해야 합니다.",
            "호환 모드 설치",
            ImeMsg::InstallLibreOfficeCompat,
            C_BLUE,
        )
    };

    let body = column![
        text(title).size(13).color(title_col),
        Space::with_height(4),
        text(desc).size(11).color(C_DIM),
        Space::with_height(8),
        row![
            Space::with_width(Length::Fill),
            action_btn(btn_label, btn_msg, !disabled, btn_col),
        ],
    ];
    container(body)
        .width(Length::Fill)
        .padding([12, 14])
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(
                if compat_installed {
                    C_OK_BG
                } else if broken_now {
                    C_ERR_BG
                } else {
                    C_SEL_BG
                }
            )),
            border: iced::Border { radius: 8.0.into(), color: title_col, width: 1.0 },
            ..Default::default()
        })
        .into()
}

fn snap_leak_card(leak: &str) -> Element<'static, ImeMsg> {
    let body = column![
        text("[!] GTK_IM_MODULE_FILE 누출 감지").size(13).color(C_WARN),
        Space::with_height(4),
        text("현재 셸/세션의 환경변수가 snap 캐시를 가리키고 있습니다. 이 변수가 IntelliJ 등 자식 프로세스로 전파되면 시스템 GTK IM 모듈을 못 찾아 한글 입력이 깨집니다.")
            .size(11).color(C_DIM),
        Space::with_height(6),
        text(format!("값: {}", leak)).size(10).color(C_WARN),
        Space::with_height(6),
        text("해결: 셸에서 `unset GTK_IM_MODULE_FILE` 후 IDE 재실행. 영구 해결은 snap 앱을 데스크탑 세션 환경 밖(예: 별도 터미널)에서 띄우거나 제거.")
            .size(11).color(C_DIM),
    ];
    container(body)
        .width(Length::Fill)
        .padding([12, 14])
        .style(|_| iced::widget::container::Style {
            background: Some(iced::Background::Color(C_WARN_BG)),
            border: iced::Border { radius: 8.0.into(), color: C_WARN, width: 1.0 },
            ..Default::default()
        })
        .into()
}

fn jetbrains_card<'a>(
    ides: &'a [JetBrainsVmOptions],
    disabled: bool,
) -> Element<'a, ImeMsg> {
    let mut col = column![
        text("[i] JetBrains IDE 한글 입력 최적화").size(13).color(C_BLUE),
        Space::with_height(4),
        text("아래 IDE vmoptions에 XIM 안정화 옵션이 빠져 있습니다. JBR이 native Wayland 대신 X11 XIM을 거치게 하면 fcitx/ibus와의 freeze가 줄어듭니다.")
            .size(11).color(C_DIM),
        Space::with_height(8),
    ];
    for ide in ides {
        let mut flags = Vec::new();
        if !ide.has_xtoolkit { flags.push("XToolkit"); }
        if !ide.has_recreate_xim { flags.push("recreate-XIM"); }
        let status = if flags.is_empty() { "OK".to_string() }
                     else { format!("누락: {}", flags.join(", ")) };
        let status_col = if flags.is_empty() { C_OK } else { C_WARN };
        col = col.push(
            row![
                text(format!("· {}", ide.name)).size(12).color(C_TEXT),
                Space::with_width(Length::Fill),
                text(status).size(11).color(status_col),
            ]
        );
    }
    col = col.push(Space::with_height(8));
    col = col.push(
        row![
            Space::with_width(Length::Fill),
            action_btn("vmoptions 패치 (백업 후 추가)", ImeMsg::PatchJetBrains, !disabled, C_BLUE),
        ]
    );
    container(col)
        .width(Length::Fill)
        .padding([12, 14])
        .style(|_| iced::widget::container::Style {
            background: Some(iced::Background::Color(C_SEL_BG)),
            border: iced::Border { radius: 8.0.into(), color: C_BLUE, width: 1.0 },
            ..Default::default()
        })
        .into()
}

// ── 공통 위젯 ───────────────────────────────────────────────────

// EOND UI App 라이트 · 파랑 팔레트(~/dev/eond-ui-app tokens.json) — 바탕 app_bg → 카드 c1 → 겹침 c2 → 선택 c3.
// 테마·메인 색을 바꾸려면 여기 THEME_MODE / THEME_ACCENT 만 고친다(main.rs app_theme 도 같은 값을 쓴다).
use eond_ui_theme::{palette, Accent, Mode, Palette, Rgba};
pub const THEME_MODE: Mode = Mode::Dark;
pub const THEME_ACCENT: Accent = Accent::Blue;
const P: Palette = palette(THEME_MODE, THEME_ACCENT);
pub const C_OK:       Color = P.success.to_iced();  // 완료·정상
pub const C_ERR:      Color = P.danger.to_iced();   // 삭제·실패
pub const C_WARN:     Color = P.warning.to_iced();  // 주의·다시 해야 함
pub const C_DIM:      Color = P.fg3.to_iced();      // 보조 텍스트
pub const C_BLUE:     Color = P.primary.to_iced();  // 메인 색(주 동작)
pub const C_GREEN:    Color = P.success.to_iced();
pub const C_PANEL:    Color = P.c1.to_iced();       // 카드 표면
pub const C_BG:       Color = P.app_bg.to_iced();   // 캔버스
pub const C_SURFACE:  Color = P.c1.to_iced();       // 카드/패널
pub const C_SURFACE2: Color = P.c2.to_iced();       // 인셋/보조 표면
pub const C_BORDER:   Color = P.c3.to_iced();       // 헤어라인
pub const C_TEXT:     Color = P.fg.to_iced();       // 본문
pub const C_BTN2:     Color = P.c3.to_iced();       // 보조 버튼
// 연한 채움(선택·완료·주의·삭제) — 반투명 토큰을 카드(c1) 위에 미리 합성한 불투명 색.
// iced 는 투명도를 선형 색공간에서 섞어서, 반투명 그대로 넘기면 웹보다 두 배쯤 진하게 나온다.
pub const C_SEL_BG:     Color = P.primary_flat.over(P.c1).to_iced();  // 선택된 카드
pub const C_OK_BG:      Color = P.success_flat.over(P.c1).to_iced();  // 완료·설치됨
pub const C_WARN_BG:    Color = P.warning_flat.over(P.c1).to_iced();  // 주의 상자
pub const C_ERR_BG:     Color = P.danger_flat.over(P.c1).to_iced();   // 삭제 표시·오류 상자
pub const C_WARN_FG:    Color = P.warning_fg.to_iced();    // 주의 글자
pub const C_WARN_LINE:  Color = Rgba { a: 115, ..P.warning }.over(P.c1).to_iced(); // 주의 상자 테두리(45%)
pub const C_TEXT2:      Color = P.fg2.to_iced();           // 비활성 메뉴 글자
pub const C_PURPLE:     Color = P.purple_fg.to_iced();     // Flatpak
pub const C_HOVER:      Color = Rgba { a: 15, ..P.fg }.over(P.c1).to_iced();  // 호버(6%)
pub const C_HOVER_WEAK: Color = Rgba { a: 8, ..P.fg }.over(P.c1).to_iced();
pub const C_ON_LIGHT:   Color = palette(Mode::Light, THEME_ACCENT).fg.to_iced(); // 밝은 바탕 위 글자(모드 무관)

pub fn action_btn<'a, M: Clone + 'a>(label: impl Into<String>, msg: M, enabled: bool, color: Color) -> Element<'a, M> {
    let bg = if enabled { color } else { C_BTN2 };
    // 배경 휘도에 따라 글자색 자동 선택(라이트 보조버튼=어두운 글자, 컬러 버튼=흰 글자).
    let on = |c: Color| if luminance(c) > 0.62 { C_ON_LIGHT } else { Color::WHITE };
    let tc = if enabled { on(bg) } else { C_DIM };
    let b = button(text(label.into()).size(13).color(tc))
        .padding([11, 22])
        .style(move |_, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered if enabled => shade(bg, 0.05),
                _ => bg,
            };
            iced::widget::button::Style {
                background: Some(iced::Background::Color(bg)),
                border: iced::Border { radius: 12.0.into(), ..Default::default() },
                text_color: tc,
                ..Default::default()
            }
        });
    if enabled { b.on_press(msg).into() } else { b.into() }
}

pub fn running_bar<'a, M: 'a>(label: &'a str) -> Element<'a, M> {
    container(
        text(label).size(12).color(C_WARN_FG)
    )
    .padding([11, 16])
    .width(Length::Fill)
    .style(|_| iced::widget::container::Style {
        background: Some(iced::Background::Color(C_WARN_BG)),
        border: iced::Border { radius: 12.0.into(), color: C_WARN_LINE, width: 1.0 },
        ..Default::default()
    })
    .into()
}

pub fn card<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content)
        .width(Length::Fill)
        .padding([18, 20])
        .style(|_| iced::widget::container::Style {
            background: Some(iced::Background::Color(C_SURFACE)),
            border: iced::Border { radius: 18.0.into(), color: C_BORDER, width: 1.0 },
            shadow: iced::Shadow {
                color: Color { r: 0.06, g: 0.10, b: 0.16, a: 0.06 },
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 16.0,
            },
            ..Default::default()
        })
        .into()
}

/// 상대 휘도(글자 대비 판단용).
pub fn luminance(c: Color) -> f32 { 0.299 * c.r + 0.587 * c.g + 0.114 * c.b }

/// amt 만큼 어둡게(양수) — hover 강조용. 밝은 버튼은 어둡게, 그게 라이트 UI 관습.
pub fn shade(c: Color, amt: f32) -> Color {
    Color { r: (c.r - amt).max(0.0), g: (c.g - amt).max(0.0), b: (c.b - amt).max(0.0), a: c.a }
}
