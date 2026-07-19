use std::process::Stdio;
use std::time::{Duration, Instant};

use iced::{
    widget::{column, container, row, text, text_input, Space},
    Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_BTN2, C_DIM, C_OK, C_TEXT};

/// 절전 방지 최대 유지 시간. 앱이 비정상 종료돼도 systemd-inhibit 프로세스가
/// 영구히 남아 절전을 막지 않도록 상한을 둔다.
const MAX_INHIBIT_SECS: u64 = 12 * 3600;

#[derive(Debug, Clone)]
pub enum PowerMsg {
    SuspendNow,
    ToggleInhibit,
    MinutesChanged(String),
    Schedule,
    CancelSchedule,
    Tick,
    Applied(CmdResult),
}

pub struct PowerState {
    pub running: Option<String>,
    pub inhibit_active: bool,
    inhibit_child: Option<tokio::process::Child>,
    pub minutes_input: String,
    schedule_deadline: Option<Instant>,
    pub schedule_minutes: u32,
}

impl PowerState {
    pub fn new() -> Self {
        Self {
            running: None,
            inhibit_active: false,
            inhibit_child: None,
            minutes_input: String::new(),
            schedule_deadline: None,
            schedule_minutes: 0,
        }
    }

    pub fn has_schedule(&self) -> bool {
        self.schedule_deadline.is_some()
    }

    pub fn update(&mut self, msg: PowerMsg) -> (Task<PowerMsg>, Option<CmdResult>) {
        match msg {
            PowerMsg::SuspendNow => {
                self.running = Some("절전모드 진입 중...".into());
                (apply(suspend_script()), None)
            }
            PowerMsg::ToggleInhibit => {
                if self.inhibit_active {
                    if let Some(mut child) = self.inhibit_child.take() {
                        let _ = child.start_kill();
                    }
                    self.inhibit_active = false;
                    (Task::none(), Some(CmdResult { success: true, output: "절전 방지 해제됨".into() }))
                } else {
                    let spawned = tokio::process::Command::new("systemd-inhibit")
                        .args([
                            "--what=sleep:idle".to_string(),
                            "--who=popmgr".to_string(),
                            "--why=사용자 요청으로 절전 방지".to_string(),
                            "--mode=block".to_string(),
                            "sleep".to_string(),
                            MAX_INHIBIT_SECS.to_string(),
                        ])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
                    match spawned {
                        Ok(child) => {
                            self.inhibit_child = Some(child);
                            self.inhibit_active = true;
                            (Task::none(), Some(CmdResult {
                                success: true,
                                output: format!("절전 방지 켜짐 (최대 {}시간, 앱 종료 또는 다시 누르면 해제)", MAX_INHIBIT_SECS / 3600),
                            }))
                        }
                        Err(e) => (Task::none(), Some(CmdResult {
                            success: false,
                            output: format!("systemd-inhibit 실행 실패: {e}"),
                        })),
                    }
                }
            }
            PowerMsg::MinutesChanged(s) => {
                self.minutes_input = s.chars().filter(|c| c.is_ascii_digit()).take(4).collect();
                (Task::none(), None)
            }
            PowerMsg::Schedule => {
                let Ok(mins) = self.minutes_input.parse::<u32>() else {
                    return (Task::none(), Some(CmdResult { success: false, output: "분(숫자)을 입력하세요".into() }));
                };
                if mins == 0 {
                    return (Task::none(), Some(CmdResult { success: false, output: "1분 이상 입력하세요".into() }));
                }
                self.schedule_deadline = Some(Instant::now() + Duration::from_secs(mins as u64 * 60));
                self.schedule_minutes = mins;
                (Task::none(), Some(CmdResult { success: true, output: format!("{mins}분 후 절전모드 예약됨") }))
            }
            PowerMsg::CancelSchedule => {
                self.schedule_deadline = None;
                (Task::none(), Some(CmdResult { success: true, output: "절전모드 예약 취소됨".into() }))
            }
            PowerMsg::Tick => {
                if let Some(deadline) = self.schedule_deadline {
                    if Instant::now() >= deadline {
                        self.schedule_deadline = None;
                        self.running = Some("예약된 절전모드 진입 중...".into());
                        return (apply(suspend_script()), None);
                    }
                }
                (Task::none(), None)
            }
            PowerMsg::Applied(r) => {
                self.running = None;
                (Task::none(), Some(r))
            }
        }
    }

