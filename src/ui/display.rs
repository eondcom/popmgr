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
    pub controls_brightness: bool,
    pub geometry: Option<OutputGeometry>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputGeometry {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TouchDevice {
    pub name: String,
    pub event: String,
    pub phys: String,
    pub external: bool,
}

/// "저전력 모드" 버튼이 적용하는 밝기(%). 어두운 방에서 화면이 켜져 있는지만
/// 확인할 수 있으면 되는 용도라 낮게 잡는다.
const LOW_POWER_PCT: u32 = 10;
/// "기본 모드" 버튼이 적용하는 밝기(%).
const DEFAULT_PCT: u32 = 50;

/// 외부 모니터 제어가 막혀 있을 때의 사유.
#[derive(Debug, Clone, PartialEq)]
pub enum SetupReason {
    NoDdcutil,   // ddcutil 미설치
    Permission,  // /dev/i2c-* 접근 권한 없음 (i2c 그룹/udev 룰/모듈 미설정)
}

#[derive(Debug, Clone)]
pub enum DisplayMsg {
    Refresh,
    Refreshed(DisplayScan),
    /// 드래그 중 로컬 갱신 (idx, 값). 즉시 적용하지 않는다.
    SetBrightness(usize, u32),
    /// 슬라이더를 놓을 때 실제 적용 (idx).
    CommitBrightness(usize),
    SetContrast(usize, u32),
    CommitContrast(usize),
    SetupPermissions,
    /// i2c-dev 모듈 재로드 + udev 재트리거 후 재스캔 (재부팅 후 모듈 미로드 대비).
    Reprobe,
    /// 모든 모니터 밝기를 목표값(%)으로 한 번에 전환 (저전력/기본 모드 버튼).
    ApplyBrightnessPreset(u32),
    Applied(CmdResult),
    /// 작업 후 곧바로 재스캔이 필요한 경우(권한 설정·재인식).
    AppliedRescan(CmdResult),
    MapTouchToOutput { touch_idx: usize, monitor_idx: usize },
}

pub struct DisplayState {
    pub monitors: Vec<Monitor>,
    pub touches: Vec<TouchDevice>,
    pub setup: Option<SetupReason>,
    pub scanned: bool,
    pub running: Option<String>,
    pub session_type: String,
    pub xinput_available: bool,
}

impl DisplayState {
    pub fn new() -> Self {
        Self {
            monitors: Vec::new(),
            touches: Vec::new(),
            setup: None,
            scanned: false,
            running: None,
            session_type: String::new(),
            xinput_available: false,
        }
    }

    pub fn update(&mut self, msg: DisplayMsg) -> (Task<DisplayMsg>, Option<CmdResult>) {
        match msg {
            DisplayMsg::Refresh => {
                (Task::perform(async { scan().await }, DisplayMsg::Refreshed), None)
            }
            DisplayMsg::Refreshed(scan) => {
                // 드래그 중(running)이면 사용자 조작값을 덮어쓰지 않도록 보호
                if self.running.is_none() {
                    self.monitors = scan.monitors;
                    self.touches = scan.touches;
                    self.setup = scan.setup;
                    self.session_type = scan.session_type;
                    self.xinput_available = scan.xinput_available;
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
            DisplayMsg::ApplyBrightnessPreset(target) => {
                for m in self.monitors.iter_mut() { m.pct = target; }
                self.running = Some(format!("밝기 프리셋 적용 중... ({target}%)"));
                (apply(preset_script(&self.monitors)), None)
            }
            DisplayMsg::Applied(r) => {
                self.running = None;
                (Task::none(), Some(r))
            }
            DisplayMsg::AppliedRescan(r) => {
                self.running = None;
                let t = Task::perform(async { scan().await }, DisplayMsg::Refreshed);
                (t, Some(r))
            }
            DisplayMsg::MapTouchToOutput { touch_idx, monitor_idx } => {
                let Some(touch) = self.touches.get(touch_idx) else { return (Task::none(), None) };
                let Some(mon) = self.monitors.get(monitor_idx) else { return (Task::none(), None) };
                self.running = Some(format!("{} 터치를 {}에 매핑 중...", touch.name, mon.connector));
                (apply(map_touch_script(touch, mon, touch_matrix(mon, &self.monitors))), None)
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

        let idle_presets = self.running.is_none();
        col = col.push(
            card(
                column![
                    text("전원 모드").size(14),
                    Space::with_height(4),
                    text(format!("버튼 하나로 모든 모니터 밝기를 전환합니다 (저전력 {LOW_POWER_PCT}% / 기본 {DEFAULT_PCT}%).")).size(11).color(C_DIM),
                    Space::with_height(10),
                    row![
                        action_btn("저전력 모드", DisplayMsg::ApplyBrightnessPreset(LOW_POWER_PCT), idle_presets, C_BTN2),
                        Space::with_width(8),
                        action_btn("기본 모드", DisplayMsg::ApplyBrightnessPreset(DEFAULT_PCT), idle_presets, C_BLUE),
                    ],
                ]
            )
        );
        col = col.push(Space::with_height(10));

        for (i, mon) in self.monitors.iter().enumerate() {
            col = col.push(monitor_card(i, mon, self.running.is_some()));
            col = col.push(Space::with_height(10));
        }

        if let Some(reason) = &self.setup {
            col = col.push(setup_card(reason, self.running.is_some()));
            col = col.push(Space::with_height(10));
        }

        col = col.push(touch_map_card(
            &self.touches,
            &self.monitors,
            &self.session_type,
            self.xinput_available,
            self.running.is_some(),
        ));
        col = col.push(Space::with_height(10));

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
    if mon.controls_brightness {
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
    } else {
        body = body.push(text("밝기 제어는 사용할 수 없지만 터치 매핑 대상으로 사용할 수 있습니다.").size(11).color(C_DIM));
    }

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

fn touch_map_card<'a>(
    touches: &'a [TouchDevice],
    monitors: &'a [Monitor],
    session_type: &'a str,
    xinput_available: bool,
    busy: bool,
) -> Element<'a, DisplayMsg> {
    let external_monitors: Vec<(usize, &Monitor)> = monitors.iter().enumerate()
        .filter(|(_, m)| matches!(m.kind, Kind::External { .. }))
        .collect();
    let external_touches: Vec<(usize, &TouchDevice)> = touches.iter().enumerate()
        .filter(|(_, t)| t.external)
        .collect();

    let mut body = column![
        text("외부 터치스크린 매핑").size(14),
        Space::with_height(8),
    ];

    if external_touches.is_empty() {
        body = body.push(text("외부 USB 터치 장치가 보이지 않습니다. 터치 USB 케이블 연결을 확인하세요.").size(11).color(C_DIM));
    } else if external_monitors.is_empty() {
        body = body.push(text("외부 모니터 출력이 보이지 않습니다. 모니터 연결 또는 디스플레이 인식을 확인하세요.").size(11).color(C_DIM));
    } else {
        let session_label = if session_type.is_empty() { "unknown" } else { session_type };
        body = body.push(
            text(format!(
                "세션: {session_label}. X11에서는 즉시 적용됩니다. COSMIC Wayland에서는 현재 컴포지터가 터치-출력 매핑 CLI를 제공하지 않아, Xwayland 기본 출력만 확실히 맞추고 나머지는 보정 행렬(calibration matrix) 기반 추정 적용이라 모니터별로 안 될 수 있습니다."
            ))
            .size(11)
            .color(C_DIM)
        );
        if !xinput_available {
            body = body.push(Space::with_height(4));
            body = body.push(text("xinput 패키지가 없으면 X11 직접 매핑은 사용할 수 없습니다.").size(10).color(C_WARN));
        }
        body = body.push(Space::with_height(10));

        for (touch_idx, touch) in external_touches {
            let event = if touch.event.is_empty() { "-".to_string() } else { touch.event.clone() };
            body = body.push(
                row![
                    column![
                        text(&touch.name).size(12).color(C_TEXT),
                        text(format!("{event} · {}", touch.phys)).size(10).color(C_DIM),
                    ].width(Length::Fill),
                ]
                .align_y(iced::Alignment::Center)
            );
            body = body.push(Space::with_height(6));

            let mut buttons = row![Space::with_width(Length::Fill)];
            for (monitor_idx, mon) in &external_monitors {
                buttons = buttons.push(action_btn(
                    &format!("{}에 매핑", mon.connector),
                    DisplayMsg::MapTouchToOutput { touch_idx, monitor_idx: *monitor_idx },
                    !busy,
                    C_BLUE,
                ));
                buttons = buttons.push(Space::with_width(8));
            }
            body = body.push(buttons);
            body = body.push(Space::with_height(10));
        }
    }

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

/// 모든 모니터에 현재 pct 값을 한 번에 적용하는 스크립트 (저전력/기본 모드 버튼).
fn preset_script(monitors: &[Monitor]) -> String {
    monitors.iter().map(brightness_script).collect::<Vec<_>>().join(" ; ")
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

fn map_touch_script(touch: &TouchDevice, mon: &Monitor, matrix: Option<[f64; 6]>) -> String {
    let dev = sh_quote(&touch.name);
    let out = sh_quote(&mon.connector);
    let event_name = touch.event.clone();
    let udev_name = udev_escape(&touch.name);
    let matrix_text = matrix.map(|m| {
        format!(
            "{:.6} {:.6} {:.6} {:.6} {:.6} {:.6}",
            m[0], m[1], m[2], m[3], m[4], m[5],
        )
    });
    let wayland_apply = if let Some(matrix_text) = matrix_text {
        // libinput의 LIBINPUT_CALIBRATION_MATRIX만 실제로 존재하는 속성이다.
        // (참고: 터치-출력 바인딩을 위한 표준 udev/libinput 속성은 없다 — 이 행렬은
        // X11의 `xinput --map-to-output`과 같은 원리로, 전체 가상 화면 중 이 모니터가
        // 차지하는 영역으로 터치 좌표 범위를 좁힐 뿐이다. 실제로 해당 모니터에만
        // 국한되는지는 컴포지터가 터치를 가상 화면 전체 좌표로 다루는지에 달려 있다.)
        let rule = format!(
            "ACTION==\"add|change\", SUBSYSTEM==\"input\", KERNEL==\"event*\", ATTRS{{name}}==\"{udev_name}\", ENV{{ID_INPUT_TOUCHSCREEN}}==\"1\", ENV{{LIBINPUT_CALIBRATION_MATRIX}}=\"{}\"",
            udev_escape(&matrix_text),
        );
        let inner = format!(
            "printf '%s\\n' {} {} > /etc/udev/rules.d/99-popmgr-touchscreen-map.rules; udevadm control --reload-rules; udevadm trigger --subsystem-match=input --action=change; devpath=$(readlink -f /sys/class/input/{event}/device); usbdev=$devpath; while [ \"$usbdev\" != \"/\" ] && [ ! -e \"$usbdev/authorized\" ]; do usbdev=$(dirname \"$usbdev\"); done; if [ -w \"$usbdev/authorized\" ]; then printf '0' > \"$usbdev/authorized\"; sleep 1; printf '1' > \"$usbdev/authorized\"; fi",
            sh_quote("# popmgr: map external touchscreen to selected output"),
            sh_quote(&rule),
            event = event_name,
        );
        format!(
            "if [ \"${{XDG_SESSION_TYPE:-}}\" = wayland ]; then \
                 if command -v pkexec >/dev/null 2>&1; then \
                     pkexec bash -c {}; \
                     echo \"Wayland 보정 행렬 규칙 적용: $dev -> $out ({}). USB 장치를 재시작해 다시 읽게 했습니다 — COSMIC이 터치를 가상 화면 전체 좌표로 처리하는 경우에만 $out 영역으로 좁혀집니다. 안 되면 여전히 컴포지터의 공식 지원이 없다는 뜻입니다.\"; \
                     exit 0; \
                 fi; \
             fi; ",
            sh_quote(&inner),
            matrix_text,
        )
    } else {
        String::new()
    };
    format!(
        "set -u; \
         dev={dev}; out={out}; mapped=0; \
         if command -v xinput >/dev/null 2>&1 && command -v xrandr >/dev/null 2>&1 && xrandr --query >/dev/null 2>&1; then \
             xinput map-to-output \"$dev\" \"$out\" && mapped=1 && echo \"XInput 터치 매핑 적용: $dev -> $out\"; \
         fi; \
         if command -v cosmic-randr >/dev/null 2>&1; then \
             cosmic-randr xwayland --primary \"$out\" >/dev/null 2>&1 || true; \
         fi; \
         if [ \"$mapped\" = 1 ]; then \
             exit 0; \
         fi; \
         {wayland_apply} \
         if [ \"${{XDG_SESSION_TYPE:-}}\" = wayland ]; then \
             echo \"현재 COSMIC Wayland 세션입니다. 출력 geometry를 읽지 못해 터치 보정 규칙을 만들 수 없습니다. 새로고침 후 다시 시도하세요. 감지된 장치: $dev, 대상 출력: $out\"; \
         else \
             echo \"xinput/xrandr로 터치 매핑을 적용하지 못했습니다. xinput 패키지와 DISPLAY 접근 상태를 확인하세요. 장치: $dev, 출력: $out\"; \
         fi; \
         exit 1"
    )
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

fn udev_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ─── 스캔 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DisplayScan {
    monitors: Vec<Monitor>,
    touches: Vec<TouchDevice>,
    setup: Option<SetupReason>,
    session_type: String,
    xinput_available: bool,
}

async fn scan() -> DisplayScan {
    let mut monitors = scan_internal().await;
    let touches = scan_touch_devices();
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let xinput_available = command_available("xinput").await;
    let output_geometries = scan_output_geometries().await;
    let mut setup = None;

    // ddcutil 설치 여부
    let has = runner::run_sh("command -v ddcutil >/dev/null 2>&1 && echo yes").await;
    if !has.output.contains("yes") {
        // 외부 모니터가 물리적으로 붙어 있을 때만 설치 안내
        if external_connected().await {
            setup = Some(SetupReason::NoDdcutil);
        }
    } else {
        // ko_KR 등 비영어 로케일에서 ddcutil 출력 라벨이 번역되면 parse_displays가
        // 전부 깨져 모니터를 0개로 읽는다(audio.rs와 동일 부류). 영어 출력 강제.
        let det = runner::run_sh("LC_ALL=C ddcutil detect 2>&1").await;
        let displays = parse_displays(&det.output);

        if displays.is_empty() {
            // 외부 모니터는 붙어 있는데 ddcutil 이 못 잡으면 권한 문제로 본다.
            if external_connected().await {
                setup = Some(SetupReason::Permission);
            }
        } else {
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
                    controls_brightness: true,
                    geometry: None,
                });
            }
        }
    }

    add_drm_external_fallbacks(&mut monitors).await;
    apply_output_geometries(&mut monitors, &output_geometries);

    DisplayScan { monitors, touches, setup, session_type, xinput_available }
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
            controls_brightness: true,
            geometry: None,
        });
    }
    out
}

async fn command_available(cmd: &str) -> bool {
    let safe = cmd.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !safe { return false; }
    runner::run_sh(&format!("command -v {cmd} >/dev/null 2>&1 && echo yes")).await.output.contains("yes")
}

async fn add_drm_external_fallbacks(monitors: &mut Vec<Monitor>) {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else { return };
    for e in entries.flatten() {
        let fname = e.file_name().to_string_lossy().to_string();
        if !fname.contains('-') || fname.contains("eDP") { continue; }
        let connected = std::fs::read_to_string(e.path().join("status"))
            .map(|s| s.trim() == "connected").unwrap_or(false);
        if !connected { continue; }
        let connector = drm_connector_name(&fname);
        if monitors.iter().any(|m| m.connector == connector) { continue; }
        monitors.push(Monitor {
            kind: Kind::External { display: 0 },
            name: format!("외부 모니터 {connector}"),
            connector,
            pct: 50,
            raw_max: 100,
            contrast: None,
            contrast_max: 0,
            controls_brightness: false,
            geometry: None,
        });
    }
}

async fn scan_output_geometries() -> Vec<(String, OutputGeometry)> {
    let out = runner::run_sh("cosmic-randr list 2>/dev/null").await;
    if out.success { parse_cosmic_output_geometries(&out.output) } else { Vec::new() }
}

fn apply_output_geometries(monitors: &mut [Monitor], geometries: &[(String, OutputGeometry)]) {
    for mon in monitors {
        mon.geometry = geometries.iter()
            .find(|(name, _)| name == &mon.connector)
            .map(|(_, geom)| *geom);
    }
}

fn touch_matrix(target: &Monitor, monitors: &[Monitor]) -> Option<[f64; 6]> {
    let target = target.geometry?;
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    for geom in monitors.iter().filter_map(|m| m.geometry) {
        min_x = min_x.min(geom.x);
        min_y = min_y.min(geom.y);
        max_x = max_x.max(geom.x + geom.w as i32);
        max_y = max_y.max(geom.y + geom.h as i32);
    }
    if min_x >= max_x || min_y >= max_y {
        return None;
    }
    let total_w = (max_x - min_x) as f64;
    let total_h = (max_y - min_y) as f64;
    Some([
        target.w as f64 / total_w,
        0.0,
        (target.x - min_x) as f64 / total_w,
        0.0,
        target.h as f64 / total_h,
        (target.y - min_y) as f64 / total_h,
    ])
}

fn parse_cosmic_output_geometries(raw: &str) -> Vec<(String, OutputGeometry)> {
    #[derive(Default)]
    struct Cur {
        name: String,
        enabled: bool,
        x: Option<i32>,
        y: Option<i32>,
        scale: Option<f64>,
        mode_w: Option<u32>,
        mode_h: Option<u32>,
    }

    fn flush(cur: &mut Cur, out: &mut Vec<(String, OutputGeometry)>) {
        if cur.enabled {
            if let (Some(x), Some(y), Some(scale), Some(mode_w), Some(mode_h)) =
                (cur.x, cur.y, cur.scale, cur.mode_w, cur.mode_h)
            {
                if scale > 0.0 {
                    out.push((
                        cur.name.clone(),
                        OutputGeometry {
                            x,
                            y,
                            w: ((mode_w as f64) / scale).round().max(1.0) as u32,
                            h: ((mode_h as f64) / scale).round().max(1.0) as u32,
                        },
                    ));
                }
            }
        }
        *cur = Cur::default();
    }

    let mut res = Vec::new();
    let mut cur = Cur::default();
    let clean = strip_ansi(raw);
    for line in clean.lines() {
        let t = line.trim();
        if t.is_empty() { continue; }
        if !line.starts_with(char::is_whitespace) {
            flush(&mut cur, &mut res);
            cur.name = t.split_whitespace().next().unwrap_or_default().to_string();
            cur.enabled = t.contains("(enabled)");
        } else if let Some(pos) = t.strip_prefix("Position:") {
            let mut parts = pos.trim().split(',');
            cur.x = parts.next().and_then(|v| v.trim().parse().ok());
            cur.y = parts.next().and_then(|v| v.trim().parse().ok());
        } else if let Some(scale) = t.strip_prefix("Scale:") {
            cur.scale = scale.trim().trim_end_matches('%').parse::<f64>().ok().map(|v| v / 100.0);
        } else if t.contains("(current)") {
            for part in t.split_whitespace() {
                if let Some((w, h)) = part.split_once('x') {
                    if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                        cur.mode_w = Some(w);
                        cur.mode_h = Some(h);
                        break;
                    }
                }
            }
        }
    }
    flush(&mut cur, &mut res);
    res
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn scan_touch_devices() -> Vec<TouchDevice> {
    let Ok(raw) = std::fs::read_to_string("/proc/bus/input/devices") else { return Vec::new() };
    raw.split("\n\n").filter_map(parse_touch_block).collect()
}

fn parse_touch_block(block: &str) -> Option<TouchDevice> {
    let name = proc_value(block, "N: Name=")?.trim_matches('"').to_string();
    let lname = name.to_lowercase();
    if lname.contains("touchpad") || lname.contains("mouse") {
        return None;
    }
    if !(lname.contains("touch") || lname.contains("finger") || lname.contains("ilitek")) {
        return None;
    }
    if !block.contains("B: ABS=") {
        return None;
    }

    let handlers = proc_value(block, "H: Handlers=").unwrap_or_default();
    let event = handlers.split_whitespace()
        .find(|h| h.starts_with("event"))
        .unwrap_or_default()
        .to_string();
    let phys = proc_value(block, "P: Phys=").unwrap_or_default();
    let sysfs = proc_value(block, "S: Sysfs=").unwrap_or_default();
    let external = phys.starts_with("usb-") || sysfs.contains("/usb");

    Some(TouchDevice { name, event, phys, external })
}

fn proc_value(block: &str, prefix: &str) -> Option<String> {
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(rest.trim().to_string());
        }
    }
    None
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
                return drm_connector_name(&fname);
            }
        }
    }
    "eDP".into()
}

fn drm_connector_name(name: &str) -> String {
    name.rsplit('-').take(2).collect::<Vec<_>>()
        .into_iter().rev().collect::<Vec<_>>().join("-")
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
