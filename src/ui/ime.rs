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
            ImeKind::Fcitx5 => &["fcitx5", "fcitx5-hangul", "fcitx5-frontend-gtk3"],
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
}

#[derive(Debug, Clone)]
pub enum ImeMsg {
    Refresh,
    Refreshed(ImeStatus),
    AutoReconnect(ImeKind),
    Select(ImeKind),
    Install(ImeKind),
    Apply,
    CleanShellInits,
    PatchJetBrains,
    Done(CmdResult),
}

pub struct ImeState {
    pub status: Option<ImeStatus>,
    pub selected: ImeKind,
    pub running: Option<String>,
}

impl ImeState {
    pub fn new() -> Self {
        Self {
            status: None,
            selected: ImeKind::Kime,
            running: None,
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
                // 시작 시 실행 중인 IME 데몬을 조용히 재연결 (Wayland 연결 복구)
                let reconnect_task = if let Some(ref kind) = s.daemon_running {
                    let k = kind.clone();
                    Task::perform(
                        async move {
                            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                            k
                        },
                        ImeMsg::AutoReconnect,
                    )
                } else {
                    Task::none()
                };
                self.status = Some(s);
                (reconnect_task, None)
            }
            ImeMsg::AutoReconnect(kind) => {
                let cmd = match &kind {
                    ImeKind::Kime   => "pkill -x kime 2>/dev/null; sleep 0.2; kime &",
                    ImeKind::Ibus   => "pkill -x ibus-daemon 2>/dev/null; sleep 0.2; ibus-daemon -drxR &",
                    ImeKind::Fcitx5 => "pkill -x fcitx5 2>/dev/null; sleep 0.2; fcitx5 -d --replace &",
                };
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

fn ime_row(kind: ImeKind, installed: bool, selected: bool, disabled: bool) -> Element<'static, ImeMsg> {
    let sel_color = if selected { C_BLUE } else { C_PANEL };
    let border_color = if selected { C_BLUE } else { Color::from_rgb(0.2, 0.2, 0.25) };

    let status_txt = if installed { "설치됨" } else { "미설치" };
    let status_col = if installed { C_OK } else { C_DIM };

    let install_btn: Element<'static, ImeMsg> = if !installed {
        action_btn("설치", ImeMsg::Install(kind.clone()), !disabled, C_GREEN)
    } else {
        Space::with_width(0).into()
    };

    let radio_bg = if selected { C_BLUE } else { Color::from_rgb(0.15, 0.15, 0.18) };
    let radio_border = if selected { C_BLUE } else { Color::from_rgb(0.35, 0.35, 0.4) };
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
            text(label_str).size(14).color(if selected { Color::WHITE } else { Color::from_rgb(0.85, 0.85, 0.9) }),
            text(status_txt).size(11).color(status_col),
        ],
        Space::with_width(Length::Fill),
        install_btn,
    ]
    .align_y(iced::Alignment::Center);

    button(
        container(row_inner).padding([10, 14])
    )
    .width(Length::Fill)
    .on_press(select_msg)
    .style(move |_, _| iced::widget::button::Style {
        background: Some(iced::Background::Color(sel_color)),
        border: iced::Border { radius: 8.0.into(), color: border_color, width: 1.5 },
        text_color: Color::WHITE,
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

    ImeStatus {
        installed_ibus, installed_fcitx5, installed_kime,
        active, daemon_running, env_match,
        shell_init_conflicts, snap_im_module_file, jetbrains_ides,
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

fn detect_snap_im_leak() -> Option<String> {
    let v = std::env::var("GTK_IM_MODULE_FILE").ok()?;
    // /snap/ 또는 ~/snap/ 에서 온 cache는 시스템 GTK 모듈을 가리지 못해 한글 깨짐
    if v.contains("/snap/") || v.contains("/.snap/") {
        Some(v)
    } else {
        None
    }
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

    // 현재 Wayland 세션에 즉시 반영 (새로 시작하는 앱에 적용)
    let dbus_keys = keys.join(" ");
    let export_pairs: Vec<String> = env_lines.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    let session_script = format!(
        "dbus-update-activation-environment --systemd {dbus_keys} 2>/dev/null; \
         systemctl --user import-environment {dbus_keys} 2>/dev/null; \
         export {exports}",
        dbus_keys = dbus_keys,
        exports = export_pairs.join(" "),
    );
    runner::run_sh(&session_script).await;

    // 선택한 IME 데몬 재시작
    let daemon_cmd = match &kind {
        ImeKind::Kime   => Some("pkill -x kime 2>/dev/null; sleep 0.3; kime &"),
        ImeKind::Ibus   => Some("pkill -x ibus-daemon 2>/dev/null; sleep 0.3; ibus-daemon -drxR &"),
        ImeKind::Fcitx5 => Some("pkill -x fcitx5 2>/dev/null; sleep 0.3; fcitx5 -d --replace &"),
    };
    if let Some(cmd) = daemon_cmd {
        runner::run_sh(cmd).await;
    }

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

    CmdResult {
        success: true,
        output: format!(
            "{} 적용 완료.\n- /etc/environment 업데이트\n- 현재 세션 환경변수 반영\n- 데몬 재시작\n새로 여는 앱에 즉시 적용됩니다.",
            kind.label()
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
        col = col.push(text(format!("· {}", c.path)).size(12).color(Color::from_rgb(0.85,0.85,0.9)));
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
            background: Some(iced::Background::Color(Color::from_rgb(0.12, 0.06, 0.05))),
            border: iced::Border { radius: 8.0.into(), color: C_ERR, width: 1.0 },
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
        text(format!("값: {}", leak)).size(10).color(Color::from_rgb(0.75, 0.7, 0.55)),
        Space::with_height(6),
        text("해결: 셸에서 `unset GTK_IM_MODULE_FILE` 후 IDE 재실행. 영구 해결은 snap 앱을 데스크탑 세션 환경 밖(예: 별도 터미널)에서 띄우거나 제거.")
            .size(11).color(Color::from_rgb(0.7, 0.75, 0.85)),
    ];
    container(body)
        .width(Length::Fill)
        .padding([12, 14])
        .style(|_| iced::widget::container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.12, 0.08, 0.03))),
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
                text(format!("· {}", ide.name)).size(12).color(Color::from_rgb(0.85,0.85,0.9)),
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
            background: Some(iced::Background::Color(Color::from_rgb(0.05, 0.07, 0.12))),
            border: iced::Border { radius: 8.0.into(), color: C_BLUE, width: 1.0 },
            ..Default::default()
        })
        .into()
}

