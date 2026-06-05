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
pub struct ImeStatus {
    pub installed_ibus: bool,
    pub installed_fcitx5: bool,
    pub installed_kime: bool,
    pub active: Option<ImeKind>,
    pub daemon_running: Option<ImeKind>,
    pub env_match: bool,
}

#[derive(Debug, Clone)]
pub enum ImeMsg {
    Refresh,
    Refreshed(ImeStatus),
    Select(ImeKind),
    Install(ImeKind),
    Apply,
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
                self.status = Some(s);
                (Task::none(), None)
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

    ImeStatus { installed_ibus, installed_fcitx5, installed_kime, active, daemon_running, env_match }
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
