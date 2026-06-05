use iced::{
    widget::{button, column, container, row, scrollable, text, Space},
    Color, Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_DIM, C_ERR, C_OK, C_WARN};

#[derive(Debug, Clone)]
pub struct UsbDevice {
    pub vid: String,
    pub pid: String,
    pub manufacturer: String,
    pub product: String,
    pub speed: String,
    pub bus: String,
    pub dev: String,
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
}

#[derive(Debug, Clone)]
pub enum UsbMsg {
    Refresh,
    Refreshed(UsbStatus),
    RetrieveDev(String, String),   // bus, dev
    RetrieveAll,
    XhciReset,
    RestartKtrackball,
    Done(CmdResult),
    ConfirmXhci,
    CancelXhci,
}

pub struct UsbState {
    pub status: Option<UsbStatus>,
    pub running: Option<String>,
    pub confirm_xhci: bool,
}

impl UsbState {
    pub fn new() -> Self {
        Self { status: None, running: None, confirm_xhci: false }
    }

    pub fn update(&mut self, msg: UsbMsg) -> (Task<UsbMsg>, Option<CmdResult>) {
        match msg {
            UsbMsg::Refresh => {
                let t = Task::perform(async { scan_usb().await }, UsbMsg::Refreshed);
                (t, None)
            }
            UsbMsg::Refreshed(s) => { self.status = Some(s); (Task::none(), None) }
            UsbMsg::RetrieveDev(bus, dev) => {
                self.running = Some("장치 재인식 중...".into());
                let path = format!("/sys/bus/usb/devices/usb{bus}/{bus}-{dev}");
                let script = format!("pkexec bash -c 'echo 0 > {path}/authorized && echo 1 > {path}/authorized'");
                let t = Task::perform(async move { runner::run_sh(&script).await }, UsbMsg::Done);
                (t, None)
            }
            UsbMsg::RetrieveAll => {
                self.running = Some("USB 전체 재인식 중...".into());
                let t = Task::perform(
                    async { runner::run_sh("pkexec udevadm trigger --subsystem-match=usb").await },
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
                Some(pid) => (format!("✓ ktrackball 실행 중 (PID {pid})"), C_OK),
                None      => ("✗ ktrackball 중지됨".into(), C_ERR),
            };
            col = col.push(card(
                column![
                    text("ktrackball 데몬").size(13).color(Color::from_rgb(0.7, 0.7, 0.8)),
                    Space::with_height(6),
                    row![
                        text(ktb_txt).size(12).color(ktb_col),
                        Space::with_width(Length::Fill),
                        action_btn("재시작", UsbMsg::RestartKtrackball, !is_running, Color::from_rgb(0.7, 0.3, 0.1)),
                    ].align_y(iced::Alignment::Center),
                ]
            ));
        } else {
            col = col.push(text("스캔 중...").size(13).color(C_DIM));
        }

        col = col.push(Space::with_height(16));
        col = col.push(
            row![
                Space::with_width(Length::Fill),
                action_btn("새로고침", UsbMsg::Refresh, !is_running, Color::from_rgb(0.25, 0.25, 0.35)),
                Space::with_width(8),
                action_btn("USB 재인식", UsbMsg::RetrieveAll, !is_running, C_WARN),
                Space::with_width(8),
                action_btn("xHCI 리셋", UsbMsg::XhciReset, !is_running, Color::from_rgb(0.75, 0.15, 0.15)),
            ]
            .align_y(iced::Alignment::Center)
        );

        scrollable(container(col).padding([4, 0])).into()
    }
}

fn device_row(d: &UsbDevice, disabled: bool) -> Element<'_, UsbMsg> {
    let bg = if d.highlight { Color::from_rgb(0.06, 0.14, 0.06) } else { Color::from_rgb(0.1, 0.1, 0.13) };
    let border = if d.highlight { Color::from_rgb(0.15, 0.4, 0.15) } else { Color::from_rgb(0.2, 0.2, 0.25) };
    let name_col = if d.highlight { C_OK } else { Color::from_rgb(0.85, 0.85, 0.9) };

    let name = if d.product.is_empty() { &d.manufacturer } else { &d.product };
    let sub = format!("{} {}:{} {}Mbps", d.manufacturer, d.vid, d.pid, d.speed);

    let bus = d.bus.clone();
    let dev = d.dev.clone();

    container(
        row![
            text(d.icon).size(18),
            Space::with_width(10),
            column![
                text(name).size(13).color(name_col),
                text(sub).size(11).color(C_DIM),
            ].width(Length::Fill),
            action_btn("재인식", UsbMsg::RetrieveDev(bus, dev), !disabled, Color::from_rgb(0.25, 0.25, 0.35)),
        ]
        .align_y(iced::Alignment::Center)
    )
    .padding([8, 12])
    .width(Length::Fill)
    .style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(bg)),
        border: iced::Border { radius: 7.0.into(), color: border, width: 1.0 },
        ..Default::default()
    })
    .into()
}

fn failed_port_row(fp: &UsbFailedPort) -> Element<'_, UsbMsg> {
    container(
        row![
            text("⚠").size(16),
            Space::with_width(10),
            column![
                text(format!("포트 {} — 열거 실패", fp.port)).size(13).color(C_ERR),
                text("케이블/포트 점검 또는 xHCI 리셋 필요").size(11).color(C_DIM),
            ],
        ]
        .align_y(iced::Alignment::Center)
    )
    .padding([8, 12])
    .width(Length::Fill)
    .style(|_| iced::widget::container::Style {
        background: Some(iced::Background::Color(Color::from_rgb(0.14, 0.05, 0.05))),
        border: iced::Border { radius: 7.0.into(), color: C_ERR, width: 1.0 },
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
                action_btn("취소", UsbMsg::CancelXhci, true, Color::from_rgb(0.25, 0.25, 0.35)),
                Space::with_width(10),
                action_btn("리셋 실행", UsbMsg::ConfirmXhci, true, Color::from_rgb(0.75, 0.15, 0.15)),
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
        let icon: &'static str = if vid == "047d" { "🖱" } else if vid == "0853" { "⌨" } else { "🔌" };

        devices.push(UsbDevice { vid, pid, manufacturer, product, speed, bus, dev, icon, highlight });
    }

    // 열거 실패 포트
    let journal = runner::run("bash", &["-c", "journalctl -k -n 200 --no-pager 2>/dev/null"]).await;
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

    UsbStatus { devices, failed_ports, ktrackball_pid }
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
