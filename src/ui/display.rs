use iced::{
    widget::{column, container, row, scrollable, slider, text, Space},
    Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_DIM, C_BTN2, C_WARN, C_TEXT};

/// 모니터 종류. 내장(eDP)은 logind/sysfs, 외부는 DDC/CI(ddcutil)로 제어한다.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// 내장 디스플레이. sysfs 백라이트 디렉터리 이름(예: intel_backlight).
    Internal { backlight: String },
    /// 외부 모니터. ddcutil 의 디스플레이 번호(`-d N`).
    External { display: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    pub kind: Kind,
    pub name: String,       // 사람이 읽는 이름 (모델명 등)
    pub connector: String,  // DRM 커넥터 (eDP-1, DP-1 ...)
    pub pct: u32,           // 현재 밝기 0~100 (슬라이더 값)
    pub raw_max: u32,       // 밝기 원시 최대값 (내장=max_brightness, 외부=VCP max)
    pub contrast: Option<u32>, // 외부 모니터 명암 0~100 (지원 시)
    pub contrast_max: u32,
}

/// 외부 모니터 제어가 막혀 있을 때의 사유.
#[derive(Debug, Clone, PartialEq)]
pub enum SetupReason {
    NoDdcutil,   // ddcutil 미설치
    Permission,  // /dev/i2c-* 접근 권한 없음 (i2c 그룹/udev 룰/모듈 미설정)
}

#[derive(Debug, Clone)]
pub enum DisplayMsg {
    Refresh,
    Refreshed(Vec<Monitor>, Option<SetupReason>),
    /// 드래그 중 로컬 갱신 (idx, 값). 즉시 적용하지 않는다.
    SetBrightness(usize, u32),
    /// 슬라이더를 놓을 때 실제 적용 (idx).
    CommitBrightness(usize),
    SetContrast(usize, u32),
    CommitContrast(usize),
    SetupPermissions,
    /// i2c-dev 모듈 재로드 + udev 재트리거 후 재스캔 (재부팅 후 모듈 미로드 대비).
    Reprobe,
    Applied(CmdResult),
    /// 작업 후 곧바로 재스캔이 필요한 경우(권한 설정·재인식).
    AppliedRescan(CmdResult),
}

pub struct DisplayState {
    pub monitors: Vec<Monitor>,
    pub setup: Option<SetupReason>,
    pub scanned: bool,
    pub running: Option<String>,
}

impl DisplayState {
    pub fn new() -> Self {
        Self { monitors: Vec::new(), setup: None, scanned: false, running: None }
    }

    pub fn update(&mut self, msg: DisplayMsg) -> (Task<DisplayMsg>, Option<CmdResult>) {
        match msg {
            DisplayMsg::Refresh => {
                (Task::perform(async { scan().await }, |(m, s)| DisplayMsg::Refreshed(m, s)), None)
            }
            DisplayMsg::Refreshed(m, s) => {
                // 드래그 중(running)이면 사용자 조작값을 덮어쓰지 않도록 보호
                if self.running.is_none() {
                    self.monitors = m;
                    self.setup = s;
                }
                self.scanned = true;
                (Task::none(), None)
            }
            DisplayMsg::SetBrightness(i, v) => {
                if let Some(mon) = self.monitors.get_mut(i) { mon.pct = v; }
                (Task::none(), None)
            }
            DisplayMsg::CommitBrightness(i) => {
                let Some(mon) = self.monitors.get(i) else { return (Task::none(), None) };
                let script = brightness_script(mon);
                (apply(script), None)
            }
            DisplayMsg::SetContrast(i, v) => {
                if let Some(mon) = self.monitors.get_mut(i) { mon.contrast = Some(v); }
                (Task::none(), None)
            }
            DisplayMsg::CommitContrast(i) => {
                let Some(mon) = self.monitors.get(i) else { return (Task::none(), None) };
                let Some(script) = contrast_script(mon) else { return (Task::none(), None) };
                (apply(script), None)
            }
            DisplayMsg::SetupPermissions => {
                self.running = Some("외부 모니터 제어 권한 설정 중... (관리자 인증)".into());
                (apply_rescan(setup_script()), None)
            }
            DisplayMsg::Reprobe => {
                self.running = Some("i2c 모듈 재로드 및 모니터 재인식 중... (관리자 인증)".into());
                (apply_rescan(reprobe_script()), None)
            }
            DisplayMsg::Applied(r) => {
                self.running = None;
                (Task::none(), Some(r))
            }
            DisplayMsg::AppliedRescan(r) => {
                self.running = None;
                let t = Task::perform(async { scan().await }, |(m, s)| DisplayMsg::Refreshed(m, s));
                (t, Some(r))
            }
        }
    }

