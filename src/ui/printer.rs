use iced::{
    widget::{column, container, row, scrollable, text, Space},
    Color, Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_DIM, C_GREEN, C_OK, C_BTN2, C_WARN, C_ERR};

/// CUPS 에 등록된 인쇄 큐.
#[derive(Debug, Clone, PartialEq)]
pub struct Queue {
    pub name: String,
    pub uri: String,
    pub model: String,   // printer-make-and-model (= PPD NickName)
    pub state: u8,       // 3 대기, 4 인쇄중, 5 정지됨
    pub is_default: bool,
    pub jobs: usize,
}

/// lpinfo 로 보이는 실제 장치. 등록 여부와 무관하게 잡힌다.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub uri: String,
    pub info: String,
    pub device_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Level { Error, Warn, Info }

/// 진단 결과 한 줄.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub level: Level,
    pub title: String,
    pub detail: String,
    /// 이 문제를 없애는 가장 흔한 조치(큐 삭제 등)를 바로 걸어줄 대상 큐.
    pub fix_queue: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scan {
    pub queues: Vec<Queue>,
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone)]
pub enum PrinterMsg {
    Refresh,
    Refreshed(Scan),
    SetDefault(String),
    Delete(String),
    TestPrint(String),
    CancelJobs(String),
    Register(String), // 장치 URI
    Applied(CmdResult),
}

pub struct PrinterState {
    pub scan: Scan,
    pub scanned: bool,
    pub running: Option<String>,
}

impl PrinterState {
    pub fn new() -> Self {
        Self { scan: Scan::default(), scanned: false, running: None }
    }

    pub fn update(&mut self, msg: PrinterMsg) -> (Task<PrinterMsg>, Option<CmdResult>) {
        match msg {
            PrinterMsg::Refresh => {
                (Task::perform(async { scan().await }, PrinterMsg::Refreshed), None)
            }
            PrinterMsg::Refreshed(s) => {
                self.scan = s;
                self.scanned = true;
                (Task::none(), None)
            }
            PrinterMsg::SetDefault(q) => {
                self.running = Some(format!("{q} 을(를) 기본 프린터로 지정 중..."));
                let s = format!("lpoptions -d '{q}' >/dev/null 2>&1 && echo '기본 프린터: {q}'");
                (apply(s), None)
            }
            PrinterMsg::Delete(q) => {
                self.running = Some(format!("{q} 삭제 중..."));
                let s = format!(
                    "cancel -a '{q}' 2>/dev/null; lpadmin -x '{q}' 2>&1 && echo '큐 삭제 완료: {q}'"
                );
                (apply(s), None)
            }
            PrinterMsg::TestPrint(q) => {
                self.running = Some(format!("{q} 테스트 페이지 인쇄 중..."));
                (apply(test_print_script(&q)), None)
            }
            PrinterMsg::CancelJobs(q) => {
                self.running = Some(format!("{q} 대기 작업 취소 중..."));
                let s = format!(
                    "cancel -a '{q}' 2>&1; cupsenable '{q}' 2>/dev/null; \
                     echo '{q} 대기 작업을 모두 취소하고 큐를 다시 켰습니다.'"
                );
                (apply(s), None)
            }
            PrinterMsg::Register(uri) => {
                self.running = Some("드라이버를 고르고 등록하는 중...".into());
                let did = self
                    .scan
                    .devices
                    .iter()
                    .find(|d| d.uri == uri)
                    .map(|d| d.device_id.clone())
                    .unwrap_or_default();
                (apply(register_script(&uri, &did)), None)
            }
            PrinterMsg::Applied(r) => {
                self.running = None;
                (Task::perform(async { scan().await }, PrinterMsg::Refreshed), Some(r))
            }
        }
    }

