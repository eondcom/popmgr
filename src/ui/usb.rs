use iced::{
    widget::{button, column, container, row, scrollable, slider, text, Space},
    Color, Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_BORDER, C_BTN2, C_DIM, C_ERR, C_OK, C_SURFACE, C_TEXT, C_WARN};

/// popmgr 전용 root 헬퍼. NOPASSWD sudoers 로만 호출되며 검증된 동작만 수행한다.
/// /usr/local/bin/popmgr-helper 에 root 소유로 설치된다(사용자는 수정 불가).
const HELPER_SCRIPT: &str = r#"#!/bin/bash
# popmgr 전용 root 헬퍼 (NOPASSWD). 인자를 엄격히 검증하고 정해진 작업만 수행.
set -euo pipefail
MAPPER=/opt/ktrackball/trackball_mapper.py
CONF=/etc/ktrackball/config.toml
ENVF=/etc/environment

case "${1:-}" in
  set-trackball-speed)
    f="${2:-}"
    [[ "$f" =~ ^[0-9]+(\.[0-9]+)?$ ]] || { echo "잘못된 배율"; exit 2; }
    awk "BEGIN{exit !($f>=1.0 && $f<=3.0)}" || { echo "배율 범위(1.0~3.0) 벗어남"; exit 2; }
    [ -f "$MAPPER" ] || { echo "데몬 없음: $MAPPER"; exit 1; }
    [ -f "$CONF" ] || { echo "설정 없음: $CONF"; exit 1; }
    [ -f "${MAPPER}.bak" ] || cp -a "$MAPPER" "${MAPPER}.bak"
    if ! grep -q speed_factor "$MAPPER"; then
      python3 - "$MAPPER" <<'PY'
import sys, io
p = sys.argv[1]
s = io.open(p, encoding="utf-8").read()
a = 'self.precision_factor = float(data.get("precision_factor", 0.35))'
s = s.replace(a, a + '\n        self.speed_factor = float(data.get("speed_factor", 1.0))', 1)
a = 'self._prec_acc_y = 0.0'
s = s.replace(a, a + '\n        self._spd_acc_x = 0.0\n        self._spd_acc_y = 0.0', 1)
old = "        else:\n            for code, val in self._pending_rel:\n                ui.write(e.EV_REL, code, val)\n            ui.syn()\n"
new = "        else:\n            f = self.cfg.speed_factor\n            if f == 1.0:\n                for code, val in self._pending_rel:\n                    ui.write(e.EV_REL, code, val)\n                ui.syn()\n            else:\n                emitted = False\n                for code, val in self._pending_rel:\n                    if code == e.REL_X:\n                        self._spd_acc_x += val * f\n                        out = int(self._spd_acc_x)\n                        self._spd_acc_x -= out\n                    elif code == e.REL_Y:\n                        self._spd_acc_y += val * f\n                        out = int(self._spd_acc_y)\n                        self._spd_acc_y -= out\n                    else:\n                        out = val\n                    if out:\n                        ui.write(e.EV_REL, code, out)\n                        emitted = True\n                if emitted:\n                    ui.syn()\n"
assert old in s, "flush block not found"
s = s.replace(old, new, 1)
io.open(p, "w", encoding="utf-8").write(s)
import py_compile
py_compile.compile(p, doraise=True)
PY
    fi
    if grep -qE '^[[:space:]]*speed_factor' "$CONF"; then
      sed -i -E "s|^[[:space:]]*speed_factor[[:space:]]*=.*|speed_factor = $f|" "$CONF"
    else
      sed -i "0,/^\[/{/^\[/i speed_factor = $f
}" "$CONF"
    fi
    systemctl restart ktrackball.service
    echo "트랙볼 배율 $f 적용"
    ;;
  set-cursor-size)
    n="${2:-}"
    { [[ "$n" =~ ^[0-9]+$ ]] && [ "$n" -ge 16 ] && [ "$n" -le 128 ]; } || { echo "잘못된 크기(16~128)"; exit 2; }
    if grep -qE '^[[:space:]]*XCURSOR_SIZE=' "$ENVF"; then
      sed -i -E "s|^[[:space:]]*XCURSOR_SIZE=.*|XCURSOR_SIZE=$n|" "$ENVF"
    else
      echo "XCURSOR_SIZE=$n" >> "$ENVF"
    fi
    echo "커서 크기 $n 적용(재로그인 필요)"
    ;;
  restart-ktrackball)
    systemctl restart ktrackball.service
    echo "ktrackball 재시작"
    ;;
  check)
    echo ok
    ;;
  *)
    echo "알 수 없는 명령: ${1:-}"; exit 1
    ;;