    pub fn view(&self) -> Element<'_, DisplayMsg> {
        let mut col = column![
            text("디스플레이").size(20),
            Space::with_height(6),
            text("내장·외부 모니터의 밝기를 조절합니다. 외부 모니터는 DDC/CI(모니터 OSD를 소프트웨어로 제어)로 동작하며, COSMIC 상단바에는 나타나지 않습니다.")
                .size(11)
                .color(C_DIM),
            Space::with_height(16),
        ];

        if let Some(label) = &self.running {
            col = col.push(running_bar(label)).push(Space::with_height(12));
        }

        if !self.scanned {
            col = col.push(text("스캔 중...").size(13).color(C_DIM));
            return scrollable(container(col).padding([4, 0])).into();
        }

        for (i, mon) in self.monitors.iter().enumerate() {
            col = col.push(monitor_card(i, mon, self.running.is_some()));
            col = col.push(Space::with_height(10));
        }

        if let Some(reason) = &self.setup {
            col = col.push(setup_card(reason, self.running.is_some()));
            col = col.push(Space::with_height(10));
        }

        col = col.push(Space::with_height(8));
        let idle = self.running.is_none();
        let actions = row![
            text("모니터가 안 보이면 '재인식'을 누르세요 (i2c 모듈 재로드)").size(10).color(C_DIM),
            Space::with_width(Length::Fill),
            action_btn("재인식", DisplayMsg::Reprobe, idle, C_BTN2),
            Space::with_width(8),
            action_btn("새로고침", DisplayMsg::Refresh, idle, C_BLUE),
        ]
        .align_y(iced::Alignment::Center);
        col = col.push(actions);

        scrollable(container(col).padding([4, 0])).into()
    }
}

fn monitor_card(i: usize, mon: &Monitor, busy: bool) -> Element<'_, DisplayMsg> {
    let tag = match &mon.kind {
        Kind::Internal { .. } => "내장",
        Kind::External { .. } => "외부",
    };
    let mut body = column![
        row![
            text(&mon.name).size(14),
            Space::with_width(8),
            text(format!("[{tag}] {}", mon.connector)).size(11).color(C_BLUE),
        ].align_y(iced::Alignment::Center),
        Space::with_height(10),
    ];

    // 밝기 슬라이더
    let pct = mon.pct;
    body = body.push(
        row![
            container(text("밝기").size(12).color(C_DIM)).width(48),
            slider(0..=100, pct, move |v| DisplayMsg::SetBrightness(i, v))
                .on_release(DisplayMsg::CommitBrightness(i))
                .width(Length::Fill),
            Space::with_width(8),
            container(text(format!("{pct}%")).size(12).color(C_TEXT)).width(40),
        ]
        .align_y(iced::Alignment::Center)
    );

    // 명암 슬라이더 (외부 모니터 + 지원 시)
    if let Some(c) = mon.contrast {
        body = body.push(Space::with_height(8));
        body = body.push(
            row![
                container(text("명암").size(12).color(C_DIM)).width(48),
                slider(0..=100, c, move |v| DisplayMsg::SetContrast(i, v))
                    .on_release(DisplayMsg::CommitContrast(i))
                    .width(Length::Fill),
                Space::with_width(8),
                container(text(format!("{c}%")).size(12).color(C_TEXT)).width(40),
            ]
            .align_y(iced::Alignment::Center)
        );
    }

    if busy {
        body = body.push(Space::with_height(6));
        body = body.push(text("적용 중...").size(10).color(C_DIM));
    }

    card(body)
}

