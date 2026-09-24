use super::ime::{TYPE_SCREEN_TITLE, TYPE_BODY, TYPE_CAPTION, FONT_BOLD, FONT_SEMIBOLD};
use std::process::Stdio;
use std::time::{Duration, Instant};

use iced::{
    widget::{column, container, row, text, text_input, Space},
    Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_BTN2, C_DIM, C_ERR, C_OK, C_TEXT, C_WARN};

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
    GuardRefresh,
    GuardScanned(GuardStatus),
    ThresholdChanged(String),
    GuardInstall,
    GuardUninstall,
    GuardCheck,
    GuardDone(CmdResult),
}

/// 배터리 가드 설치 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardState {
    Missing,
    /// 파일은 있지만 구버전이거나 타이머가 꺼져 있음
    Inactive,
    Active,
}

#[derive(Debug, Clone)]
pub struct GuardStatus {
    pub state: GuardState,
    pub threshold: Option<u32>,
    pub battery: String,
}

pub struct PowerState {
    pub running: Option<String>,
    pub inhibit_active: bool,
    inhibit_child: Option<tokio::process::Child>,
    pub minutes_input: String,
    schedule_deadline: Option<Instant>,
    pub schedule_minutes: u32,
    guard: Option<GuardStatus>,
    threshold_input: String,
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
            guard: None,
            threshold_input: String::new(),
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
            PowerMsg::GuardRefresh => (Task::perform(scan_guard(), PowerMsg::GuardScanned), None),
            PowerMsg::GuardScanned(st) => {
                // 입력칸이 비어 있으면 설치된 임계값으로 채워 둔다
                if self.threshold_input.is_empty() {
                    if let Some(t) = st.threshold {
                        self.threshold_input = t.to_string();
                    }
                }
                self.guard = Some(st);
                (Task::none(), None)
            }
            PowerMsg::ThresholdChanged(s) => {
                self.threshold_input = s.chars().filter(|c| c.is_ascii_digit()).take(2).collect();
                (Task::none(), None)
            }
            PowerMsg::GuardInstall => {
                let threshold = parse_threshold(&self.threshold_input);
                self.threshold_input = threshold.to_string();
                self.running = Some("배터리 자동 절전 설치 중 (비밀번호 확인)...".into());
                (Task::perform(install_guard(threshold), PowerMsg::GuardDone), None)
            }
            PowerMsg::GuardUninstall => {
                self.running = Some("배터리 자동 절전 제거 중 (비밀번호 확인)...".into());
                (Task::perform(uninstall_guard(), PowerMsg::GuardDone), None)
            }
            PowerMsg::GuardCheck => {
                let installed = self.guard.as_ref().is_some_and(|g| g.state != GuardState::Missing);
                (Task::perform(check_guard(installed), PowerMsg::GuardDone), None)
            }
            PowerMsg::GuardDone(r) => {
                self.running = None;
                (Task::perform(scan_guard(), PowerMsg::GuardScanned), Some(r))
            }
        }
    }

    pub fn view(&self) -> Element<'_, PowerMsg> {
        let mut col = column![
            text("전원").size(TYPE_SCREEN_TITLE).font(FONT_BOLD),
            Space::with_height(6),
            text("절전모드 진입, 절전 방지, 예약 절전을 제어합니다.")
                .size(TYPE_CAPTION)
                .color(C_DIM),
            Space::with_height(16),
        ];

        if let Some(label) = &self.running {
            col = col.push(running_bar(label)).push(Space::with_height(12));
        }

        let idle = self.running.is_none();

        col = col.push(card(
            column![
                text("지금 절전모드").size(TYPE_BODY).font(FONT_SEMIBOLD),
                Space::with_height(4),
                text("화면과 시스템을 즉시 대기 상태(suspend)로 전환합니다.").size(TYPE_CAPTION).color(C_DIM),
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
                text("절전 방지").size(TYPE_BODY).font(FONT_SEMIBOLD),
                Space::with_height(4),
                text(format!(
                    "켜면 화면 잠금/절전이 자동으로 일어나지 않습니다 (최대 {}시간, 앱 종료 시에도 해제하는 걸 권장).",
                    MAX_INHIBIT_SECS / 3600
                )).size(TYPE_CAPTION).color(C_DIM),
                Space::with_height(10),
                row![
                    Space::with_width(Length::Fill),
                    action_btn(inhibit_label, PowerMsg::ToggleInhibit, idle, inhibit_color),
                ],
            ]
        ));
        col = col.push(Space::with_height(10));

        let mut schedule_body = column![
            text("예약 절전모드").size(TYPE_BODY).font(FONT_SEMIBOLD),
            Space::with_height(4),
        ];
        if let Some(deadline) = self.schedule_deadline {
            let remaining = deadline.saturating_duration_since(Instant::now()).as_secs();
            let (m, s) = (remaining / 60, remaining % 60);
            schedule_body = schedule_body.push(
                text(format!("{}분 후 절전 예약됨 — 남은 시간 {m:02}:{s:02}", self.schedule_minutes))
                    .size(TYPE_CAPTION).color(C_TEXT)
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
                text("지정한 시간 뒤 자동으로 절전모드에 진입합니다.").size(TYPE_CAPTION).color(C_DIM)
            );
            schedule_body = schedule_body.push(Space::with_height(10));
            schedule_body = schedule_body.push(
                row![
                    text_input("분", &self.minutes_input)
                .style(eond_ui_theme::iced_theme::text_input::default)
                        .on_input(PowerMsg::MinutesChanged)
                        .padding([8, 10])
                        .size(TYPE_BODY)
                        .width(80),
                    Space::with_width(8),
                    text("분 후").size(TYPE_CAPTION).color(C_DIM),
                    Space::with_width(Length::Fill),
                    action_btn("예약", PowerMsg::Schedule, idle, C_BLUE),
                ]
                .align_y(iced::Alignment::Center)
            );
        }
        col = col.push(card(schedule_body));
        col = col.push(Space::with_height(10));
        col = col.push(self.guard_card(idle));

        container(col).padding([4, 0]).into()
    }

    fn guard_card(&self, idle: bool) -> Element<'_, PowerMsg> {
        let (status_txt, status_col) = match &self.guard {
            None => ("확인 중...".to_string(), C_DIM),
            Some(g) => match g.state {
                GuardState::Active => {
                    let t = g.threshold.unwrap_or(DEFAULT_THRESHOLD);
                    (format!("켜짐 — 배터리 {t}% 이하에서 절전"), C_OK)
                }
                GuardState::Inactive => ("구버전 또는 꺼져 있음 — 다시 켜세요".to_string(), C_WARN),
                GuardState::Missing => ("꺼짐 — 2%에서 전원이 꺼질 수 있음".to_string(), C_ERR),
            },
        };
        let battery = self.guard.as_ref().map(|g| g.battery.clone()).unwrap_or_default();
        let installed = self.guard.as_ref().is_some_and(|g| g.state != GuardState::Missing);

        let mut buttons = row![
            text_input("5", &self.threshold_input)
                .style(eond_ui_theme::iced_theme::text_input::default)
                .on_input(PowerMsg::ThresholdChanged)
                .padding([8, 10])
                .size(TYPE_BODY)
                .width(60),
            Space::with_width(8),
            text("% 이하").size(TYPE_CAPTION).color(C_DIM),
            Space::with_width(Length::Fill),
            action_btn("지금 점검", PowerMsg::GuardCheck, idle, C_BTN2),
            Space::with_width(8),
        ]
        .align_y(iced::Alignment::Center);
        if installed {
            buttons = buttons
                .push(action_btn("끄기 (pkexec)", PowerMsg::GuardUninstall, idle, C_BTN2))
                .push(Space::with_width(8))
                .push(action_btn("임계값 적용 (pkexec)", PowerMsg::GuardInstall, idle, C_BLUE));
        } else {
            buttons = buttons.push(action_btn("켜기 (pkexec)", PowerMsg::GuardInstall, idle, C_BLUE));
        }

        card(column![
            text("배터리 부족 시 자동 절전").size(TYPE_BODY).font(FONT_SEMIBOLD),
            Space::with_height(4),
            text(
                "배터리로 쓰는 중 잔량이 지정한 % 이하가 되면 강제 종료 전에 절전합니다. \
                 복귀 후 3분은 다시 잠들지 않습니다.\n\
                 (이 PC 는 최대절전이 불가해 기본 UPower 가 2%에서 전원을 끕니다 — 작업 데이터 유실)"
            ).size(TYPE_CAPTION).color(C_DIM),
            Space::with_height(8),
            text(status_txt).size(TYPE_CAPTION).color(status_col),
            text(battery).size(TYPE_CAPTION).color(C_DIM),
            Space::with_height(10),
            buttons,
        ])
    }
}