esac
"#;

#[derive(Debug, Clone)]
pub struct UsbDevice {
    pub vid: String,
    pub pid: String,
    pub manufacturer: String,
    pub product: String,
    pub speed: String,
    pub bus: String,
    pub dev: String,
    pub sysfs_name: String,  // 토폴로지 식별자 (예: "1-5.1", "4-1.1")
    pub icon: &'static str,
    pub highlight: bool,
}

#[derive(Debug, Clone)]
pub struct UsbFailedPort {
    pub port: String,
}

#[derive(Debug, Clone)]
pub struct UsbStatus {
    pub devices: Vec<UsbDevice>,
    pub failed_ports: Vec<UsbFailedPort>,
    pub ktrackball_pid: Option<u32>,
    pub pointer_speed: Option<f64>,    // COSMIC 포인터 가속 속도 (-1.0 ~ 1.0)
    pub tb_speed_factor: f64,          // ktrackball 모션 배율 (1.0 ~ 3.0)
    pub cursor_size: i32,              // XCURSOR_SIZE / gsettings cursor-size
    pub helper_installed: bool,        // popmgr-helper(NOPASSWD) 설치 여부
}

#[derive(Debug, Clone)]
pub enum UsbMsg {
    Refresh,
    Refreshed(UsbStatus),
    RetrieveDev(String),           // sysfs_name (예: "1-5.1")
    RetrieveAll,
    XhciReset,
    RestartKtrackball,
    SetPointerSpeed(i32),    // 슬라이더 드래그 중 (-100 ~ 100)
    CommitPointerSpeed,      // 슬라이더 놓을 때 파일 기록
    SetTbSpeed(i32),         // 트랙볼 배율 드래그 중 (factor*100, 100~300)
    CommitTbSpeed,
    SetCursorSize(i32),      // 커서 크기 드래그 중 (16~96)
    CommitCursorSize,
    InstallHelper,           // popmgr-helper + NOPASSWD sudoers 설치(1회, pkexec)
    Done(CmdResult),
    ConfirmXhci,
    CancelXhci,
}

pub struct UsbState {
    pub status: Option<UsbStatus>,
    pub running: Option<String>,
    pub confirm_xhci: bool,
    pub slider_pct: Option<i32>,   // 포인터 속도 드래그 중 임시 값; None이면 status에서 읽음
    pub tb_slider: Option<i32>,    // 트랙볼 배율 드래그 중
    pub cursor_slider: Option<i32>,// 커서 크기 드래그 중
}

impl UsbState {
    pub fn new() -> Self {
        Self { status: None, running: None, confirm_xhci: false,
               slider_pct: None, tb_slider: None, cursor_slider: None }
    }