// ── 공통 위젯 ───────────────────────────────────────────────────

pub const C_OK:    Color = Color { r: 0.2,  g: 0.78, b: 0.35, a: 1.0 };
pub const C_ERR:   Color = Color { r: 0.9,  g: 0.25, b: 0.25, a: 1.0 };
pub const C_WARN:  Color = Color { r: 0.9,  g: 0.65, b: 0.1,  a: 1.0 };
pub const C_DIM:   Color = Color { r: 0.45, g: 0.45, b: 0.5,  a: 1.0 };
pub const C_BLUE:  Color = Color { r: 0.13, g: 0.45, b: 0.85, a: 1.0 };
pub const C_GREEN: Color = Color { r: 0.15, g: 0.65, b: 0.3,  a: 1.0 };
pub const C_PANEL: Color = Color { r: 0.12, g: 0.12, b: 0.15, a: 1.0 };

pub fn action_btn<'a, M: Clone + 'a>(label: impl Into<String>, msg: M, enabled: bool, color: Color) -> Element<'a, M> {
    let bg = if enabled { color } else { Color::from_rgb(0.2, 0.2, 0.22) };
    let tc = if enabled { Color::WHITE } else { Color::from_rgb(0.4, 0.4, 0.4) };
    let b = button(text(label.into()).size(13).color(tc))
        .padding([8, 18])
        .style(move |_, _| iced::widget::button::Style {
            background: Some(iced::Background::Color(bg)),
            border: iced::Border { radius: 7.0.into(), ..Default::default() },
            text_color: tc,
            ..Default::default()
        });
    if enabled { b.on_press(msg).into() } else { b.into() }
}

pub fn running_bar<'a, M: 'a>(label: &'a str) -> Element<'a, M> {
    container(
        text(label).size(12).color(Color::from_rgb(0.9, 0.8, 0.3))
    )
    .padding([6, 12])
    .style(|_| iced::widget::container::Style {
        background: Some(iced::Background::Color(Color::from_rgb(0.1, 0.09, 0.02))),
        border: iced::Border { radius: 6.0.into(), color: Color::from_rgb(0.4, 0.35, 0.1), width: 1.0 },
        ..Default::default()
    })
    .into()
}

pub fn card<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content)
        .width(Length::Fill)
        .padding([14, 16])
        .style(|_| iced::widget::container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.1, 0.1, 0.13))),
            border: iced::Border { radius: 9.0.into(), color: Color::from_rgb(0.2, 0.2, 0.26), width: 1.0 },
            ..Default::default()
        })
        .into()
}
