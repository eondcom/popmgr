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
                self.running = Some("KakaoTalk 설치 중...".into());
                // kakaotalk-wine 설치 스크립트 실행
                let script = r#"
                    set -e
                    TMP=$(mktemp -d)
                    git clone --depth 1 https://github.com/eondcom/kakaotalk-wine "$TMP/repo" 2>&1
                    bash "$TMP/repo/install.sh" 2>&1
                    rm -rf "$TMP"
                    echo "KakaoTalk 설치 완료"
                "#;
                let t = Task::perform(async move { runner::run_stream(script).await }, AppsMsg::Done);
                (t, None)
            }
            AppsMsg::LaunchKakaotalk => {
                let t = Task::perform(
                    async { runner::run_sh("kakaotalk &").await },
                    AppsMsg::Done,
                );
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
        let kt_installed = self.status.as_ref().map(|s| s.kakaotalk_installed).unwrap_or(false);
        col = col.push(kakaotalk_card(kt_installed, is_running));
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

fn kakaotalk_card(installed: bool, disabled: bool) -> Element<'static, AppsMsg> {
    let status_txt = if installed { "✓ 설치됨" } else { "✗ 미설치" };
    let status_col = if installed { C_OK } else { C_DIM };

    card(
        row![
            column![
                text("KakaoTalk (Wine)").size(14).color(Color::from_rgb(0.9, 0.9, 0.95)),
                Space::with_height(3),
                text("eondcom/kakaotalk-wine — Wine 기반 카카오톡 Linux 설치").size(11).color(C_DIM),
                Space::with_height(4),
                text(status_txt).size(12).color(status_col),
            ].width(Length::Fill),
            column![
                if installed {
                    action_btn("실행", AppsMsg::LaunchKakaotalk, !disabled, Color::from_rgb(0.15, 0.45, 0.75))
                } else {
                    action_btn("설치", AppsMsg::InstallKakaotalk, !disabled, C_OK)
                },
            ].align_x(iced::Alignment::End),
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

    // KakaoTalk 설치 여부
    let kt = runner::run("bash", &["-c", "which kakaotalk 2>/dev/null || test -f /opt/kakaotalk/kakaotalk.exe"]).await;
    let kakaotalk_installed = kt.success;

    AppsStatus { kakaotalk_installed, packages }
}