// ─── 배터리 가드 ─────────────────────────────────────────────────────────────
//
// UPower 1.90.3 은 CriticalPowerAction=Suspend 를 지원하지 않고, 이 PC 는 디스크 스왑이 없어
// HybridSleep 이 불가능하다 → UPower 가 PowerOff 로 폴백해 2%에서 전원이 꺼진다.
// 그래서 root systemd 타이머(1분)가 방전 중 임계값 이하일 때 먼저 절전시킨다.
// popmgr 가 꺼져 있어도 동작해야 하므로 앱 내부 폴링이 아니라 시스템 유닛으로 설치한다.

const DEFAULT_THRESHOLD: u32 = 5;
const GUARD_MARKER: &str = "# popmgr-battery-guard: v1";
const GUARD_SCRIPT_PATH: &str = "/usr/local/lib/popmgr/battery-guard.sh";
const GUARD_CONF_PATH: &str = "/etc/popmgr/battery-guard.conf";
const GUARD_SERVICE_PATH: &str = "/etc/systemd/system/popmgr-battery-guard.service";
const GUARD_TIMER_PATH: &str = "/etc/systemd/system/popmgr-battery-guard.timer";
const GUARD_HOOK_PATH: &str = "/etc/systemd/system-sleep/zz-popmgr-battery-guard";
const GUARD_TIMER: &str = "popmgr-battery-guard.timer";

