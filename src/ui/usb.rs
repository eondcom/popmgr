use iced::{
    widget::{button, column, container, row, scrollable, slider, text, Space},
    Color, Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_BORDER, C_BTN2, C_DIM, C_ERR, C_OK, C_SURFACE, C_TEXT, C_WARN, C_OK_BG, C_ERR_BG};

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
  sync-trackball-match)
    # 블루투스로 연결하면 장치 이름이 바뀐다(동글: "Kensington Expert Wireless TB Mouse",
    # BT: "ExpertBT5.0 Mouse"). device_match 에 BT 이름이 없으면 데몬이 장치를 못 찾고
    # 종료되어 speed_factor·버튼매핑이 전부 무효가 된다. 누락된 패턴만 덧붙인다.
    [ -f "$CONF" ] || { echo "설정 없음: $CONF"; exit 1; }
    changed=0
    for pat in ExpertBT SlimbladeBT; do
      grep -q "\"$pat\"" "$CONF" && continue
      if grep -qE '^[[:space:]]*device_match[[:space:]]*=' "$CONF"; then
        sed -i -E "s|^([[:space:]]*device_match[[:space:]]*=[[:space:]]*\[)|\1\"$pat\", |" "$CONF"
        changed=1
      fi
    done
    if [ "$changed" -eq 1 ]; then
      systemctl restart ktrackball.service
      echo "device_match 에 블루투스 장치 이름을 추가하고 ktrackball 을 재시작했습니다."
      grep -E '^[[:space:]]*device_match' "$CONF"
    else
      echo "device_match 에 이미 블루투스 이름이 있습니다."
    fi
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
    pub bt: BtStatus,                  // 블루투스 트랙볼 상태
}

/// 블루투스 어댑터 + 트랙볼(HID) 연결 상태.
#[derive(Debug, Clone, Default)]
pub struct BtStatus {
    pub powered: bool,             // 컨트롤러 전원
    pub blocked: bool,             // rfkill soft/hard block
    pub devices: Vec<BtDevice>,    // 페어링됐거나 방금 스캔된 HID 후보
}