    pub fn view(&self) -> Element<'_, PowerMsg> {
        let mut col = column![
            text("전원").size(20),
            Space::with_height(6),
            text("절전모드 진입, 절전 방지, 예약 절전을 제어합니다.")
                .size(11)
                .color(C_DIM),
            Space::with_height(16),
        ];

        if let Some(label) = &self.running {
            col = col.push(running_bar(label)).push(Space::with_height(12));
        }

        let idle = self.running.is_none();

        col = col.push(card(
            column![
                text("지금 절전모드").size(14),
                Space::with_height(4),
                text("화면과 시스템을 즉시 대기 상태(suspend)로 전환합니다.").size(11).color(C_DIM),
                Space::with_height(10),
                row![
                    Space::with_width(Length::Fill),
                    action_btn("지금 절전모드 진입", PowerMsg::SuspendNow, idle, C_BLUE),
                ],
            ]
        ));
        col = col.push(Space::with_height(10));

        let inhibit_label = if self.inhibit_active { "절전 방지 끄기" } else { "절전 방지 켜기" };
        let inhibit_color = if self.inhibit_active { C_OK } else { C_BTN2 };
        col = col.push(card(
            column![
                text("절전 방지").size(14),
                Space::with_height(4),
                text(format!(
                    "켜면 화면 잠금/절전이 자동으로 일어나지 않습니다 (최대 {}시간, 앱 종료 시에도 해제하는 걸 권장).",
                    MAX_INHIBIT_SECS / 3600
                )).size(11).color(C_DIM),
                Space::with_height(10),
                row![
                    Space::with_width(Length::Fill),
                    action_btn(inhibit_label, PowerMsg::ToggleInhibit, idle, inhibit_color),
                ],
            ]
        ));
        col = col.push(Space::with_height(10));

        let mut schedule_body = column![
            text("예약 절전모드").size(14),
            Space::with_height(4),
        ];
        if let Some(deadline) = self.schedule_deadline {
            let remaining = deadline.saturating_duration_since(Instant::now()).as_secs();
            let (m, s) = (remaining / 60, remaining % 60);
            schedule_body = schedule_body.push(
                text(format!("{}분 후 절전 예약됨 — 남은 시간 {m:02}:{s:02}", self.schedule_minutes))
                    .size(12).color(C_TEXT)
            );
            schedule_body = schedule_body.push(Space::with_height(10));
            schedule_body = schedule_body.push(
                row![
                    Space::with_width(Length::Fill),
                    action_btn("예약 취소", PowerMsg::CancelSchedule, idle, C_BTN2),
                ]
            );
        } else {
            schedule_body = schedule_body.push(
                text("지정한 시간 뒤 자동으로 절전모드에 진입합니다.").size(11).color(C_DIM)
            );
            schedule_body = schedule_body.push(Space::with_height(10));
            schedule_body = schedule_body.push(
                row![
                    text_input("분", &self.minutes_input)
                        .on_input(PowerMsg::MinutesChanged)
                        .padding([8, 10])
                        .size(13)
                        .width(80),
                    Space::with_width(8),
                    text("분 후").size(12).color(C_DIM),
                    Space::with_width(Length::Fill),
                    action_btn("예약", PowerMsg::Schedule, idle, C_BLUE),
                ]
                .align_y(iced::Alignment::Center)
            );
        }
        col = col.push(card(schedule_body));

        container(col).padding([4, 0]).into()
    }
}

fn apply(script: String) -> Task<PowerMsg> {
    Task::perform(async move { runner::run_sh(&script).await }, PowerMsg::Applied)
}

/// 로그인 세션 사용자가 (일반적으로 pkexec 없이) 절전모드를 요청.
fn suspend_script() -> String {
    "systemctl suspend && echo '절전모드에서 복귀했습니다'".to_string()
}