    pub fn update(&mut self, msg: UsbMsg) -> (Task<UsbMsg>, Option<CmdResult>) {
        match msg {
            UsbMsg::Refresh => {
                let t = Task::perform(async { scan_usb().await }, UsbMsg::Refreshed);
                (t, None)
            }
            UsbMsg::Refreshed(s) => {
                self.status = Some(s);
                self.slider_pct = None; self.tb_slider = None; self.cursor_slider = None;
                (Task::none(), None)
            }
            UsbMsg::SetPointerSpeed(v) => { self.slider_pct = Some(v); (Task::none(), None) }
            UsbMsg::CommitPointerSpeed => {
                // 슬라이더 현재 값을 -1.0~1.0 으로 변환해 input_default 에 기록.
                // ~/.config 안이라 pkexec 불필요, cosmic-comp 가 파일 watch → 즉시 반영.
                let pct = self.slider_pct
                    .or_else(|| self.status.as_ref().and_then(|s| s.pointer_speed).map(|v| (v * 100.0).round() as i32))
                    .unwrap_or(0);
                let speed = (pct as f64 / 100.0).clamp(-1.0, 1.0);
                let t = Task::perform(async move {
                    match write_pointer_speed(speed) {
                        Ok(()) => CmdResult { success: true, output: format!("포인터 속도 적용: {speed:.2}") },
                        Err(e) => CmdResult { success: false, output: format!("포인터 속도 적용 실패: {e}") },
                    }
                }, UsbMsg::Done);
                (t, None)
            }
            UsbMsg::SetTbSpeed(v) => { self.tb_slider = Some(v); (Task::none(), None) }
            UsbMsg::CommitTbSpeed => {
                let v = self.tb_slider
                    .or_else(|| self.status.as_ref().map(|s| (s.tb_speed_factor * 100.0).round() as i32))
                    .unwrap_or(100);
                let factor = (v as f64 / 100.0).clamp(1.0, 3.0);
                self.running = Some(format!("트랙볼 배율 {factor:.2}x 적용 중..."));
                let script = format!("sudo -n /usr/local/bin/popmgr-helper set-trackball-speed {factor:.2} 2>&1");
                let t = Task::perform(async move { runner::run_sh(&script).await }, UsbMsg::Done);
                (t, None)
            }
            UsbMsg::SetCursorSize(v) => { self.cursor_slider = Some(v); (Task::none(), None) }
            UsbMsg::CommitCursorSize => {
                let n = self.cursor_slider
                    .or_else(|| self.status.as_ref().map(|s| s.cursor_size))
                    .unwrap_or(24)
                    .clamp(16, 96);
                self.running = Some(format!("커서 크기 {n} 적용 중..."));
                // /etc/environment 는 헬퍼(root)로, gsettings 는 사용자 권한으로 즉시 반영.
                let script = format!(
                    "gsettings set org.gnome.desktop.interface cursor-size {n} 2>/dev/null; \
                     sudo -n /usr/local/bin/popmgr-helper set-cursor-size {n} 2>&1"
                );
                let t = Task::perform(async move { runner::run_sh(&script).await }, UsbMsg::Done);
                (t, None)
            }
            UsbMsg::InstallHelper => {
                self.running = Some("권한 헬퍼 설치 중 (암호 입력)...".into());
                let user = std::env::var("USER").unwrap_or_else(|_| "dell".into());
                let helper_b64 = base64_encode(HELPER_SCRIPT.as_bytes());
                // 헬퍼를 root 소유로 설치하고, 그 한 파일만 NOPASSWD 로 허용하는 sudoers 작성.
                let script = format!(
                    "pkexec bash -c 'set -e; \
                     echo {helper_b64} | base64 -d > /usr/local/bin/popmgr-helper; \
                     chown root:root /usr/local/bin/popmgr-helper; \
                     chmod 0755 /usr/local/bin/popmgr-helper; \
                     printf \"%s ALL=(root) NOPASSWD: /usr/local/bin/popmgr-helper\\n\" {user} > /etc/sudoers.d/popmgr; \
                     chmod 0440 /etc/sudoers.d/popmgr; \
                     visudo -cf /etc/sudoers.d/popmgr'"
                );
                let t = Task::perform(async move { runner::run_sh(&script).await }, UsbMsg::Done);
                (t, None)
            }
            UsbMsg::RetrieveDev(sysfs_name) => {
                self.running = Some(format!("{} 재인식 중...", sysfs_name));
                let path = format!("/sys/bus/usb/devices/{sysfs_name}");
                // authorized 토글로 실제 disconnect/reconnect 발생
                let script = format!(
                    "pkexec bash -c 'set -e; \
                     test -f {path}/authorized || {{ echo \"경로 없음: {path}\" >&2; exit 1; }}; \
                     echo 0 > {path}/authorized; sleep 0.3; echo 1 > {path}/authorized'"
                );
                let t = Task::perform(async move { runner::run_sh(&script).await }, UsbMsg::Done);
                (t, None)
            }
            UsbMsg::RetrieveAll => {
                self.running = Some("USB 전체 재인식 중 (authorized 토글)...".into());
                // udevadm trigger는 udev 규칙만 재실행, 장치 reset이 일어나지 않음.
                // 각 장치의 authorized를 0/1 토글해서 실제 disconnect/reconnect 유도.
                let script = "pkexec bash -c '\
                    set -e; \
                    paths=$(ls -d /sys/bus/usb/devices/*/authorized 2>/dev/null | grep -v /usb[1-9]/authorized); \
                    for p in $paths; do echo 0 > \"$p\" 2>/dev/null || true; done; \
                    sleep 0.5; \
                    for p in $paths; do echo 1 > \"$p\" 2>/dev/null || true; done; \
                    udevadm trigger --subsystem-match=usb; \
                    udevadm settle\
                '";
                let t = Task::perform(
                    async move { runner::run_sh(script).await },
                    UsbMsg::Done,
                );
                (t, None)
            }
            UsbMsg::XhciReset => { self.confirm_xhci = true; (Task::none(), None) }
            UsbMsg::ConfirmXhci => {
                self.confirm_xhci = false;
                self.running = Some("xHCI 리셋 중...".into());
                let script = "pkexec bash -c '\
                    echo 0000:00:14.0 > /sys/bus/pci/drivers/xhci_hcd/unbind && \
                    sleep 2 && \
                    echo 0000:00:14.0 > /sys/bus/pci/drivers/xhci_hcd/bind\
                '";
                let t = Task::perform(async move { runner::run_sh(script).await }, UsbMsg::Done);
                (t, None)
            }
            UsbMsg::CancelXhci => { self.confirm_xhci = false; (Task::none(), None) }
            UsbMsg::RestartKtrackball => {
                self.running = Some("ktrackball 재시작 중...".into());
                let t = Task::perform(
                    async { runner::run_sh("pkexec systemctl restart ktrackball.service").await },
                    UsbMsg::Done,
                );
                (t, None)
            }
            UsbMsg::Done(r) => {
                self.running = None;
                let refresh = Task::perform(async { scan_usb().await }, UsbMsg::Refreshed);
                (refresh, Some(r))
            }
        }
    }