/// 출력은 한 줄, 첫 단어가 상태 토큰(OK / SKIP / WOULD_SUSPEND / SUSPEND).
/// PSU_ROOT·CONF·STATE_DIR·DRY_RUN·NOW 는 테스트용으로 덮어쓸 수 있다.
const GUARD_SCRIPT: &str = r#"#!/bin/sh
# popmgr-battery-guard: v1
# 배터리 방전 중 잔량이 THRESHOLD% 이하면 UPower 의 강제 전원 차단 전에 절전한다.
PSU_ROOT="${PSU_ROOT:-/sys/class/power_supply}"
CONF="${CONF:-/etc/popmgr/battery-guard.conf}"
STATE_DIR="${STATE_DIR:-/run}"
NOW="${NOW:-$(date +%s)}"

THRESHOLD=5
GRACE_SECS=180
[ -r "$CONF" ] && . "$CONF"
case "$THRESHOLD" in ''|*[!0-9]*) THRESHOLD=5 ;; esac
case "$GRACE_SECS" in ''|*[!0-9]*) GRACE_SECS=180 ;; esac
[ "$THRESHOLD" -lt 1 ] && THRESHOLD=1
[ "$THRESHOLD" -gt 30 ] && THRESHOLD=30

# 시스템 배터리만 대상(scope=Device 인 무선 마우스 등 주변기기 배터리 제외)
bats=""
ac_online=0
for d in "$PSU_ROOT"/*; do
  [ -r "$d/type" ] || continue
  case "$(cat "$d/type")" in
    Battery)
      if [ -r "$d/scope" ] && [ "$(cat "$d/scope")" != "System" ]; then continue; fi
      bats="$bats $d"
      ;;
    Mains)
      [ "$(cat "$d/online" 2>/dev/null)" = "1" ] && ac_online=1
      ;;
  esac
done

[ -n "$bats" ] || { echo "SKIP no-battery"; exit 0; }
[ "$ac_online" = 1 ] && { echo "SKIP on-ac"; exit 0; }

sum=0
n=0
for d in $bats; do
  [ "$(cat "$d/status" 2>/dev/null)" = "Discharging" ] || { echo "SKIP not-discharging"; exit 0; }
  c="$(cat "$d/capacity" 2>/dev/null)"
  case "$c" in ''|*[!0-9]*) echo "SKIP no-capacity"; exit 0 ;; esac
  sum=$((sum + c))
  n=$((n + 1))
done
cap=$((sum / n))
[ "$cap" -gt "$THRESHOLD" ] && { echo "OK ${cap}%"; exit 0; }

# 복귀 직후 유예: 없으면 1분 뒤 다시 잠들어 저장·충전기 연결을 할 수 없다
resumed="$(cat "$STATE_DIR/popmgr-battery-guard.resumed" 2>/dev/null)"
case "$resumed" in ''|*[!0-9]*) resumed=0 ;; esac
if [ "$resumed" -gt $((NOW - GRACE_SECS)) ]; then echo "SKIP grace"; exit 0; fi

if [ "$DRY_RUN" = 1 ]; then echo "WOULD_SUSPEND ${cap}%"; exit 0; fi
logger -t popmgr-battery-guard "battery ${cap}% <= ${THRESHOLD}% -> suspend" 2>/dev/null
# 복귀 훅이 실패해도 유예가 동작하도록 절전 직전에 기록
echo "$NOW" > "$STATE_DIR/popmgr-battery-guard.resumed"
# -i: 억제(절전 방지) 무시 — 여기서 안 자면 곧 UPower 가 전원을 끈다
systemctl suspend -i
echo "SUSPEND ${cap}%"
"#;

const GUARD_HOOK: &str = r#"#!/bin/sh
# popmgr-battery-guard-hook: v1
# 복귀 시각을 기록해 battery-guard 가 복귀 직후 다시 절전하지 않게 한다.
[ "$1" = "post" ] || exit 0
date +%s > /run/popmgr-battery-guard.resumed
"#;

const GUARD_SERVICE: &str = "[Unit]
Description=popmgr battery guard (suspend before critical power-off)

[Service]
Type=oneshot
ExecStart=/usr/local/lib/popmgr/battery-guard.sh
";

const GUARD_TIMER_UNIT: &str = "[Unit]
Description=popmgr battery guard check every minute

[Timer]
OnBootSec=1min
OnUnitActiveSec=1min
AccuracySec=15s

[Install]
WantedBy=timers.target
";

/// 임계값 입력 → 1..=30 (비었거나 숫자가 없으면 기본값)
fn parse_threshold(s: &str) -> u32 {
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    match digits.parse::<u32>() {
        Ok(v) => v.clamp(1, 30),
        Err(_) => DEFAULT_THRESHOLD,
    }
}

fn parse_conf_threshold(conf: &str) -> Option<u32> {
    conf.lines()
        .find_map(|l| l.trim().strip_prefix("THRESHOLD="))
        .and_then(|v| v.trim().parse::<u32>().ok())
}

async fn scan_guard() -> GuardStatus {
    let script = tokio::fs::read_to_string(GUARD_SCRIPT_PATH).await.ok();
    let enabled = runner::run("systemctl", &["is-enabled", GUARD_TIMER]).await;
    let state = match script {
        None => GuardState::Missing,
        Some(s) if s.contains(GUARD_MARKER) && enabled.output.trim() == "enabled" => GuardState::Active,
        Some(_) => GuardState::Inactive,
    };
    let threshold = tokio::fs::read_to_string(GUARD_CONF_PATH)
        .await
        .ok()
        .and_then(|c| parse_conf_threshold(&c));
    GuardStatus { state, threshold, battery: read_battery().await }
}

/// "현재 배터리 14% · 충전 중" 형태의 한 줄
async fn read_battery() -> String {
    let Ok(mut dir) = tokio::fs::read_dir("/sys/class/power_supply").await else {
        return String::new();
    };
    while let Ok(Some(entry)) = dir.next_entry().await {
        let p = entry.path();
        let kind = tokio::fs::read_to_string(p.join("type")).await.unwrap_or_default();
        if kind.trim() != "Battery" {
            continue;
        }
        let scope = tokio::fs::read_to_string(p.join("scope")).await.unwrap_or_default();
        if !scope.is_empty() && scope.trim() != "System" {
            continue;
        }
        let cap = tokio::fs::read_to_string(p.join("capacity")).await.unwrap_or_default();
        let status = tokio::fs::read_to_string(p.join("status")).await.unwrap_or_default();
        let status = match status.trim() {
            "Charging" => "충전 중",
            "Discharging" => "배터리 사용 중",
            "Full" => "완충",
            "Not charging" => "충전 안 함(전원 연결)",
            other => other,
        };
        return format!("현재 배터리 {}% · {status}", cap.trim());
    }
    "배터리를 찾지 못함".to_string()
}

fn unique_tmp_dir() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("popmgr-battery-guard.{}.{nanos}", std::process::id()))
}

async fn install_guard(threshold: u32) -> CmdResult {
    let dir = unique_tmp_dir();
    let conf = format!("THRESHOLD={threshold}\nGRACE_SECS=180\n");
    let files = [
        ("battery-guard.sh", GUARD_SCRIPT),
        ("battery-guard.conf", conf.as_str()),
        ("guard.service", GUARD_SERVICE),
        ("guard.timer", GUARD_TIMER_UNIT),
        ("guard-hook", GUARD_HOOK),
    ];
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return CmdResult { success: false, output: format!("임시 디렉터리 생성 실패: {e}") };
    }
    for (name, body) in files {
        if let Err(e) = tokio::fs::write(dir.join(name), body).await {
            let _ = tokio::fs::remove_dir_all(&dir).await;
            return CmdResult { success: false, output: format!("임시 파일 쓰기 실패: {e}") };
        }
    }
    let t = dir.display();
    let inner = format!(
        "set -e; \
         install -D -m 755 -o root -g root {t}/battery-guard.sh {GUARD_SCRIPT_PATH}; \
         install -D -m 644 -o root -g root {t}/battery-guard.conf {GUARD_CONF_PATH}; \
         install -D -m 644 -o root -g root {t}/guard.service {GUARD_SERVICE_PATH}; \
         install -D -m 644 -o root -g root {t}/guard.timer {GUARD_TIMER_PATH}; \
         install -D -m 755 -o root -g root {t}/guard-hook {GUARD_HOOK_PATH}; \
         systemctl daemon-reload; \
         systemctl enable --now {GUARD_TIMER}"
    );
    let r = runner::run("pkexec", &["sh", "-c", &inner]).await;
    let _ = tokio::fs::remove_dir_all(&dir).await;
    if r.success {
        CmdResult {
            success: true,
            output: format!(
                "배터리 자동 절전 켜짐: 방전 중 {threshold}% 이하에서 절전합니다 (1분마다 점검)."
            ),
        }
    } else {
        r
    }
}

async fn uninstall_guard() -> CmdResult {
    let inner = format!(
        "systemctl disable --now {GUARD_TIMER} 2>/dev/null; \
         rm -f {GUARD_SCRIPT_PATH} {GUARD_CONF_PATH} {GUARD_SERVICE_PATH} {GUARD_TIMER_PATH} {GUARD_HOOK_PATH}; \
         rm -f /run/popmgr-battery-guard.resumed; \
         systemctl daemon-reload"
    );
    let r = runner::run("pkexec", &["sh", "-c", &inner]).await;
    if r.success {
        CmdResult { success: true, output: "배터리 자동 절전 꺼짐".into() }
    } else {
        r
    }
}

/// 절전하지 않고 지금 조건이면 어떻게 될지만 본다(권한 불필요 — /sys·/run 읽기만).
async fn check_guard(installed: bool) -> CmdResult {
    let out = if installed {
        tokio::process::Command::new("sh").arg(GUARD_SCRIPT_PATH).env("DRY_RUN", "1").output().await
    } else {
        tokio::process::Command::new("sh")
            .args(["-c", GUARD_SCRIPT])
            .env("DRY_RUN", "1")
            .output()
            .await
    };
    match out {
        Ok(o) => {
            let line = String::from_utf8_lossy(&o.stdout).trim().to_string();
            CmdResult { success: o.status.success(), output: format!("배터리 가드 점검: {}", describe(&line)) }
        }
        Err(e) => CmdResult { success: false, output: format!("점검 실행 실패: {e}") },
    }
}

fn describe(line: &str) -> String {
    let msg = match line.split_whitespace().next().unwrap_or("") {
        "OK" => "잔량 충분 — 절전 안 함",
        "WOULD_SUSPEND" => "지금 조건이면 절전함",
        "SKIP" if line.contains("on-ac") || line.contains("not-discharging") => "전원 연결 중 — 절전 안 함",
        "SKIP" if line.contains("grace") => "복귀 직후 유예 중 — 절전 안 함",
        "SKIP" => "배터리 정보 없음 — 절전 안 함",
        _ => "알 수 없는 결과",
    };
    format!("{msg} ({line})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 가짜 /sys/class/power_supply 와 conf·state 디렉터리
    struct Fake {
        root: PathBuf,
    }

    impl Fake {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!("popmgr-guard-test.{}.{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("psu")).unwrap();
            std::fs::create_dir_all(root.join("state")).unwrap();
            std::fs::write(root.join("guard.sh"), GUARD_SCRIPT).unwrap();
            std::fs::write(root.join("guard.conf"), "THRESHOLD=5\nGRACE_SECS=180\n").unwrap();
            Self { root }
        }

        fn supply(&self, name: &str, files: &[(&str, &str)]) {
            let d = self.root.join("psu").join(name);
            std::fs::create_dir_all(&d).unwrap();
            for (f, v) in files {
                std::fs::write(d.join(f), format!("{v}\n")).unwrap();
            }
        }

        fn battery(&self, status: &str, cap: u32) {
            self.supply("BAT0", &[("type", "Battery"), ("status", status), ("capacity", &cap.to_string())]);
        }

        fn write(&self, rel: &str, body: &str) {
            std::fs::write(self.root.join(rel), body).unwrap();
        }

        fn run(&self) -> String {
            let out = std::process::Command::new("sh")
                .arg(self.root.join("guard.sh"))
                .env("PSU_ROOT", self.root.join("psu"))
                .env("CONF", self.root.join("guard.conf"))
                .env("STATE_DIR", self.root.join("state"))
                .env("DRY_RUN", "1")
                .env("NOW", "100000")
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn discharging_below_threshold_suspends() {
        let f = Fake::new("low");
        f.battery("Discharging", 4);
        assert_eq!(f.run(), "WOULD_SUSPEND 4%");
    }

    #[test]
    fn discharging_above_threshold_ok() {
        let f = Fake::new("high");
        f.battery("Discharging", 6);
        assert_eq!(f.run(), "OK 6%");
    }

    #[test]
    fn charging_skips() {
        let f = Fake::new("charging");
        f.battery("Charging", 3);
        assert_eq!(f.run(), "SKIP not-discharging");
    }

    #[test]
    fn ac_online_skips() {
        let f = Fake::new("ac");
        f.battery("Discharging", 3);
        f.supply("AC", &[("type", "Mains"), ("online", "1")]);
        assert_eq!(f.run(), "SKIP on-ac");
    }

    #[test]
    fn recent_resume_is_grace() {
        let f = Fake::new("grace");
        f.battery("Discharging", 3);
        f.write("state/popmgr-battery-guard.resumed", "99940\n");
        assert_eq!(f.run(), "SKIP grace");
    }

    #[test]
    fn old_resume_suspends() {
        let f = Fake::new("oldresume");
        f.battery("Discharging", 3);
        f.write("state/popmgr-battery-guard.resumed", "99400\n");
        assert_eq!(f.run(), "WOULD_SUSPEND 3%");
    }

    #[test]
    fn no_battery_skips() {
        let f = Fake::new("nobat");
        f.supply("AC", &[("type", "Mains"), ("online", "0")]);
        assert_eq!(f.run(), "SKIP no-battery");
    }

    #[test]
    fn device_scope_battery_ignored() {
        let f = Fake::new("device");
        f.supply(
            "hidpp_battery_0",
            &[("type", "Battery"), ("scope", "Device"), ("status", "Discharging"), ("capacity", "1")],
        );
        assert_eq!(f.run(), "SKIP no-battery");
    }

    #[test]
    fn bad_conf_threshold_falls_back_to_default() {
        let f = Fake::new("badconf");
        f.write("guard.conf", "THRESHOLD=abc\n");
        f.battery("Discharging", 4);
        assert_eq!(f.run(), "WOULD_SUSPEND 4%");
    }

    #[test]
    fn threshold_input_parsing() {
        assert_eq!(parse_threshold(""), 5);
        assert_eq!(parse_threshold("0"), 1);
        assert_eq!(parse_threshold("31"), 30);
        assert_eq!(parse_threshold("7"), 7);
        assert_eq!(parse_threshold("a8"), 8);
    }

    #[test]
    fn conf_threshold_parsing() {
        assert_eq!(parse_conf_threshold("THRESHOLD=7\nGRACE_SECS=180\n"), Some(7));
        assert_eq!(parse_conf_threshold("GRACE_SECS=180\n"), None);
    }
}

fn apply(script: String) -> Task<PowerMsg> {
    Task::perform(async move { runner::run_sh(&script).await }, PowerMsg::Applied)
}

/// 로그인 세션 사용자가 (일반적으로 pkexec 없이) 절전모드를 요청.
fn suspend_script() -> String {
    "systemctl suspend && echo '절전모드에서 복귀했습니다'".to_string()
}