    pub fn view(&self) -> Element<'_, PrinterMsg> {
        let mut col = column![
            text("프린터").size(20),
            Space::with_height(6),
            text("연결된 프린터를 등록하고, 잘못 잡힌 큐(다른 기종 드라이버·중복 등록)를 찾아 정리합니다.")
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

        let busy = self.running.is_some();
        let issues = diagnose(&self.scan);

        // 진단 — 가장 먼저 보여야 할 정보
        if !issues.is_empty() {
            col = col.push(text("진단").size(14).color(C_DIM));
            col = col.push(Space::with_height(8));
            for i in issues {
                col = col.push(issue_card(i, busy));
                col = col.push(Space::with_height(8));
            }
            col = col.push(Space::with_height(10));
        }

        // 등록된 큐
        col = col.push(text("등록된 프린터").size(14).color(C_DIM));
        col = col.push(Space::with_height(8));
        if self.scan.queues.is_empty() {
            col = col.push(card(column![
                text("등록된 프린터 없음").size(12).color(C_DIM),
                Space::with_height(4),
                text("아래 '감지된 장치'에서 [등록]을 누르면 기종에 맞는 드라이버를 골라 자동으로 추가합니다.")
                    .size(10).color(C_DIM),
            ]));
        } else {
            for q in &self.scan.queues {
                col = col.push(queue_card(q, &self.scan, busy));
                col = col.push(Space::with_height(10));
            }
        }
        col = col.push(Space::with_height(14));

        // 아직 등록 안 된 장치
        let unregistered: Vec<&Device> = self
            .scan
            .devices
            .iter()
            .filter(|d| !self.scan.queues.iter().any(|q| q.uri == d.uri))
            .collect();
        if !unregistered.is_empty() {
            col = col.push(text("감지된 장치 (미등록)").size(14).color(C_DIM));
            col = col.push(Space::with_height(8));
            for d in unregistered {
                col = col.push(device_card(d, busy));
                col = col.push(Space::with_height(10));
            }
            col = col.push(Space::with_height(8));
        }

        col = col.push(
            row![
                Space::with_width(Length::Fill),
                action_btn("새로고침", PrinterMsg::Refresh, !busy, C_BTN2),
            ]
        );

        scrollable(container(col).padding([4, 0])).into()
    }
}

// ---------------------------------------------------------------- 진단

/// device-id 의 `CMD:` 목록 = 프린터가 실제로 해석할 수 있는 페이지 기술 언어.
/// 여기에 없는 언어로 데이터를 보내면 백지가 나오거나 깨진 페이지가 쏟아진다.
fn cmd_langs(device_id: &str) -> Vec<String> {
    field(device_id, "CMD")
        .map(|v| v.split(',').map(|s| s.trim().to_uppercase()).collect())
        .unwrap_or_default()
}