    pub fn view(&self) -> Element<'_, UsbMsg> {
        let is_running = self.running.is_some();
        let mut col = column![
            text("USB 장치").size(20),
            Space::with_height(16),
        ];

        if let Some(label) = &self.running {
            col = col.push(running_bar(label)).push(Space::with_height(10));
        }

        // xHCI 확인 다이얼로그
        if self.confirm_xhci {
            col = col.push(xhci_confirm_card());
            return scrollable(container(col).padding([4, 0])).into();
        }

        // 장치 목록
        if let Some(st) = &self.status {
            // 열거 실패 포트
            for fp in &st.failed_ports {
                col = col.push(failed_port_row(fp)).push(Space::with_height(6));
            }

            // USB 장치 목록
            let device_list = st.devices.iter().map(|d| {
                device_row(d, is_running)
            });
            let list_col = device_list.fold(column![].spacing(4), |c, r| c.push(r));

            col = col.push(
                scrollable(list_col).height(280)
            );
            col = col.push(Space::with_height(12));

            // ktrackball 상태
            let (ktb_txt, ktb_col) = match st.ktrackball_pid {
                Some(pid) => (format!("● ktrackball 실행 중 (PID {pid})"), C_OK),
                None      => ("○ ktrackball 중지됨".into(), C_ERR),
            };
            col = col.push(card(
                column![
                    text("ktrackball 데몬").size(13).color(C_TEXT),
                    Space::with_height(6),
                    row![
                        text(ktb_txt).size(12).color(ktb_col),
                        Space::with_width(Length::Fill),
                        action_btn("재시작", UsbMsg::RestartKtrackball, !is_running, C_WARN),
                    ].align_y(iced::Alignment::Center),
                ]
            ));

            // 포인터 속도 (COSMIC input_default · 트랙볼/마우스 공통)
            col = col.push(Space::with_height(10));
            let cur_pct = self.slider_pct
                .or_else(|| st.pointer_speed.map(|v| (v * 100.0).round() as i32))
                .unwrap_or(0);
            col = col.push(card(
                column![
                    row![
                        text("포인터 속도").size(13).color(C_TEXT),
                        Space::with_width(Length::Fill),
                        text(format!("{:.2}", cur_pct as f64 / 100.0)).size(12).color(C_TEXT),
                    ].align_y(iced::Alignment::Center),
                    Space::with_height(8),
                    row![
                        text("느림").size(11).color(C_DIM),
                        Space::with_width(8),
                        slider(-100..=100, cur_pct, UsbMsg::SetPointerSpeed)
                            .on_release(UsbMsg::CommitPointerSpeed)
                            .width(Length::Fill),
                        Space::with_width(8),
                        text("빠름").size(11).color(C_DIM),
                    ].align_y(iced::Alignment::Center),
                    Space::with_height(6),
                    text("COSMIC 포인터 가속 설정(트랙볼·마우스 공통). 놓는 즉시 적용됩니다.")
                        .size(11).color(C_DIM),
                ]
            ));

            // 트랙볼 배율 + 커서 크기 — root 헬퍼 필요
            col = col.push(Space::with_height(10));
            if st.helper_installed {
                // 트랙볼 가속 배율 (libinput 상한을 넘는 진짜 속도 증가)
                let tb_v = self.tb_slider.unwrap_or((st.tb_speed_factor * 100.0).round() as i32);
                col = col.push(card(
                    column![
                        row![
                            text("트랙볼 가속 배율").size(13).color(C_TEXT),
                            Space::with_width(Length::Fill),
                            text(format!("{:.2}x", tb_v as f64 / 100.0)).size(12).color(C_TEXT),
                        ].align_y(iced::Alignment::Center),
                        Space::with_height(8),
                        row![
                            text("1.0x").size(11).color(C_DIM),
                            Space::with_width(8),
                            slider(100..=300, tb_v, UsbMsg::SetTbSpeed)
                                .on_release(UsbMsg::CommitTbSpeed)
                                .width(Length::Fill),
                            Space::with_width(8),
                            text("3.0x").size(11).color(C_DIM),
                        ].align_y(iced::Alignment::Center),
                        Space::with_height(6),
                        text("ktrackball 데몬에서 모션에 배율 적용(libinput 최대보다 빠름). 놓으면 데몬 재시작·즉시 반영.")
                            .size(11).color(C_DIM),
                    ]
                ));
                col = col.push(Space::with_height(10));

                // 커서 크기
                let cs_v = self.cursor_slider.unwrap_or(st.cursor_size).clamp(16, 96);
                col = col.push(card(
                    column![
                        row![
                            text("커서 크기").size(13).color(C_TEXT),
                            Space::with_width(Length::Fill),
                            text(format!("{cs_v}px")).size(12).color(C_TEXT),
                        ].align_y(iced::Alignment::Center),
                        Space::with_height(8),
                        row![
                            text("작게").size(11).color(C_DIM),
                            Space::with_width(8),
                            slider(16..=96, cs_v, UsbMsg::SetCursorSize)
                                .on_release(UsbMsg::CommitCursorSize)
                                .width(Length::Fill),
                            Space::with_width(8),
                            text("크게").size(11).color(C_DIM),
                        ].align_y(iced::Alignment::Center),
                        Space::with_height(6),
                        text("XCURSOR_SIZE + gsettings 기록. GTK 앱은 즉시, COSMIC 컴포지터 커서는 재로그인 후 반영.")
                            .size(11).color(C_DIM),
                    ]
                ));
            } else {
                // 헬퍼 미설치 → 1회 설치 안내
                col = col.push(card(
                    column![
                        text("트랙볼 배율 · 커서 크기").size(13).color(C_TEXT),
                        Space::with_height(6),
                        text("이 두 설정은 root 권한이 필요합니다. 전용 헬퍼를 1회 설치하면\n이후 암호 없이(NOPASSWD) 슬라이더로 바로 조절할 수 있습니다.")
                            .size(11).color(C_DIM),
                        Space::with_height(10),
                        action_btn("권한 헬퍼 설치 (1회)", UsbMsg::InstallHelper, !is_running, C_BLUE),
                    ]
                ));
            }
        } else {
            col = col.push(text("스캔 중...").size(13).color(C_DIM));
        }

