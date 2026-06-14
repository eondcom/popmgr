mod runner;
mod ui;

use iced::{
    widget::{button, column, container, row, scrollable, text, Space},
    Color, Element, Length, Subscription, Task,
};
use runner::CmdResult;
use ui::{
    apps::{AppsMsg, AppsState},
    audio::{AudioMsg, AudioState},
    cosmic_tweaks::{CosmicMsg, CosmicState},
    disk::{DiskMsg, DiskState},
    ime::{ImeMsg, ImeState},
    usb::{UsbMsg, UsbState},
};
use ui::ime::{C_BG, C_BLUE, C_BORDER, C_DIM, C_OK, C_ERR, C_SURFACE, C_TEXT};

/// 두 색을 비율 t(0~1)로 섞기.
fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: 1.0 }
}

/// 앱 전역 Toss 라이트 테마. 윈도우 배경·슬라이더·스크롤바·입력창 기본색을 결정.
fn app_theme() -> iced::Theme {
    iced::Theme::custom(
        "Toss Light".to_string(),
        iced::theme::Palette {
            background: C_BG,
            text: C_TEXT,
            primary: C_BLUE,
            success: C_OK,
            danger: C_ERR,
        },
    )
}

#[derive(Debug, Clone, PartialEq)]
enum Tab { Ime, Usb, Audio, Disk, Cosmic, Apps }

#[derive(Debug, Clone)]
enum Message {
    TabSelect(Tab),
    Ime(ImeMsg),
    Usb(UsbMsg),
    Audio(AudioMsg),
    Disk(DiskMsg),
    Cosmic(CosmicMsg),
    Apps(AppsMsg),
    CopyLog,
    DrainStreamLog,
}

struct App {
    tab: Tab,
    ime: ImeState,
    usb: UsbState,
    audio: AudioState,
    disk: DiskState,
    cosmic: CosmicState,
    apps: AppsState,
    output: String,
}