/// device-id 에서 `KEY:값;` 한 칸을 꺼낸다. (MFG / MDL / CMD / URF ...)
fn field(device_id: &str, key: &str) -> Option<String> {
    for part in device_id.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix(&format!("{key}:")) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

fn has_postscript(langs: &[String]) -> bool {
    langs.iter().any(|l| {
        l.contains("POSTSCRIPT") || l.contains("BRSCRIPT") || l.contains("BR-SCRIPT")
            || l == "PS" || l.contains("KPDL")
    })
}

fn has_pcl(langs: &[String]) -> bool {
    langs.iter().any(|l| l.contains("PCL") || l.contains("XL"))
}

/// PPD 이름(printer-make-and-model)에서 드라이버가 어떤 언어를 뱉는지 추정한다.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DriverKind { PostScript, Pcl, Vendor }

fn driver_kind(model: &str) -> DriverKind {
    let m = model.to_uppercase();
    if m.contains("BR-SCRIPT") || m.contains("POSTSCRIPT") || m.contains("KPDL") || m.contains("-PS") {
        DriverKind::PostScript
    } else if m.contains("PCL") || m.contains("PXL") || m.contains("LJ") || m.contains("LASERJET") {
        DriverKind::Pcl
    } else {
        DriverKind::Vendor
    }
}

/// 오늘 실제로 겪은 함정들을 규칙으로 굳혀 둔다.
/// - 같은 장치에 큐가 여러 개 (GUI 자동추가 + 수동추가 + 제조사 설치 스크립트)
/// - 프린터가 해석 못 하는 언어의 드라이버 (PostScript 없는 기종에 BR-Script3)
/// - 아예 다른 기종의 PPD
pub fn diagnose(scan: &Scan) -> Vec<Issue> {
    let mut out = Vec::new();

    // 1) 같은 device-uri 에 큐가 둘 이상
    for q in &scan.queues {
        let dups: Vec<&Queue> = scan.queues.iter().filter(|o| o.uri == q.uri).collect();
        if dups.len() > 1 && dups[0].name == q.name {
            let names: Vec<&str> = dups.iter().map(|d| d.name.as_str()).collect();
            out.push(Issue {
                level: Level::Warn,
                title: format!("같은 프린터에 큐가 {}개 등록됨", dups.len()),
                detail: format!(
                    "{} — 모두 같은 장치({})를 가리킵니다. 하나만 남기고 지우세요.",
                    names.join(", "),
                    short_uri(&q.uri)
                ),
                fix_queue: None,
            });
        }
    }

    // 2) 드라이버 언어 ↔ 프린터 지원 언어 불일치
    for q in &scan.queues {
        let Some(dev) = scan.devices.iter().find(|d| d.uri == q.uri) else { continue };
        let langs = cmd_langs(&dev.device_id);
        if langs.is_empty() { continue; }

        match driver_kind(&q.model) {
            DriverKind::PostScript if !has_postscript(&langs) => {
                out.push(Issue {
                    level: Level::Error,
                    title: format!("{}: PostScript 드라이버인데 프린터가 PostScript를 모릅니다", q.name),
                    detail: format!(
                        "드라이버 '{}' 는 PostScript를 보냅니다. 이 프린터가 알리는 언어는 CMD:{} 뿐이라 \
                         백지나 깨진 페이지가 나옵니다. 이 큐를 지우고 기종 전용 드라이버로 다시 등록하세요.",
                        q.model,
                        langs.join(",")
                    ),
                    fix_queue: Some(q.name.clone()),
                });
            }
            DriverKind::Pcl if !has_pcl(&langs) => {
                out.push(Issue {
                    level: Level::Error,
                    title: format!("{}: PCL 드라이버인데 프린터가 PCL을 모릅니다", q.name),
                    detail: format!(
                        "드라이버 '{}' 는 PCL을 보냅니다. 이 프린터가 알리는 언어는 CMD:{} 뿐입니다.",
                        q.model,
                        langs.join(",")
                    ),
                    fix_queue: Some(q.name.clone()),
                });
            }
            _ => {}
        }

        // 3) 다른 기종의 PPD (제네릭은 정상적인 선택지라 제외)
        if let Some(mdl) = field(&dev.device_id, "MDL") {
            let base = mdl.replace(" series", "");
            let m = q.model.to_uppercase();
            if !m.contains("GENERIC")
                && !m.contains(&base.to_uppercase())
                && !base.is_empty()
            {
                out.push(Issue {
                    level: Level::Warn,
                    title: format!("{}: 다른 기종 드라이버일 수 있습니다", q.name),
                    detail: format!(
                        "프린터는 '{}' 인데 드라이버는 '{}' 입니다.",
                        mdl, q.model
                    ),
                    fix_queue: Some(q.name.clone()),
                });
            }
        }
    }

    // 4) 정지된 큐 / 쌓인 작업
    for q in &scan.queues {
        if q.state == 5 {
            out.push(Issue {
                level: Level::Error,
                title: format!("{}: 큐가 정지됨", q.name),
                detail: "작업이 실패해 큐가 멈췄습니다. [작업 취소]를 누르면 대기 작업을 비우고 다시 켭니다."
                    .into(),
                fix_queue: Some(q.name.clone()),
            });
        } else if q.jobs > 0 {
            out.push(Issue {
                level: Level::Info,
                title: format!("{}: 대기 작업 {}건", q.name, q.jobs),
                detail: "인쇄가 안 되고 쌓이기만 한다면 [작업 취소]로 비우고 다시 시도하세요.".into(),
                fix_queue: Some(q.name.clone()),
            });
        }
    }

    // 5) AirPrint 가능한데 USB 로만 물려 있는 경우
    for d in &scan.devices {
        if d.uri.starts_with("usb://") && field(&d.device_id, "URF").is_some() {
            out.push(Issue {
                level: Level::Info,
                title: format!("{}: 랜선을 꽂으면 드라이버 없이 쓸 수 있습니다", d.info),
                detail: "이 프린터는 AirPrint(URF)를 지원합니다. 유선 네트워크에 연결하면 \
                         드라이버 설치 없이 컬러·양면까지 자동으로 잡힙니다. \
                         USB 로는 IPP-over-USB 인터페이스가 없어 드라이버가 필요합니다."
                    .into(),
                fix_queue: None,
            });
        }
    }

    out
}

fn short_uri(uri: &str) -> String {
    match uri.split_once("://") {
        Some((scheme, rest)) => {
            let head: String = rest.chars().take(28).collect();
            format!("{scheme}://{head}{}", if rest.chars().count() > 28 { "..." } else { "" })
        }
        None => uri.to_string(),
    }
}

// ---------------------------------------------------------------- 뷰

fn issue_card<'a>(i: Issue, busy: bool) -> Element<'a, PrinterMsg> {
    let (c, tag) = match i.level {
        Level::Error => (C_ERR, "문제"),
        Level::Warn => (C_WARN, "주의"),
        Level::Info => (C_BLUE, "안내"),
    };

    let mut body = column![
        row![
            text(tag).size(11).color(c),
            Space::with_width(8),
            text(i.title).size(13),
        ]
        .align_y(iced::Alignment::Center),
        Space::with_height(6),
        text(i.detail).size(11).color(C_DIM),
    ];

    if let Some(q) = i.fix_queue {
        body = body.push(Space::with_height(10));
        body = body.push(
            row![
                Space::with_width(Length::Fill),
                action_btn("작업 취소", PrinterMsg::CancelJobs(q.clone()), !busy, C_BTN2),
                Space::with_width(6),
                action_btn("이 큐 삭제", PrinterMsg::Delete(q), !busy, C_ERR),
            ]
            .align_y(iced::Alignment::Center),
        );
    }

    card(body)
}

fn queue_card<'a>(q: &'a Queue, scan: &'a Scan, busy: bool) -> Element<'a, PrinterMsg> {
    let connected = scan.devices.iter().any(|d| d.uri == q.uri);

    let (state_txt, state_col): (&str, Color) = match q.state {
        3 => ("● 대기", C_OK),
        4 => ("● 인쇄 중", C_BLUE),
        5 => ("● 정지됨", C_ERR),
        _ => ("● 알 수 없음", C_DIM),
    };

    let mut head = row![
        container(text(&q.name).size(13)).width(Length::Fill),
        text(state_txt).size(11).color(state_col),
    ]
    .align_y(iced::Alignment::Center);
    if q.is_default {
        head = head.push(Space::with_width(8));
        head = head.push(text("기본").size(10).color(C_BLUE));
    }

    let mut body = column![
        head,
        Space::with_height(6),
        text(format!("드라이버  {}", q.model)).size(11).color(C_DIM),
        text(format!("연결  {}", short_uri(&q.uri))).size(11).color(C_DIM),
    ];

    if !connected {
        body = body.push(Space::with_height(4));
        body = body.push(
            text("장치가 지금 안 보입니다 — 케이블이 빠졌거나 프린터가 꺼져 있습니다.")
                .size(10)
                .color(C_WARN),
        );
    }
    if q.jobs > 0 {
        body = body.push(Space::with_height(4));
        body = body.push(text(format!("대기 작업 {}건", q.jobs)).size(10).color(C_WARN));
    }

    body = body.push(Space::with_height(12));
    let mut actions = row![Space::with_width(Length::Fill)].align_y(iced::Alignment::Center);
    actions = actions.push(action_btn(
        "테스트 인쇄",
        PrinterMsg::TestPrint(q.name.clone()),
        !busy && connected,
        C_GREEN,
    ));
    actions = actions.push(Space::with_width(6));
    if !q.is_default {
        actions = actions.push(action_btn(
            "기본으로",
            PrinterMsg::SetDefault(q.name.clone()),
            !busy,
            C_BLUE,
        ));
        actions = actions.push(Space::with_width(6));
    }
    if q.jobs > 0 {
        actions = actions.push(action_btn(
            "작업 취소",
            PrinterMsg::CancelJobs(q.name.clone()),
            !busy,
            C_WARN,
        ));
        actions = actions.push(Space::with_width(6));
    }
    actions = actions.push(action_btn("삭제", PrinterMsg::Delete(q.name.clone()), !busy, C_ERR));
    body = body.push(actions);

    card(body)
}

fn device_card<'a>(d: &'a Device, busy: bool) -> Element<'a, PrinterMsg> {
    let langs = cmd_langs(&d.device_id);
    let lang_txt = if langs.is_empty() {
        "알 수 없음".to_string()
    } else {
        langs.join(", ")
    };

    let body = column![
        text(&d.info).size(13),
        Space::with_height(6),
        text(format!("연결  {}", short_uri(&d.uri))).size(11).color(C_DIM),
        text(format!("지원 언어  {lang_txt}")).size(11).color(C_DIM),
        Space::with_height(12),
        row![
            Space::with_width(Length::Fill),
            action_btn("등록", PrinterMsg::Register(d.uri.clone()), !busy, C_BLUE),
        ]
        .align_y(iced::Alignment::Center),
    ];

    card(body)
}