fn setup_card(reason: &SetupReason, busy: bool) -> Element<'_, DisplayMsg> {
    let (msg, btn_label): (&str, &str) = match reason {
        SetupReason::NoDdcutil => (
            "외부 모니터 밝기 제어에는 ddcutil 이 필요합니다. 터미널에서 sudo apt install ddcutil 로 설치한 뒤 새로고침하세요.",
            "권한 설정",
        ),
        SetupReason::Permission => (
            "외부 모니터가 연결돼 있지만 /dev/i2c-* 접근 권한이 없어 제어할 수 없습니다. 아래 버튼으로 i2c 그룹·udev 룰·모듈을 설정하세요. 설정 후 로그아웃 → 재로그인하면 활성화됩니다.",
            "권한 설정",
        ),
    };
    let mut body = column![
        text("외부 모니터 제어 권한").size(14).color(C_WARN),
        Space::with_height(8),
        text(msg).size(11).color(C_DIM),
        Space::with_height(12),
    ];
    body = body.push(
        row![
            Space::with_width(Length::Fill),
            action_btn(btn_label, DisplayMsg::SetupPermissions, !busy, C_BLUE),
        ]
    );
    card(body)
}

fn apply(script: String) -> Task<DisplayMsg> {
    Task::perform(async move { runner::run_sh(&script).await }, DisplayMsg::Applied)
}

fn apply_rescan(script: String) -> Task<DisplayMsg> {
    Task::perform(async move { runner::run_sh(&script).await }, DisplayMsg::AppliedRescan)
}

/// i2c-dev 모듈을 (재)로드하고 udev 를 다시 트리거해 모니터 i2c 장치를 재인식.
fn reprobe_script() -> String {
    "pkexec bash -c \"modprobe i2c-dev; udevadm trigger --subsystem-match=i2c-dev; udevadm trigger; echo 'i2c 모듈 재로드 및 장치 재인식 완료'\"".to_string()
}

/// 밝기 적용 스크립트. 내장은 logind, 외부는 ddcutil.
fn brightness_script(mon: &Monitor) -> String {
    match &mon.kind {
        Kind::Internal { backlight } => {
            // 슬라이더 % → sysfs 원시값. 화면 완전 꺼짐 방지 위해 최소 1.
            let raw = ((mon.pct as u64 * mon.raw_max as u64) / 100).max(1);
            // 세션 사용자가 root 없이 백라이트를 바꾸는 표준 경로(logind).
            format!(
                "busctl call org.freedesktop.login1 /org/freedesktop/login1/session/auto \
                 org.freedesktop.login1.Session SetBrightness ssu backlight {backlight} {raw} \
                 && echo '내장 밝기 {}% 적용'",
                mon.pct
            )
        }
        Kind::External { display } => {
            let raw = (mon.pct as u64 * mon.raw_max as u64) / 100;
            format!("ddcutil -d {display} setvcp 10 {raw} && echo '외부({}) 밝기 {}% 적용'", mon.connector, mon.pct)
        }
    }
}

/// 명암 적용 스크립트 (외부 모니터 전용).
fn contrast_script(mon: &Monitor) -> Option<String> {
    let Kind::External { display } = &mon.kind else { return None };
    let c = mon.contrast?;
    let raw = (c as u64 * mon.contrast_max.max(1) as u64) / 100;
    Some(format!("ddcutil -d {display} setvcp 12 {raw} && echo '외부({}) 명암 {c}% 적용'", mon.connector))
}

