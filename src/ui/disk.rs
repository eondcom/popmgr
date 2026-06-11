use iced::{
    widget::{column, container, row, scrollable, text, Space},
    Color, Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_DIM, C_GREEN, C_OK};

#[derive(Debug, Clone, PartialEq)]
pub struct PartInfo {
    pub path: String,            // /dev/sdb1
    pub size: String,
    pub fstype: Option<String>,
    pub label: Option<String>,
    pub mountpoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiskGroup {
    pub path: String,            // /dev/sdb
    pub size: String,
    pub model: String,
    pub tran: String,            // usb / nvme / sata ...
    pub removable: bool,
    pub parts: Vec<PartInfo>,
}

#[derive(Debug, Clone)]
pub enum DiskMsg {
    Refresh,
    Refreshed(Vec<DiskGroup>),
    Mount(String),
    Unmount(String),
    PowerOff(String),
    Open(String),
    Applied(CmdResult),
}

pub struct DiskState {
    pub disks: Vec<DiskGroup>,
    pub scanned: bool,
    pub running: Option<String>,
}

impl DiskState {
    pub fn new() -> Self {
        Self { disks: Vec::new(), scanned: false, running: None }
    }

    pub fn update(&mut self, msg: DiskMsg) -> (Task<DiskMsg>, Option<CmdResult>) {
        match msg {
            DiskMsg::Refresh => {
                let t = Task::perform(async { scan_disks().await }, DiskMsg::Refreshed);
                (t, None)
            }
            DiskMsg::Refreshed(d) => {
                self.disks = d;
                self.scanned = true;
                (Task::none(), None)
            }
            DiskMsg::Mount(dev) => {
                self.running = Some(format!("{dev} 마운트 중..."));
                let script = format!("udisksctl mount -b '{dev}' 2>&1");
                (apply(script), None)
            }
            DiskMsg::Unmount(dev) => {
                self.running = Some(format!("{dev} 마운트 해제 중..."));
                let script = format!("udisksctl unmount -b '{dev}' 2>&1");
                (apply(script), None)
            }
            DiskMsg::PowerOff(dev) => {
                self.running = Some(format!("{dev} 안전 제거 중..."));
                let script = format!("udisksctl power-off -b '{dev}' 2>&1 && echo '{dev} 안전 제거 완료 — 케이블을 뽑아도 됩니다.'");
                (apply(script), None)
            }
            DiskMsg::Open(mp) => {
                let script = format!("nohup xdg-open '{mp}' >/dev/null 2>&1 & echo '파일 관리자로 열기: {mp}'");
                (apply(script), None)
            }
            DiskMsg::Applied(r) => {
                self.running = None;
                let t = Task::perform(async { scan_disks().await }, DiskMsg::Refreshed);
                (t, Some(r))
            }
        }
    }