        col = col.push(Space::with_height(16));
        col = col.push(
            row![
                Space::with_width(Length::Fill),
                action_btn("새로고침", UsbMsg::Refresh, !is_running, C_BTN2),
                Space::with_width(8),
                action_btn("USB 재인식", UsbMsg::RetrieveAll, !is_running, C_WARN),
                Space::with_width(8),
                action_btn("xHCI 리셋", UsbMsg::XhciReset, !is_running, C_ERR),
            ]
            .align_y(iced::Alignment::Center)
        );

        scrollable(container(col).padding([4, 0])).into()
    }
}

fn device_row(d: &UsbDevice, disabled: bool) -> Element<'_, UsbMsg> {
    let bg = if d.highlight { Color { r: 0.906, g: 0.976, b: 0.949, a: 1.0 } } else { C_SURFACE };
    let border = if d.highlight { C_OK } else { C_BORDER };
    let name_col = if d.highlight { C_OK } else { C_TEXT };

    let name = if d.product.is_empty() { &d.manufacturer } else { &d.product };
    let sub = format!("{} {}:{} {}Mbps", d.manufacturer, d.vid, d.pid, d.speed);

    let sysfs_name = d.sysfs_name.clone();

    container(
        row![
            text(d.icon).size(18),
            Space::with_width(12),
            column![
                text(name).size(13).color(name_col),
                text(sub).size(11).color(C_DIM),
            ].width(Length::Fill),
            action_btn("재인식", UsbMsg::RetrieveDev(sysfs_name), !disabled, C_BTN2),
        ]
        .align_y(iced::Alignment::Center)
    )
    .padding([12, 14])
    .width(Length::Fill)
    .style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(bg)),
        border: iced::Border { radius: 12.0.into(), color: border, width: 1.0 },
        ..Default::default()
    })
    .into()
}

