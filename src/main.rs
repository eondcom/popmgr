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
        .theme(|_| iced::Theme::Dark)
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

    // 오디오/디스크 탭: 장치 상태 자동 새로고침 (꽂으면 바로 반영)
    match app.tab {
        Tab::Audio => {
            let audio = iced::time::every(std::time::Duration::from_secs(2))
                .map(|_| Message::Audio(AudioMsg::Refresh));
            Subscription::batch([drain, audio])
        }
        Tab::Disk => {
            let disk = iced::time::every(std::time::Duration::from_secs(2))
                .map(|_| Message::Disk(DiskMsg::Refresh));
            Subscription::batch([drain, disk])
        }
        _ => drain,
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
            container(content).width(Length::Fill).padding([20, 24])
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
        text("popmgr")
            .size(18)
            .color(Color::from_rgb(0.35, 0.65, 1.0))
    )
    .padding(iced::Padding { top: 18.0, right: 16.0, bottom: 6.0, left: 16.0 });

    let sub = container(
        text("Pop!_OS 관리 도구")
            .size(10)
            .color(Color::from_rgb(0.4, 0.4, 0.5))
    )
    .padding(iced::Padding { top: 0.0, right: 16.0, bottom: 16.0, left: 16.0 });

    let mut col = column![logo, sub];

    for (tab, label, hint) in tabs {
        let active = &app.tab == tab;
        let bg = if active { Color::from_rgb(0.1, 0.25, 0.5) } else { Color::from_rgba(0.0, 0.0, 0.0, 0.0) };
        let tc = if active { Color::WHITE } else { Color::from_rgb(0.65, 0.65, 0.72) };

        let btn = button(
            column![
                text(*label).size(13).color(tc),
                text(*hint).size(10).color(if active { Color::from_rgb(0.7, 0.85, 1.0) } else { Color::from_rgb(0.4, 0.4, 0.45) }),
            ]
            .spacing(1)
        )
        .width(Length::Fill)
        .padding([8, 14])
        .on_press(Message::TabSelect(tab.clone()))
        .style(move |_, _| iced::widget::button::Style {
            background: Some(iced::Background::Color(bg)),
            border: iced::Border { radius: 6.0.into(), ..Default::default() },
            text_color: tc,
            ..Default::default()
        });

        col = col.push(
            container(btn).padding(iced::Padding { top: 0.0, right: 8.0, bottom: 2.0, left: 8.0 })
        );
    }

    container(col)
        .width(150)
        .height(Length::Fill)
        .style(|_| iced::widget::container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.07, 0.07, 0.09))),
            border: iced::Border {
                color: Color::from_rgb(0.15, 0.15, 0.2),
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
        Color::from_rgb(0.35, 0.35, 0.4)
    } else {
        Color::from_rgb(0.5, 0.8, 0.5)
    };

    let copy_btn = button(text("복사").size(11).color(Color::from_rgb(0.5, 0.7, 0.5)))
        .on_press(Message::CopyLog)
        .padding([3, 8])
        .style(|_, _| iced::widget::button::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.06, 0.12, 0.06))),
            border: iced::Border { radius: 4.0.into(), color: Color::from_rgb(0.18, 0.3, 0.18), width: 1.0 },
            text_color: Color::from_rgb(0.5, 0.7, 0.5),
            ..Default::default()
        });

    let header = row![
        text("로그").size(11).color(Color::from_rgb(0.35, 0.5, 0.35)),
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
        background: Some(iced::Background::Color(Color::from_rgb(0.04, 0.07, 0.04))),
        border: iced::Border { color: Color::from_rgb(0.12, 0.2, 0.12), width: 1.0, radius: 0.0.into() },
        ..Default::default()
    })
    .into()
}