    pub fn view(&self) -> Element<'_, DiskMsg> {
        let mut col = column![
            text("디스크").size(20),
            Space::with_height(6),
            text("외장하드/USB 연결 상태를 보고 마운트·해제·안전 제거를 합니다. 연결하면 자동으로 목록에 나타납니다.")
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

        let is_busy = self.running.is_some();
        let (external, internal): (Vec<_>, Vec<_>) =
            self.disks.iter().partition(|d| d.removable || d.tran == "usb");

        // 외장 디스크
        col = col.push(text("외장 디스크").size(14).color(Color::from_rgb(0.7, 0.7, 0.8)));
        col = col.push(Space::with_height(8));
        if external.is_empty() {
            col = col.push(card(
                text("연결된 외장 디스크 없음").size(12).color(C_DIM)
            ));
        } else {
            for d in &external {
                col = col.push(disk_card(d, is_busy, true));
                col = col.push(Space::with_height(10));
            }
        }
        col = col.push(Space::with_height(14));

        // 내장 파티션 (마운트 안 된 NTFS 등 — 윈도우 파티션 접근용)
        let internal_mountable: Vec<&DiskGroup> = internal
            .into_iter()
            .filter(|d| d.parts.iter().any(part_actionable))
            .collect();
        if !internal_mountable.is_empty() {
            col = col.push(text("내장 파티션").size(14).color(Color::from_rgb(0.7, 0.7, 0.8)));
            col = col.push(Space::with_height(8));
            for d in internal_mountable {
                col = col.push(disk_card(d, is_busy, false));
                col = col.push(Space::with_height(10));
            }
        }

        col = col.push(Space::with_height(8));
        let actions = row![
            Space::with_width(Length::Fill),
            action_btn("새로고침", DiskMsg::Refresh, !is_busy, Color::from_rgb(0.3, 0.3, 0.4)),
        ];
        col = col.push(actions);

        scrollable(container(col).padding([4, 0])).into()
    }
}

/// 보여줄 가치가 있는 파티션: 마운트 가능(미마운트+파일시스템 있음)
/// 또는 사용자 마운트 지점(/media, /mnt)에 마운트됨
fn part_actionable(p: &PartInfo) -> bool {
    match &p.mountpoint {
        None => p.fstype.as_deref().is_some_and(|f| f != "swap"),
        Some(mp) => is_user_mount(mp),
    }
}

fn is_user_mount(mp: &str) -> bool {
    mp.starts_with("/media/") || mp.starts_with("/mnt/") || mp.starts_with("/run/media/")
}

fn disk_card<'a>(d: &'a DiskGroup, is_busy: bool, external: bool) -> Element<'a, DiskMsg> {
    let title = if d.model.is_empty() {
        format!("{} ({})", d.path, d.size)
    } else {
        format!("{} — {} ({})", d.model, d.path, d.size)
    };
    let tran_tag = if d.tran.is_empty() { String::new() } else { format!("  [{}]", d.tran.to_uppercase()) };

    let mut body = column![
        row![
            text(title).size(13),
            text(tran_tag).size(11).color(C_BLUE),
        ].align_y(iced::Alignment::Center),
        Space::with_height(8),
    ];

    let parts: Vec<&PartInfo> = if external {
        d.parts.iter().collect()
    } else {
        d.parts.iter().filter(|p| part_actionable(p)).collect()
    };

    if parts.is_empty() {
        body = body.push(text("파티션 없음 (미디어 없음 또는 포맷 필요)").size(12).color(C_DIM));
    }

    let mut any_mounted = false;
    for p in parts {
        let name = match (&p.label, &p.fstype) {
            (Some(l), Some(f)) => format!("{l} ({f}, {})", p.size),
            (Some(l), None) => format!("{l} ({})", p.size),
            (None, Some(f)) => format!("{} ({f}, {})", p.path, p.size),
            (None, None) => format!("{} ({})", p.path, p.size),
        };

        let mut r = row![
            container(text(name).size(12)).width(Length::Fill),
        ].align_y(iced::Alignment::Center);

        match &p.mountpoint {
            Some(mp) => {
                any_mounted = true;
                r = r.push(
                    container(text(format!("● {mp}")).size(11).color(C_OK))
                        .width(Length::Shrink)
                );
                r = r.push(Space::with_width(8));
                r = r.push(action_btn("열기", DiskMsg::Open(mp.clone()), !is_busy, C_GREEN));
                if is_user_mount(mp) {
                    r = r.push(Space::with_width(6));
                    r = r.push(action_btn("해제", DiskMsg::Unmount(p.path.clone()), !is_busy, Color::from_rgb(0.6, 0.4, 0.15)));
                }
            }
            None => {
                if p.fstype.as_deref().is_some_and(|f| f != "swap") {
                    r = r.push(text("○ 마운트 안 됨").size(11).color(C_DIM));
                    r = r.push(Space::with_width(8));
                    r = r.push(action_btn("마운트", DiskMsg::Mount(p.path.clone()), !is_busy, C_BLUE));
                } else {
                    r = r.push(text("파일시스템 없음").size(11).color(C_DIM));
                }
            }
        }
        body = body.push(r);
        body = body.push(Space::with_height(4));
    }

    // 외장 디스크만: 전부 해제된 상태에서 안전 제거(전원 차단)
    if external {
        body = body.push(Space::with_height(6));
        let mut bottom = row![Space::with_width(Length::Fill)].align_y(iced::Alignment::Center);
        if any_mounted {
            bottom = bottom.push(text("안전 제거하려면 먼저 모든 파티션을 해제하세요").size(10).color(C_DIM));
            bottom = bottom.push(Space::with_width(8));
        }
        bottom = bottom.push(action_btn(
            "안전 제거",
            DiskMsg::PowerOff(d.path.clone()),
            !is_busy && !any_mounted,
            Color::from_rgb(0.55, 0.2, 0.2),
        ));
        body = body.push(bottom);
    }

    card(body)
}

fn apply(script: String) -> Task<DiskMsg> {
    Task::perform(async move { runner::run_sh(&script).await }, DiskMsg::Applied)
}

async fn scan_disks() -> Vec<DiskGroup> {
    let r = runner::run("lsblk", &[
        "-J", "-o", "NAME,PATH,SIZE,TYPE,FSTYPE,LABEL,MOUNTPOINT,RM,TRAN,MODEL",
    ]).await;
    if !r.success { return Vec::new(); }
    parse_lsblk(&r.output)
}

fn parse_lsblk(json: &str) -> Vec<DiskGroup> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let Some(devs) = v.get("blockdevices").and_then(|b| b.as_array()) else { return Vec::new() };

    let s = |v: &serde_json::Value, k: &str| -> String {
        v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string()
    };
    let opt = |v: &serde_json::Value, k: &str| -> Option<String> {
        v.get(k).and_then(|x| x.as_str()).map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
    };

    let mut out = Vec::new();
    for d in devs {
        if s(d, "type") != "disk" { continue; }
        let size = s(d, "size");
        if size == "0B" { continue; } // 미디어 없는 카드리더 슬롯 등
        let path = s(d, "path");
        if path.starts_with("/dev/zram") || path.starts_with("/dev/loop") { continue; }

        let mut parts = Vec::new();
        if let Some(children) = d.get("children").and_then(|c| c.as_array()) {
            for c in children {
                if s(c, "type") != "part" { continue; }
                parts.push(PartInfo {
                    path: s(c, "path"),
                    size: s(c, "size"),
                    fstype: opt(c, "fstype"),
                    label: opt(c, "label"),
                    mountpoint: opt(c, "mountpoint"),
                });
            }
        } else if opt(d, "fstype").is_some() {
            // 파티션 테이블 없이 디스크 전체가 파일시스템인 경우
            parts.push(PartInfo {
                path: path.clone(),
                size: size.clone(),
                fstype: opt(d, "fstype"),
                label: opt(d, "label"),
                mountpoint: opt(d, "mountpoint"),
            });
        }

        out.push(DiskGroup {
            path,
            size,
            model: s(d, "model"),
            tran: s(d, "tran"),
            removable: d.get("rm").and_then(|x| x.as_bool()).unwrap_or(false),
            parts,
        });
    }
    out
}