fn failed_port_row(fp: &UsbFailedPort) -> Element<'_, UsbMsg> {
    container(
        row![
            text("[!]").size(13).color(C_ERR),
            Space::with_width(10),
            column![
                text(format!("포트 {} — 열거 실패", fp.port)).size(13).color(C_ERR),
                text("케이블/포트 점검 또는 xHCI 리셋 필요").size(11).color(C_DIM),
            ],
        ]
        .align_y(iced::Alignment::Center)
    )
    .padding([12, 14])
    .width(Length::Fill)
    .style(|_| iced::widget::container::Style {
        background: Some(iced::Background::Color(Color { r: 0.996, g: 0.925, b: 0.933, a: 1.0 })),
        border: iced::Border { radius: 12.0.into(), color: C_ERR, width: 1.0 },
        ..Default::default()
    })
    .into()
}

fn xhci_confirm_card<'a>() -> Element<'a, UsbMsg> {
    card(
        column![
            text("xHCI 컨트롤러 리셋").size(14).color(C_ERR),
            Space::with_height(8),
            text("모든 USB 장치가 잠시 연결 해제됩니다.\n계속하시겠습니까?").size(13),
            Space::with_height(16),
            row![
                action_btn("취소", UsbMsg::CancelXhci, true, C_BTN2),
                Space::with_width(10),
                action_btn("리셋 실행", UsbMsg::ConfirmXhci, true, C_ERR),
            ],
        ]
    )
}

async fn scan_usb() -> UsbStatus {
    let mut devices = Vec::new();

    let entries = std::fs::read_dir("/sys/bus/usb/devices").unwrap_or_else(|_| {
        std::fs::read_dir("/tmp").unwrap()
    });

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        // 실제 장치만 (usb1, 1-1 등; 포트 인터페이스 제외)
        if name.contains(':') { continue; }
        // 루트 허브(usb1/usb2/...)는 재인식 대상 아님
        if name.starts_with("usb") { continue; }

        let read = |f: &str| std::fs::read_to_string(path.join(f)).unwrap_or_default().trim().to_string();
        let vid = read("idVendor");
        let pid = read("idProduct");
        if vid.is_empty() { continue; }

        let manufacturer = read("manufacturer");
        let product      = read("product");
        let speed        = read("speed");
        let bus          = read("busnum");
        let dev          = read("devnum");

        let highlight = vid == "047d" || vid == "0853";  // Kensington / Realforce
        let icon: &'static str = if vid == "047d" { "[M]" } else if vid == "0853" { "[K]" } else { "[U]" };

        devices.push(UsbDevice {
            vid, pid, manufacturer, product, speed, bus, dev,
            sysfs_name: name,
            icon, highlight,
        });
    }
    devices.sort_by(|a, b| a.sysfs_name.cmp(&b.sysfs_name));

    // 열거 실패 포트
    let journal = runner::run("bash", &["-c", "LC_ALL=C journalctl -k -n 200 --no-pager 2>/dev/null"]).await;
    let mut failed_ports = Vec::new();
    for line in journal.output.lines() {
        if line.contains("unable to enumerate USB device") {
            if let Some(port) = extract_port(line) {
                if !failed_ports.iter().any(|f: &UsbFailedPort| f.port == port) {
                    failed_ports.push(UsbFailedPort { port });
                }
            }
        }
    }

    // ktrackball PID
    let ktb = runner::run("pgrep", &["-x", "ktrackball"]).await;
    let ktrackball_pid = ktb.output.trim().parse::<u32>().ok();

    let pointer_speed = read_pointer_speed();
    let tb_speed_factor = read_tb_speed_factor();
    let cursor_size = read_cursor_size().await;
    let helper_installed = helper_ok().await;

    UsbStatus { devices, failed_ports, ktrackball_pid, pointer_speed,
                tb_speed_factor, cursor_size, helper_installed }
}