// ---------------------------------------------------------------- 실행

fn apply(script: String) -> Task<PrinterMsg> {
    Task::perform(async move { runner::run_sh(&script).await }, PrinterMsg::Applied)
}

/// CUPS 기본 테스트 페이지가 없는 환경도 있어 텍스트로 폴백한다.
fn test_print_script(q: &str) -> String {
    format!(
        "set -e; \
         f=/usr/share/cups/data/testprint; \
         if [ ! -f \"$f\" ]; then \
           f=$(mktemp); \
           printf '\\n\\n    popmgr 테스트 페이지\\n\\n    큐: {q}\\n    시각: %s\\n\\n' \"$(date '+%Y-%m-%d %H:%M:%S')\" > \"$f\"; \
         fi; \
         id=$(lp -d '{q}' \"$f\" 2>&1); \
         echo \"$id\"; \
         echo '종이가 나오지 않으면 진단의 드라이버 경고를 확인하세요.'"
    )
}

/// device-id 를 근거로 드라이버를 고른다.
/// 1순위는 기종 이름이 그대로 들어간 PPD, 없으면 프린터가 알리는 CMD 언어에 맞는 제네릭 PPD.
/// 둘 다 없으면 등록하지 않고 이유를 알린다 — 엉뚱한 드라이버로 등록하는 게 제일 나쁘다.
fn register_script(uri: &str, device_id: &str) -> String {
    format!(
        "set -o pipefail; \
         export LC_ALL=C; \
         uri='{uri}'; did='{device_id}'; \
         mdl=$(printf '%s' \"$did\" | sed -n 's/.*MDL:\\([^;]*\\).*/\\1/p'); \
         [ -z \"$mdl\" ] && mdl='printer'; \
         name=$(printf '%s' \"$mdl\" | tr ' ' '_' | tr -cd 'A-Za-z0-9_.-'); \
         base=$(printf '%s' \"$mdl\" | sed 's/ series$//'); \
         models=$(lpinfo -m 2>/dev/null); \
         m=$(printf '%s\\n' \"$models\" | grep -iF \"$base\" | head -1 | awk '{{print $1}}'); \
         why='기종 전용 드라이버'; \
         if [ -z \"$m\" ]; then \
           case \"$did\" in \
             *POSTSCRIPT*|*BRSCRIPT*|*BR-Script*|*KPDL*) \
               m=$(printf '%s\\n' \"$models\" | grep -i 'Generic PostScript' | head -1 | awk '{{print $1}}'); \
               why='제네릭 PostScript';; \
             *PCLXL*|*PCL6*|*XL2HB*|*XL*) \
               m=$(printf '%s\\n' \"$models\" | grep -i 'Generic PCL 6/PCL XL Printer Foomatic/pxlcolor' | head -1 | awk '{{print $1}}'); \
               why='제네릭 PCL-XL(컬러)';; \
             *PCL*) \
               m=$(printf '%s\\n' \"$models\" | grep -i 'Generic PCL 5c Printer' | head -1 | awk '{{print $1}}'); \
               why='제네릭 PCL 5c';; \
           esac; \
         fi; \
         if [ -z \"$m\" ]; then \
           echo \"이 기종($mdl)에 맞는 드라이버가 시스템에 없습니다.\"; \
           echo '제조사 리눅스 드라이버를 설치한 뒤 다시 시도하세요.'; \
           exit 1; \
         fi; \
         lpadmin -p \"$name\" -E -v \"$uri\" -m \"$m\" -D \"$mdl\" 2>&1 || exit 1; \
         lpoptions -d \"$name\" >/dev/null 2>&1; \
         echo \"등록 완료: $name\"; \
         echo \"드라이버: $why ($m)\"; \
         echo '테스트 인쇄로 실제 출력을 확인하세요.'"
    )
}