/// 일회성 권한 설정 스크립트 (pkexec 로 root 실행).
fn setup_script() -> String {
    let user = std::env::var("USER").unwrap_or_default();
    let inner = format!(
        "set -e; \
         modprobe i2c-dev || true; \
         echo i2c-dev > /etc/modules-load.d/i2c-dev.conf; \
         getent group i2c >/dev/null || groupadd i2c; \
         printf 'KERNEL==\\\"i2c-[0-9]*\\\", GROUP=\\\"i2c\\\", MODE=\\\"0660\\\"\\n' > /etc/udev/rules.d/60-ddcutil-i2c.rules; \
         udevadm control --reload-rules; \
         udevadm trigger; \
         usermod -aG i2c '{user}'; \
         echo '권한 설정 완료 — 로그아웃 후 재로그인하면 외부 모니터 밝기 조절이 활성화됩니다.'"
    );
    format!("pkexec bash -c \"{}\"", inner.replace('"', "\\\""))
}

// ─── 스캔 ────────────────────────────────────────────────────────────────

async fn scan() -> (Vec<Monitor>, Option<SetupReason>) {
    let mut monitors = scan_internal().await;

    // ddcutil 설치 여부
    let has = runner::run_sh("command -v ddcutil >/dev/null 2>&1 && echo yes").await;
    if !has.output.contains("yes") {
        // 외부 모니터가 물리적으로 붙어 있을 때만 설치 안내
        if external_connected().await {
            return (monitors, Some(SetupReason::NoDdcutil));
        }
        return (monitors, None);
    }

    // ko_KR 등 비영어 로케일에서 ddcutil 출력 라벨이 번역되면 parse_displays가
    // 전부 깨져 모니터를 0개로 읽는다(audio.rs와 동일 부류). 영어 출력 강제.
    let det = runner::run_sh("LC_ALL=C ddcutil detect 2>&1").await;
    let displays = parse_displays(&det.output);

    if displays.is_empty() {
        // 외부 모니터는 붙어 있는데 ddcutil 이 못 잡으면 권한 문제로 본다.
        if external_connected().await {
            return (monitors, Some(SetupReason::Permission));
        }
        return (monitors, None);
    }

    for d in displays {
        // 밝기·명암을 한 번에 읽는다.
        let vcp = runner::run_sh(&format!("LC_ALL=C ddcutil -d {} getvcp 10 12 2>&1", d.number)).await;
        let (bright, bmax) = parse_vcp(&vcp.output, 0x10).unwrap_or((50, 100));
        let contrast = parse_vcp(&vcp.output, 0x12);
        let pct = if bmax > 0 { (bright * 100 / bmax).min(100) } else { 50 };
        monitors.push(Monitor {
            kind: Kind::External { display: d.number },
            name: if d.model.is_empty() { format!("외부 모니터 {}", d.number) } else { d.model },
            connector: d.connector,
            pct,
            raw_max: bmax.max(1),
            contrast: contrast.map(|(c, cm)| if cm > 0 { (c * 100 / cm).min(100) } else { c }),
            contrast_max: contrast.map(|(_, cm)| cm.max(1)).unwrap_or(100),
        });
    }

    (monitors, None)
}

/// sysfs 백라이트(내장 디스플레이) 스캔.
async fn scan_internal() -> Vec<Monitor> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir("/sys/class/backlight") else { return out };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let base = e.path();
        let cur = read_u32(&base.join("brightness"));
        let max = read_u32(&base.join("max_brightness"));
        let (Some(cur), Some(max)) = (cur, max) else { continue };
        if max == 0 { continue; }
        let pct = (cur * 100 / max).min(100);
        out.push(Monitor {
            kind: Kind::Internal { backlight: name.clone() },
            name: "내장 디스플레이".into(),
            connector: connector_for_backlight(&name),
            pct,
            raw_max: max,
            contrast: None,
            contrast_max: 0,
        });
    }
    out
}