#[derive(Debug, Clone)]
pub struct BtDevice {
    pub mac: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
    pub trusted: bool,
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
    BtPair,                  // 스캔→페어링→trust→연결 (단일 bluetoothctl 세션)
    BtConnect(String),       // 이미 페어링된 기기 재연결
    BtDisconnect(String),
    BtForget(String),        // 페어링 해제(재등록 필요)
    BtSyncMatch,             // ktrackball device_match 에 BT 장치 이름 추가
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
            UsbMsg::BtPair => {
                self.running = Some("블루투스 검색 중... (버튼 4개 3초로 페어링 모드 진입)".into());
                let t = Task::perform(async { bt_pair_flow().await }, UsbMsg::Done);
                (t, None)
            }
            UsbMsg::BtConnect(mac) => {
                self.running = Some("연결 중...".into());
                let q = shell_quote(&mac);
                let script = format!("bluetoothctl connect {q} 2>&1 | tail -3");
                let t = Task::perform(async move { runner::run_sh(&script).await }, UsbMsg::Done);
                (t, None)
            }
            UsbMsg::BtDisconnect(mac) => {
                self.running = Some("연결 해제 중...".into());
                let q = shell_quote(&mac);
                let script = format!("bluetoothctl disconnect {q} 2>&1 | tail -3");
                let t = Task::perform(async move { runner::run_sh(&script).await }, UsbMsg::Done);
                (t, None)
            }
            UsbMsg::BtForget(mac) => {
                self.running = Some("페어링 해제 중...".into());
                let q = shell_quote(&mac);
                let script = format!(
                    "bluetoothctl remove {q} 2>&1 | tail -3; echo '페어링을 해제했습니다. 다시 등록하려면 페어링 모드로 만든 뒤 [트랙볼 페어링]을 누르세요.'"
                );
                let t = Task::perform(async move { runner::run_sh(&script).await }, UsbMsg::Done);
                (t, None)
            }
            UsbMsg::BtSyncMatch => {
                self.running = Some("ktrackball 설정 동기화 중...".into());
                let t = Task::perform(
                    async { runner::run_sh(
                        "sudo -n /usr/local/bin/popmgr-helper sync-trackball-match 2>&1"
                    ).await },
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

            // 블루투스 트랙볼
            col = col.push(bt_card(&st.bt, st.ktrackball_pid.is_some(), is_running));
            col = col.push(Space::with_height(10));

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

/// 블루투스 트랙볼 카드: 상태 표시 + 페어링/연결/해제.
///
/// 모드 전환(2.4GHz 동글 <-> BT)은 기기 펌웨어가 물리 버튼으로만 처리하므로
/// 소프트웨어로 제어할 수 없다. 여기서는 BT 쪽 스캔·페어링·연결만 담당한다.
fn bt_card(bt: &BtStatus, ktb_running: bool, is_running: bool) -> Element<'_, UsbMsg> {
    let mut inner = column![
        row![
            text("블루투스 트랙볼").size(13).color(C_TEXT),
            Space::with_width(Length::Fill),
            action_btn("트랙볼 페어링", UsbMsg::BtPair, !is_running, C_BLUE),
        ].align_y(iced::Alignment::Center),
        Space::with_height(8),
    ];

    let (ad_txt, ad_col) = if bt.blocked {
        ("○ 블루투스 차단됨 (rfkill)".to_string(), C_ERR)
    } else if bt.powered {
        ("● 블루투스 켜짐".to_string(), C_OK)
    } else {
        ("○ 블루투스 꺼짐".to_string(), C_ERR)
    };
    inner = inner.push(text(ad_txt).size(12).color(ad_col));

    if bt.devices.is_empty() {
        inner = inner.push(Space::with_height(6));
        inner = inner.push(
            text("등록된 트랙볼이 없습니다. 기기를 페어링 모드로 만든 뒤 [트랙볼 페어링]을 누르세요.")
                .size(11).color(C_DIM)
        );
    } else {
        for d in &bt.devices {
            inner = inner.push(Space::with_height(6));
            let (st_txt, st_col) = if d.connected {
                ("● 연결됨", C_OK)
            } else if d.paired {
                ("○ 페어링됨 (연결 안 됨)", C_WARN)
            } else {
                ("○ 미등록", C_DIM)
            };
            let mut actions = row![].spacing(6).align_y(iced::Alignment::Center);
            if d.connected {
                actions = actions.push(action_btn(
                    "연결 해제", UsbMsg::BtDisconnect(d.mac.clone()), !is_running, C_BTN2));
            } else if d.paired {
                actions = actions.push(action_btn(
                    "연결", UsbMsg::BtConnect(d.mac.clone()), !is_running, C_BLUE));
            }
            if d.paired {
                actions = actions.push(action_btn(
                    "등록 해제", UsbMsg::BtForget(d.mac.clone()), !is_running, C_WARN));
            }
            inner = inner.push(
                container(column![
                    row![
                        text(d.name.clone()).size(12).color(C_TEXT),
                        Space::with_width(Length::Fill),
                        actions,
                    ].align_y(iced::Alignment::Center),
                    Space::with_height(2),
                    row![
                        text(st_txt).size(11).color(st_col),
                        Space::with_width(8),
                        text(d.mac.clone()).size(10).color(C_DIM),
                    ].align_y(iced::Alignment::Center),
                ])
                .padding(8)
                .style(|_: &iced::Theme| container::Style {
                    background: Some(C_SURFACE.into()),
                    border: iced::Border { color: C_BORDER, width: 1.0, radius: 4.0.into() },
                    ..Default::default()
                })
            );
        }
    }

    // 블루투스로는 장치 이름이 `ExpertBT5.0 Mouse` 로 바뀌므로 ktrackball 의
    // device_match(`Expert Wireless`)에 안 걸린다. 그러면 데몬이 장치를 못 찾고 죽어
    // speed_factor·버튼매핑이 전부 무효가 되고, 사용자는 "포인터가 밀린다"고 느낀다.
    let bt_connected = bt.devices.iter().any(|d| d.connected);
    if bt_connected && !ktb_running {
        inner = inner.push(Space::with_height(8));
        inner = inner.push(
            container(column![
                text("⚠ ktrackball 데몬이 실행되고 있지 않습니다")
                    .size(12).color(C_WARN),
                Space::with_height(4),
                text("블루투스로 연결하면 장치 이름이 `ExpertBT5.0` 으로 바뀌어 device_match 에 걸리지 않습니다. 데몬이 장치를 못 찾고 종료되어 포인터 배율·버튼 매핑이 적용되지 않습니다(포인터가 느리게 느껴집니다).")
                    .size(11).color(C_DIM),
                Space::with_height(6),
                row![
                    Space::with_width(Length::Fill),
                    action_btn("BT 이름 동기화 후 재시작", UsbMsg::BtSyncMatch, !is_running, C_WARN),
                ].align_y(iced::Alignment::Center),
            ])
            .padding(8)
            .style(|_: &iced::Theme| container::Style {
                background: Some(C_SURFACE.into()),
                border: iced::Border { color: C_WARN, width: 1.0, radius: 4.0.into() },
                ..Default::default()
            })
        );
    }

    inner = inner.push(Space::with_height(8));
    inner = inner.push(
        text("페어링 모드: Kensington Expert 는 상단 버튼 4개를 동시에 3초간 누릅니다(바닥에 별도 페어링 버튼 없음). 동글(2.4GHz) <-> 블루투스 전환은 기기 버튼으로만 가능하며 소프트웨어로는 바꿀 수 없습니다.")
            .size(11).color(C_DIM)
    );

    card(inner)
}

fn device_row(d: &UsbDevice, disabled: bool) -> Element<'_, UsbMsg> {
    let bg = if d.highlight { C_OK_BG } else { C_SURFACE };
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
        background: Some(iced::Background::Color(C_ERR_BG)),
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

    // ktrackball PID.
    // 실행 파일은 python3 이고 ktrackball 은 스크립트 경로에만 나타나므로
    // `pgrep -x ktrackball`(프로세스명 완전일치)로는 절대 잡히지 않는다.
    // 전체 명령줄(-f)에서 mapper 스크립트 경로를 찾아야 한다.
    // 패턴에 경로를 포함시켜 "trackball_mapper.py" 문자열을 인자로 가진 다른
    // 프로세스(예: 이 이름을 grep 하는 셸)가 섞이지 않게 한다.
    let ktb = runner::run("pgrep", &["-f", r"python3?\s+/opt/ktrackball/trackball_mapper\.py"]).await;
    let ktrackball_pid = ktb.output
        .lines()
        .find_map(|l| l.trim().parse::<u32>().ok());

    let pointer_speed = read_pointer_speed();
    let tb_speed_factor = read_tb_speed_factor();
    let cursor_size = read_cursor_size().await;
    let helper_installed = helper_ok().await;
    let bt = scan_bt().await;

    UsbStatus { devices, failed_ports, ktrackball_pid, pointer_speed,
                tb_speed_factor, cursor_size, helper_installed, bt }
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
/// 블루투스 HID(마우스/트랙볼) 후보로 볼 이름 패턴.
/// Kensington Expert 는 `ExpertBT5.0` / `ExpertBT3.0` 으로 광고한다(제품명에 Kensington 없음).
fn is_hid_candidate(name: &str, icon: &str, uuids: &str) -> bool {
    if uuids.contains("00001812") { return true; }          // HID over GATT
    if icon == "input-mouse" || icon == "input-keyboard" { return true; }
    let n = name.to_ascii_lowercase();
    ["expert", "kensington", "trackball", "slimblade", "orbit", "mouse"]
        .iter().any(|k| n.contains(k))
}

/// 현재 블루투스 상태 + HID 후보 목록을 읽는다.
/// 스캔은 하지 않는다(이미 알려진 기기만). 스캔은 BtPair 에서만 수행.
async fn scan_bt() -> BtStatus {
    let show = runner::run_sh("bluetoothctl show 2>/dev/null").await;
    let powered = show.output.lines().any(|l| l.trim() == "Powered: yes");
    let rf = runner::run_sh("rfkill list bluetooth 2>/dev/null").await;
    let blocked = rf.output.lines().any(|l| l.trim().ends_with("blocked: yes"));

    let listing = runner::run_sh("bluetoothctl devices 2>/dev/null").await;
    let mut devices = Vec::new();
    for line in listing.output.lines() {
        // "Device C0:31:F2:BB:80:4A ExpertBT5.0"
        let mut it = line.split_whitespace();
        if it.next() != Some("Device") { continue; }
        let Some(mac) = it.next() else { continue };
        let name = it.collect::<Vec<_>>().join(" ");
        let info = runner::run_sh(&format!(
            "bluetoothctl info {} 2>/dev/null", shell_quote(mac)
        )).await;
        let field = |key: &str| -> String {
            info.output.lines()
                .find_map(|l| l.trim().strip_prefix(key).map(|v| v.trim().to_string()))
                .unwrap_or_default()
        };
        let icon = field("Icon:");
        let uuids = info.output.lines().filter(|l| l.contains("UUID:"))
            .collect::<Vec<_>>().join(" ");
        let disp = {
            let n = field("Name:");
            if n.is_empty() { name.clone() } else { n }
        };
        if !is_hid_candidate(&disp, &icon, &uuids) { continue; }
        devices.push(BtDevice {
            mac: mac.to_string(),
            name: disp,
            paired: field("Paired:") == "yes",
            connected: field("Connected:") == "yes",
            trusted: field("Trusted:") == "yes",
        });
    }
    // 연결됨 → 페어링됨 → 나머지 순
    devices.sort_by_key(|d| (!d.connected, !d.paired));
    BtStatus { powered, blocked, devices }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// 스캔 → 페어링 → trust → connect 를 **하나의 bluetoothctl 세션**에서 수행한다.
///
/// 명령마다 `bluetoothctl` 을 새로 띄우면 scan 종료와 함께 발견 캐시가 DEL 되어
/// `Attempting to pair` 직후 중단된다(2026-09-11 실측). coproc 으로 세션을 유지한 채
/// 순차 입력해야 통과한다. docs/bluetooth-usb-trackball-notes.md 참고.
async fn bt_pair_flow() -> CmdResult {
    let script = r#"
set -u
coproc BTC { stdbuf -oL bluetoothctl 2>&1; }
exec 3>&"${BTC[1]}"
cleanup() { echo "scan off" >&3 2>/dev/null || true; echo "quit" >&3 2>/dev/null || true; }
trap cleanup EXIT

echo "power on" >&3;      sleep 1
echo "agent on" >&3;      sleep 1
echo "default-agent" >&3; sleep 1
echo "scan on" >&3

# HID 후보가 나타날 때까지 최대 45초 대기
MAC=""
for i in $(seq 1 15); do
  sleep 3
  while read -r _ m rest; do
    [ -n "${m:-}" ] || continue
    info=$(bluetoothctl info "$m" 2>/dev/null)
    icon=$(printf '%s\n' "$info" | sed -n 's/^\s*Icon:\s*//p')
    if printf '%s\n' "$info" | grep -q "00001812" \
       || [ "$icon" = "input-mouse" ] \
       || printf '%s\n' "$rest" | grep -qiE "expert|kensington|trackball|slimblade|orbit"; then
      # 이미 연결된 기기는 건너뛴다
      printf '%s\n' "$info" | grep -q "Connected: yes" && continue
      MAC="$m"; NAME="$rest"; break
    fi
  done < <(bluetoothctl devices 2>/dev/null)
  [ -n "$MAC" ] && break
done

if [ -z "$MAC" ]; then
  echo "트랙볼을 찾지 못했습니다."
  echo ""
  echo "Kensington Expert: 상단 버튼 4개를 동시에 3초간 누르면 페어링 모드로 들어갑니다."
  echo "(바닥에 별도 페어링 버튼은 없습니다)"
  echo "페어링 모드로 만든 뒤 다시 눌러주세요."
  exit 1
fi

echo "발견: $NAME ($MAC)"
echo "pair $MAC" >&3;    sleep 12
echo "trust $MAC" >&3;   sleep 3
echo "connect $MAC" >&3; sleep 10

info=$(bluetoothctl info "$MAC" 2>/dev/null)
paired=$(printf '%s\n' "$info" | grep -c "Paired: yes" || true)
conn=$(printf '%s\n'  "$info" | grep -c "Connected: yes" || true)

if [ "$conn" -ge 1 ]; then
  echo "연결 완료: $NAME"
  printf '%s\n' "$info" | grep -E "Name:|Paired:|Trusted:|Connected:"
  # BT 이름은 동글과 다르므로 ktrackball 의 device_match 를 맞춰준다.
  # 이걸 안 하면 데몬이 장치를 못 찾고 죽어 speed_factor·버튼매핑이 전부 무효가 된다.
  if [ -x /usr/local/bin/popmgr-helper ]; then
    echo ""
    sudo -n /usr/local/bin/popmgr-helper sync-trackball-match 2>&1 || \
      echo "(ktrackball 설정 동기화 실패 - USB 탭의 [BT 이름 동기화] 버튼을 눌러주세요)"
  fi
  exit 0
elif [ "$paired" -ge 1 ]; then
  echo "페어링은 됐지만 연결이 안 됐습니다: $NAME"
  echo "목록의 [연결] 버튼을 눌러보세요."
  exit 1
else
  echo "페어링 실패: $NAME"
  echo "페어링 모드(버튼 4개 3초)를 다시 만든 뒤 시도하세요."
  exit 1
fi
"#;
    runner::run_sh(script).await
}

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