async fn scan() -> Scan {
    let r = runner::run_sh(SCAN_SH).await;
    if !r.success && r.output.trim().is_empty() {
        return Scan::default();
    }
    parse_scan(&r.output)
}

/// CUPS 는 JSON 을 내주지 않아 필요한 값만 TSV 로 뽑아 쓴다.
const SCAN_SH: &str = r#"
export LC_ALL=C
default=$(lpstat -d 2>/dev/null | sed -n 's/^system default destination: *//p')
lpstat -a 2>/dev/null | awk '{print $1}' | while read -r q; do
  uri=$(lpstat -v "$q" 2>/dev/null | sed -n 's/^device for [^:]*: *//p')
  opts=$(lpoptions -p "$q" 2>/dev/null)
  mm=$(printf '%s' "$opts" | sed -n "s/.*printer-make-and-model='\([^']*\)'.*/\1/p")
  [ -z "$mm" ] && mm=$(printf '%s' "$opts" | tr ' ' '\n' | sed -n 's/^printer-make-and-model=//p')
  st=$(printf '%s' "$opts" | tr ' ' '\n' | sed -n 's/^printer-state=//p')
  nj=$(lpstat -o "$q" 2>/dev/null | grep -c .)
  d=0; [ "$q" = "$default" ] && d=1
  printf 'Q\t%s\t%s\t%s\t%s\t%s\t%s\n' "$q" "$uri" "$mm" "$st" "$d" "$nj"