fn main() -> iced::Result {
    // 루트 헬퍼 모드: pkexec popmgr --apply-vref /dev/snd/hwC0D0 0x1a 0x24
    // (HDA hwdep은 CAP_SYS_RAWIO 필요 — 잭 마이크 바이어스 전원 제어용)
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("--apply-vref") {
        std::process::exit(apply_vref_cli(&args));
    }

    if let Err(msg) = acquire_single_instance_lock() {
        eprintln!("{msg}");
        std::process::exit(0);
    }

    let icon = iced::window::icon::from_rgba(
        include_bytes!("../assets/icon.rgba").to_vec(), 128, 128,
    ).ok();

    iced::application("popmgr", update, view)
        .theme(|_| app_theme())
        .font(include_bytes!("../assets/NanumGothic.ttf"))
        .font(include_bytes!("../assets/NanumSquareR.ttf"))
        .font(include_bytes!("../assets/DejaVuSans.ttf")) // ✓ ✗ ⚠ █ ░ 등 기호 폴백
        .default_font(iced::Font::with_name("NanumSquare"))
        .subscription(subscription)
        .window(iced::window::Settings {
            size: iced::Size::new(780.0, 680.0),
            icon,
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: "com.eondcom.Popmgr".into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .run_with(init)
}

fn acquire_single_instance_lock() -> Result<(), String> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;

    let dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/tmp/popmgr-{}", users_uid()));
    let _ = std::fs::create_dir_all(&dir);
    let path = format!("{dir}/popmgr.lock");

    let file = std::fs::OpenOptions::new()
        .read(true).write(true).create(true).mode(0o600)
        .open(&path)
        .map_err(|e| format!("lock 파일 열기 실패: {e}"))?;

    // LOCK_EX | LOCK_NB
    let rc = unsafe { libc_flock(file.as_raw_fd(), 2 | 4) };
    if rc != 0 {
        let other_pid = std::fs::read_to_string(&path).unwrap_or_default();
        return Err(format!(
            "popmgr가 이미 실행 중입니다 (pid: {}). 종료합니다.",
            other_pid.trim()
        ));
    }

    // 잠금 유지를 위해 파일 핸들을 leak — 프로세스 종료 시 OS가 정리
    use std::io::{Seek, SeekFrom, Write};
    let mut f = file;
    let _ = f.set_len(0);
    let _ = f.seek(SeekFrom::Start(0));
    let _ = writeln!(f, "{}", std::process::id());
    let _ = f.flush();
    std::mem::forget(f);
    Ok(())
}

fn users_uid() -> u32 {
    unsafe { libc_getuid() }
}

unsafe extern "C" {
    #[link_name = "flock"]
    fn libc_flock(fd: i32, op: i32) -> i32;
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
    #[link_name = "ioctl"]
    fn libc_ioctl(fd: i32, req: u64, arg: *mut core::ffi::c_void) -> i32;
}

fn apply_vref_cli(args: &[String]) -> i32 {
    use std::os::fd::AsRawFd;

    const HDA_IOCTL_VERB_WRITE: u64 = 0xC008_4811;
    const SET_PIN_WIDGET_CONTROL: u32 = 0x707;
    const GET_PIN_WIDGET_CONTROL: u32 = 0xF07;
    #[repr(C)]
    struct HdaVerb { verb: u32, res: u32 }

    if args.len() != 5 {
        eprintln!("사용법: popmgr --apply-vref /dev/snd/hwC0D0 0x1a 0x24");
        return 2;
    }
    let dev = &args[2];
    if !dev.starts_with("/dev/snd/hwC") {
        eprintln!("잘못된 장치 경로: {dev}");
        return 2;
    }
    let hex = |s: &str| u32::from_str_radix(s.trim_start_matches("0x"), 16);
    let (nid, val) = match (hex(&args[3]), hex(&args[4])) {
        (Ok(n), Ok(v)) => (n, v),
        _ => { eprintln!("16진수 파싱 실패: {} {}", args[3], args[4]); return 2; }
    };

    let f = match std::fs::OpenOptions::new().read(true).write(true).open(dev) {
        Ok(f) => f,
        Err(e) => { eprintln!("{dev} 열기 실패: {e}"); return 1; }
    };
    let run = |verb: u32, param: u32| -> Result<u32, i32> {
        let mut v = HdaVerb { verb: (nid << 24) | (verb << 8) | param, res: 0 };
        let rc = unsafe {
            libc_ioctl(f.as_raw_fd(), HDA_IOCTL_VERB_WRITE, &mut v as *mut _ as *mut core::ffi::c_void)
        };
        if rc < 0 { Err(rc) } else { Ok(v.res) }
    };

    let before = run(GET_PIN_WIDGET_CONTROL, 0).unwrap_or(0xFFFF);
    if run(SET_PIN_WIDGET_CONTROL, val).is_err() {
        eprintln!("verb 전송 실패 (ioctl)");
        return 1;
    }
    let after = run(GET_PIN_WIDGET_CONTROL, 0).unwrap_or(0xFFFF);
    println!("핀 0x{nid:x} pin-ctl: 0x{before:x} -> 0x{after:x}");
    if after == val { 0 } else { 1 }
}

fn init() -> (App, Task<Message>) {
    let app = App {
        tab: Tab::Ime,
        ime: ImeState::new(),
        usb: UsbState::new(),
        audio: AudioState::new(),
        disk: DiskState::new(),
        cosmic: CosmicState::new(),
        apps: AppsState::new(),
        output: String::new(),
    };
    let task = Task::batch([
        Task::perform(async { () }, |_| Message::Ime(ImeMsg::Refresh)),
        Task::perform(async { () }, |_| Message::Usb(UsbMsg::Refresh)),
        Task::perform(async { () }, |_| Message::Audio(AudioMsg::Refresh)),
        Task::perform(async { () }, |_| Message::Disk(DiskMsg::Refresh)),
        Task::perform(async { () }, |_| Message::Cosmic(CosmicMsg::Refresh)),
        Task::perform(async { () }, |_| Message::Apps(AppsMsg::Refresh)),
    ]);
    (app, task)
}

fn subscription(app: &App) -> Subscription<Message> {
    // 배포/빌드 중 실시간 출력 폴링
    let drain = iced::time::every(std::time::Duration::from_millis(400))
        .map(|_| Message::DrainStreamLog);

    // IME 워치독: popmgr가 떠 있는 동안 한글 입력기 데몬이 죽으면 자동 재기동
    let ime_watch = iced::time::every(std::time::Duration::from_secs(30))
        .map(|_| Message::Ime(ImeMsg::Watchdog));

    // 오디오/디스크 탭: 장치 상태 자동 새로고침 (꽂으면 바로 반영)
    match app.tab {
        Tab::Audio => {
            let audio = iced::time::every(std::time::Duration::from_secs(2))
                .map(|_| Message::Audio(AudioMsg::Refresh));
            Subscription::batch([drain, ime_watch, audio])
        }
        Tab::Disk => {
            let disk = iced::time::every(std::time::Duration::from_secs(2))
                .map(|_| Message::Disk(DiskMsg::Refresh));
            Subscription::batch([drain, ime_watch, disk])
        }
        // 다른 탭에서도 느린 주기로 오디오 감시 — 고정 설정 자동 복원이 항상 동작하도록
        _ => {
            let audio_slow = iced::time::every(std::time::Duration::from_secs(5))
                .map(|_| Message::Audio(AudioMsg::Refresh));
            Subscription::batch([drain, ime_watch, audio_slow])
        }
    }
}

fn update(app: &mut App, msg: Message) -> Task<Message> {
    match msg {
        Message::TabSelect(t) => { app.tab = t; Task::none() }
        Message::Ime(m) => {
            let (task, res) = app.ime.update(m);
            if let Some(r) = res { push_log(&mut app.output, r); }
            task.map(Message::Ime)
        }
        Message::Usb(m) => {
            let (task, res) = app.usb.update(m);
            if let Some(r) = res { push_log(&mut app.output, r); }
            task.map(Message::Usb)
        }
        Message::Audio(m) => {
            let (task, res) = app.audio.update(m);
            if let Some(r) = res { push_log(&mut app.output, r); }
            task.map(Message::Audio)
        }
        Message::Disk(m) => {
            let (task, res) = app.disk.update(m);
            if let Some(r) = res { push_log(&mut app.output, r); }
            task.map(Message::Disk)
        }
        Message::Cosmic(m) => {
            let (task, res) = app.cosmic.update(m);
            if let Some(r) = res { push_log(&mut app.output, r); }
            task.map(Message::Cosmic)
        }
        Message::Apps(m) => {
            let (task, res) = app.apps.update(m);
            if let Some(r) = res { push_log(&mut app.output, r); }
            task.map(Message::Apps)
        }
        Message::CopyLog => iced::clipboard::write(app.output.clone()),
        Message::DrainStreamLog => {
            let lines = runner::stream_drain();
            if !lines.is_empty() {
                for line in &lines {
                    app.output.push_str(line);
                    app.output.push('\n');
                }
                trim_log(&mut app.output, 400);
            }
            Task::none()
        }
    }
}

fn push_log(out: &mut String, r: CmdResult) {
    let prefix = if r.success { "[OK]" } else { "[ERR]" };
    out.push_str(&format!("{prefix} {}\n", r.output.trim()));
    trim_log(out, 300);
}

fn trim_log(out: &mut String, max: usize) {
    let lines: Vec<&str> = out.lines().collect();
    if lines.len() > max {
        *out = lines[lines.len() - max..].join("\n") + "\n";
    }
}

fn view(app: &App) -> Element<'_, Message> {
    let sidebar = sidebar_view(app);

    let content: Element<'_, Message> = match &app.tab {
        Tab::Ime    => app.ime.view().map(Message::Ime),
        Tab::Usb    => app.usb.view().map(Message::Usb),
        Tab::Audio  => app.audio.view().map(Message::Audio),
        Tab::Disk   => app.disk.view().map(Message::Disk),
        Tab::Cosmic => app.cosmic.view().map(Message::Cosmic),
        Tab::Apps   => app.apps.view().map(Message::Apps),
    };

    let log_panel = log_panel_view(&app.output);

    let right = column![
        scrollable(
            container(content).width(Length::Fill).padding([24, 28])
        ).height(Length::Fill),
        log_panel,
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    row![sidebar, right].height(Length::Fill).into()
}