/// /etc/ktrackball/config.toml 의 speed_factor 값 (없으면 1.0).
fn read_tb_speed_factor() -> f64 {
    let content = match std::fs::read_to_string("/etc/ktrackball/config.toml") {
        Ok(c) => c, Err(_) => return 1.0,
    };
    for line in content.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("speed_factor") {
            if let Some(eq) = rest.find('=') {
                if let Ok(v) = rest[eq + 1..].trim().parse::<f64>() {
                    return v.clamp(1.0, 3.0);
                }
            }
        }
    }
    1.0
}

/// XCURSOR_SIZE(우선) 또는 gsettings cursor-size (없으면 24).
async fn read_cursor_size() -> i32 {
    if let Ok(content) = std::fs::read_to_string("/etc/environment") {
        for line in content.lines() {
            if let Some(rest) = line.trim().strip_prefix("XCURSOR_SIZE=") {
                if let Ok(v) = rest.trim().parse::<i32>() { return v; }
            }
        }
    }
    let g = runner::run("gsettings", &["get", "org.gnome.desktop.interface", "cursor-size"]).await;
    g.output.trim().parse::<i32>().unwrap_or(24)
}

/// popmgr-helper 가 설치되고 NOPASSWD 로 호출 가능한지.
async fn helper_ok() -> bool {
    if !std::path::Path::new("/usr/local/bin/popmgr-helper").exists() { return false; }
    let r = runner::run_sh("sudo -n /usr/local/bin/popmgr-helper check 2>/dev/null").await;
    r.success && r.output.trim() == "ok"
}

/// 표준 base64 인코딩 (외부 크레이트 없이).
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

fn cosmic_input_path() -> std::path::PathBuf {
    let mut p = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    p.push("cosmic/com.system76.CosmicComp/v1/input_default");
    p
}

/// input_default RON 에서 acceleration.speed 값을 파싱한다.
fn read_pointer_speed() -> Option<f64> {
    let content = std::fs::read_to_string(cosmic_input_path()).ok()?;
    let idx = content.find("speed:")?;
    let rest = content[idx + "speed:".len()..].trim_start();
    let num: String = rest.chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    num.parse().ok()
}

/// acceleration.speed 만 교체(나머지 설정 보존). 없으면 기본 구조로 새로 작성.
fn write_pointer_speed(speed: f64) -> Result<(), String> {
    let path = cosmic_input_path();
    let speed_str = format!("{speed:.6}");
    let existing = std::fs::read_to_string(&path).ok();

    let new_content = match existing {
        Some(content) if content.contains("acceleration:") && content.contains("speed:") => {
            let idx = content.find("speed:").unwrap();
            let after = idx + "speed:".len();
            let ws = content[after..].len() - content[after..].trim_start().len();
            let num_start = after + ws;
            let num_len: usize = content[num_start..].chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                .map(|c| c.len_utf8())
                .sum();
            format!("{}{}{}", &content[..num_start], speed_str, &content[num_start + num_len..])
        }
        _ => format!(
            "(\n    state: Enabled,\n    acceleration: Some((\n        profile: Some(Adaptive),\n        speed: {speed_str},\n    )),\n    left_handed: Some(false),\n)\n"
        ),
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&path, new_content).map_err(|e| e.to_string())
}

fn extract_port(line: &str) -> Option<String> {
    // "usb 1-1.2: unable to enumerate" → "1-1.2"
    for part in line.split_whitespace() {
        let p = part.trim_end_matches(':');
        if p.contains('-') && p.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            return Some(p.to_string());
        }
    }
    None
}