done
lpinfo -l -v 2>/dev/null | awk '
  /^Device: uri = / { uri=substr($0, index($0,"= ")+2) }
  / info = /        { info=substr($0, index($0,"= ")+2) }
  / device-id = /   { did=substr($0, index($0,"= ")+2);
                      if (uri ~ /:\/\//) printf "D\t%s\t%s\t%s\n", uri, info, did }
'
"#;

fn parse_scan(out: &str) -> Scan {
    let mut scan = Scan::default();
    for line in out.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first() {
            Some(&"Q") if f.len() >= 7 => scan.queues.push(Queue {
                name: f[1].to_string(),
                uri: f[2].to_string(),
                model: f[3].to_string(),
                state: f[4].parse().unwrap_or(0),
                is_default: f[5] == "1",
                jobs: f[6].trim().parse().unwrap_or(0),
            }),
            Some(&"D") if f.len() >= 4 => scan.devices.push(Device {
                uri: f[1].to_string(),
                info: f[2].to_string(),
                device_id: f[3].to_string(),
            }),
            _ => {}
        }
    }
    scan
}

#[cfg(test)]
mod tests {
    use super::*;

    const BROTHER: &str = "MFG:Brother;CMD:PJL,XL2HB,URF;MDL:HL-3150CDN series;CLS:PRINTER;\
                           CID:Brother Color Type4;URF:SRGB24,W8,CP1,RS600,DM1;";

    fn dev() -> Device {
        Device {
            uri: "usb://Brother/HL-3150CDN%20series?serial=E7".into(),
            info: "Brother HL-3150CDN series".into(),
            device_id: BROTHER.into(),
        }
    }

    fn queue(name: &str, model: &str) -> Queue {
        Queue {
            name: name.into(),
            uri: "usb://Brother/HL-3150CDN%20series?serial=E7".into(),
            model: model.into(),
            state: 3,
            is_default: false,
            jobs: 0,
        }
    }

    #[test]
    fn parses_device_id_fields() {
        assert_eq!(field(BROTHER, "MDL").as_deref(), Some("HL-3150CDN series"));
        assert_eq!(field(BROTHER, "MFG").as_deref(), Some("Brother"));
        assert!(field(BROTHER, "URF").is_some());
        assert_eq!(field(BROTHER, "NOPE"), None);
        assert_eq!(cmd_langs(BROTHER), vec!["PJL", "XL2HB", "URF"]);
    }

    #[test]
    fn brother_hl3150cdn_has_no_postscript() {
        let langs = cmd_langs(BROTHER);
        assert!(!has_postscript(&langs));
        assert!(has_pcl(&langs)); // XL2HB
    }

    #[test]
    fn flags_postscript_driver_on_non_postscript_printer() {
        // 실제 사례: GNOME 이 HL-3150CDN 에 HL-4050CDN BR-Script3 PPD 를 붙였다.
        let scan = Scan {
            queues: vec![queue("Brother-HL-3150CDN-series", "Brother HL-4050CDN BR-Script3")],
            devices: vec![dev()],
        };
        let issues = diagnose(&scan);
        assert!(issues.iter().any(|i| i.level == Level::Error
            && i.title.contains("PostScript")
            && i.fix_queue.as_deref() == Some("Brother-HL-3150CDN-series")));
    }

    #[test]
    fn accepts_matching_vendor_driver() {
        let scan = Scan {
            queues: vec![queue("HL3150CDN", "Brother HL-3150CDN series CUPS")],
            devices: vec![dev()],
        };
        let issues = diagnose(&scan);
        assert!(!issues.iter().any(|i| i.level == Level::Error));
    }

    #[test]
    fn generic_pcl_driver_is_not_a_model_mismatch() {
        let scan = Scan {
            queues: vec![queue("q", "Generic PCL 6/PCL XL Printer Foomatic/pxlcolor")],
            devices: vec![dev()],
        };
        let issues = diagnose(&scan);
        assert!(!issues.iter().any(|i| i.title.contains("다른 기종")));
    }

    #[test]
    fn flags_duplicate_queues_once() {
        let scan = Scan {
            queues: vec![
                queue("a", "Brother HL-3150CDN series CUPS"),
                queue("b", "Brother HL-3150CDN series CUPS"),
            ],
            devices: vec![dev()],
        };
        let dups = diagnose(&scan)
            .into_iter()
            .filter(|i| i.title.contains("큐가"))
            .count();
        assert_eq!(dups, 1);
    }

    #[test]
    fn suggests_ethernet_for_airprint_over_usb() {
        let scan = Scan { queues: vec![], devices: vec![dev()] };
        assert!(diagnose(&scan).iter().any(|i| i.title.contains("랜선")));
    }

    /// 2026-08-21 실제 고장 상태 재현: HL-3150CDN 하나에 큐가 3개 붙어 있었고
    /// 그중 하나는 GNOME 이 자동으로 고른 HL-4050CDN BR-Script3(PostScript) 였다.
    /// scan.sh 가 그때 뱉은 TSV 를 그대로 고정해 둔다.
    #[test]
    fn real_world_three_queue_breakage() {
        const URI: &str = "usb://Brother/HL-3150CDN%20series?serial=E71876G7J291777";
        let out = format!(
            "Q\tBrother-HL-3150CDN-series\t{URI}\tBrother HL-4050CDN BR-Script3\t3\t0\t0\n\
             Q\tBrother_HL-3150CDN\t{URI}\tBrother HL-3150CDN series CUPS\t3\t1\t0\n\
             Q\tHL3150CDN\t{URI}\tBrother HL-3150CDN series CUPS\t3\t0\t0\n\
             D\t{URI}\tBrother HL-3150CDN series\t{BROTHER}\n"
        );
        let scan = parse_scan(&out);
        assert_eq!(scan.queues.len(), 3);
        assert_eq!(scan.devices.len(), 1);

        let issues = diagnose(&scan);

        // 중복 경고는 URI 당 한 번만
        assert_eq!(issues.iter().filter(|i| i.title.contains("큐가 3개")).count(), 1);

        // PostScript 오적용은 그 큐 하나만 지목해야 한다
        let ps: Vec<&Issue> = issues
            .iter()
            .filter(|i| i.level == Level::Error && i.title.contains("PostScript"))
            .collect();
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].fix_queue.as_deref(), Some("Brother-HL-3150CDN-series"));

        // 제조사 정품 드라이버를 쓰는 두 큐는 오류로 잡히면 안 된다
        for q in ["Brother_HL-3150CDN", "HL3150CDN"] {
            assert!(!issues
                .iter()
                .any(|i| i.level == Level::Error && i.fix_queue.as_deref() == Some(q)));
        }
    }

    #[test]
    fn parses_scan_tsv() {
        let out = "Q\tp1\tusb://x\tGeneric PCL\t3\t1\t2\n\
                   D\tusb://x\tSome Printer\tMFG:A;MDL:B;CMD:PCL;\n";
        let s = parse_scan(out);
        assert_eq!(s.queues.len(), 1);
        assert_eq!(s.queues[0].name, "p1");
        assert_eq!(s.queues[0].jobs, 2);
        assert!(s.queues[0].is_default);
        assert_eq!(s.devices.len(), 1);
        assert_eq!(s.devices[0].info, "Some Printer");
    }
}