fn sidebar_view(app: &App) -> Element<'_, Message> {
    let tabs: &[(Tab, &str, &str)] = &[
        (Tab::Ime,    "IME",        "한글 입력기"),
        (Tab::Usb,    "USB",        "USB / 트랙볼"),
        (Tab::Audio,  "오디오",     "입출력 / 마이크"),
        (Tab::Disk,   "디스크",     "외장하드 / 마운트"),
        (Tab::Cosmic, "COSMIC",     "COSMIC 트윅"),
        (Tab::Apps,   "앱 관리",    "설치 / 제거"),
    ];

    let logo = container(
        column![
            text("popmgr").size(22).color(C_TEXT),
            Space::with_height(2),
            text("Pop!_OS 관리 도구").size(10).color(C_DIM),
        ]
    )
    .padding(iced::Padding { top: 22.0, right: 18.0, bottom: 18.0, left: 18.0 });

    let mut col = column![logo].spacing(4);

    for (tab, label, hint) in tabs {
        let active = &app.tab == tab;
        // 활성: 연한 블루 배경 + 블루 텍스트(토스 사이드 내비 방식)
        let active_bg = mix(C_SURFACE, C_BLUE, 0.10);
        let bg = if active { active_bg } else { C_SURFACE };
        let tc = if active { C_BLUE } else { Color::from_rgb(0.30, 0.34, 0.40) };
        let hc = if active { mix(C_BLUE, C_TEXT, 0.35) } else { C_DIM };

        let btn = button(
            column![
                text(*label).size(14).color(tc),
                text(*hint).size(10).color(hc),
            ]
            .spacing(2)
        )
        .width(Length::Fill)
        .padding([11, 16])
        .on_press(Message::TabSelect(tab.clone()))
        .style(move |_, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered if !active => C_BG,
                _ => bg,
            };
            iced::widget::button::Style {
                background: Some(iced::Background::Color(bg)),
                border: iced::Border { radius: 12.0.into(), ..Default::default() },
                text_color: tc,
                ..Default::default()
            }
        });

        col = col.push(
            container(btn).padding(iced::Padding { top: 0.0, right: 12.0, bottom: 0.0, left: 12.0 })
        );
    }

    container(col)
        .width(190)
        .height(Length::Fill)
        .style(|_| iced::widget::container::Style {
            background: Some(iced::Background::Color(C_SURFACE)),
            border: iced::Border {
                color: C_BORDER,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn log_panel_view(output: &str) -> Element<'_, Message> {
    let log_txt = if output.is_empty() {
        "작업 결과가 여기에 표시됩니다."
    } else {
        output
    };
    let log_col = if output.is_empty() {
        C_DIM
    } else {
        C_TEXT
    };

    let copy_btn = button(text("복사").size(11).color(C_BLUE))
        .on_press(Message::CopyLog)
        .padding([4, 12])
        .style(|_, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.06),
                _ => Color::from_rgba(1.0, 1.0, 1.0, 0.03),
            };
            iced::widget::button::Style {
                background: Some(iced::Background::Color(bg)),
                border: iced::Border { radius: 8.0.into(), color: C_BORDER, width: 1.0 },
                text_color: C_BLUE,
                ..Default::default()
            }
        });

    let header = row![
        text("로그").size(11).color(C_DIM),
        Space::with_width(Length::Fill),
        copy_btn,
    ]
    .align_y(iced::Alignment::Center);

    container(
        column![
            container(header).padding(iced::Padding { top: 4.0, right: 10.0, bottom: 2.0, left: 10.0 }),
            scrollable(
                container(text(log_txt).size(12).color(log_col))
                    .padding(iced::Padding { top: 0.0, right: 10.0, bottom: 6.0, left: 10.0 })
                    .width(Length::Fill)
            ).height(120),
        ]
    )
    .width(Length::Fill)
    .style(|_| iced::widget::container::Style {
        background: Some(iced::Background::Color(C_SURFACE)),
        border: iced::Border { color: C_BORDER, width: 1.0, radius: 0.0.into() },
        ..Default::default()
    })
    .into()
}