fn read_u32(path: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// 백라이트 이름으로부터 DRM 커넥터(eDP-1 등)를 추정. 실패 시 백라이트 이름 반환.
fn connector_for_backlight(_name: &str) -> String {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else { return "eDP".into() };
    for e in entries.flatten() {
        let fname = e.file_name().to_string_lossy().to_string();
        if fname.contains("eDP") {
            if std::fs::read_to_string(e.path().join("status"))
                .map(|s| s.trim() == "connected").unwrap_or(false)
            {
                // card1-eDP-1 → eDP-1
                return fname.rsplit('-').take(2).collect::<Vec<_>>()
                    .into_iter().rev().collect::<Vec<_>>().join("-");
            }
        }
    }
    "eDP".into()
}

/// DRM 에서 eDP 가 아닌 connected 커넥터(=외부 모니터)가 있는지.
async fn external_connected() -> bool {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else { return false };
    for e in entries.flatten() {
        let fname = e.file_name().to_string_lossy().to_string();
        if !fname.contains('-') || fname.contains("eDP") { continue; }
        if std::fs::read_to_string(e.path().join("status"))
            .map(|s| s.trim() == "connected").unwrap_or(false)
        {
            return true;
        }
    }
    false
}

struct DetectedDisplay {
    number: u32,
    connector: String,
    model: String,
}

/// `ddcutil detect` 출력 파싱. "Invalid display"(eDP 등) 블록은 건너뛴다.
fn parse_displays(out: &str) -> Vec<DetectedDisplay> {
    let mut res = Vec::new();
    let mut cur: Option<DetectedDisplay> = None;
    let mut invalid = false;

    let flush = |cur: &mut Option<DetectedDisplay>, invalid: &mut bool, res: &mut Vec<DetectedDisplay>| {
        if let Some(d) = cur.take() {
            if !*invalid { res.push(d); }
        }
        *invalid = false;
    };

    for line in out.lines() {
        let t = line.trim();
        if let Some(rest) = line.strip_prefix("Display ") {
            flush(&mut cur, &mut invalid, &mut res);
            if let Ok(n) = rest.trim().parse::<u32>() {
                cur = Some(DetectedDisplay { number: n, connector: String::new(), model: String::new() });
            }
        } else if t == "Invalid display" {
            flush(&mut cur, &mut invalid, &mut res);
            invalid = true;
            // Invalid 블록도 임시로 담아 connector 추적은 불필요 — 그냥 무시
        } else if let Some(c) = t.strip_prefix("DRM connector:") {
            if let Some(d) = cur.as_mut() {
                // card1-DP-1 → DP-1
                let conn = c.trim();
                d.connector = conn.split_once('-').map(|(_, r)| r.to_string())
                    .unwrap_or_else(|| conn.to_string());
            }
        } else if let Some(m) = t.strip_prefix("Model:") {
            if let Some(d) = cur.as_mut() {
                d.model = m.trim().to_string();
            }
        }
    }
    flush(&mut cur, &mut invalid, &mut res);
    res
}

/// getvcp 출력에서 특정 VCP 코드의 (current, max) 추출.
/// 예: "VCP code 0x10 (Brightness ...): current value =   50, max value =  100"
fn parse_vcp(out: &str, code: u8) -> Option<(u32, u32)> {
    let needle = format!("0x{code:02x}");
    for line in out.lines() {
        let l = line.to_lowercase();
        if !l.contains(&needle) { continue; }
        let cur = extract_after(&l, "current value =")?;
        let max = extract_after(&l, "max value =")?;
        return Some((cur, max));
    }
    None
}

fn extract_after(line: &str, key: &str) -> Option<u32> {
    let idx = line.find(key)? + key.len();
    let tail = &line[idx..];
    let num: String = tail.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
    num.parse().ok()
}
