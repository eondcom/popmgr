use iced::{
    widget::{column, container, pick_list, row, scrollable, slider, text, Space},
    Color, Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_DIM, C_ERR, C_OK, C_WARN};

const TEST_WAV: &str = "/tmp/popmgr-mictest.wav";

#[derive(Debug, Clone, PartialEq)]
pub struct PortInfo {
    pub name: String,
    pub desc: String,
    /// Some(true)=연결됨, Some(false)=연결 안 됨, None=알 수 없음
    pub available: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub name: String,
    pub desc: String,
    pub ports: Vec<PortInfo>,
    pub active_port: Option<String>,
    pub volume_pct: Option<u32>,
    pub muted: bool,
}

#[derive(Debug, Clone)]
pub struct AudioScan {
    pub sinks: Vec<DeviceInfo>,
    pub sources: Vec<DeviceInfo>,
    pub default_sink: Option<String>,
    pub default_source: Option<String>,
    /// ALSA 잭 감지 상태 (이름, 꽂힘 여부) — PipeWire 포트 availability가
    /// "unknown"인 콤보잭에서도 물리 연결을 보여주기 위함
    pub jacks: Vec<(String, bool)>,
}

/// pick_list 항목 — id는 pactl 이름, label은 표시용
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

#[derive(Debug, Clone)]
pub enum MicTest {
    None,
    Recording,
    Ok { peak_pct: f64, rms_pct: f64, samples: usize },
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum AudioMsg {
    Refresh,
    Refreshed(AudioScan),
    PickSink(Choice),
    PickSinkPort(Choice),
    PickSource(Choice),
    PickSourcePort(Choice),
    SinkVol(u32),
    SinkVolCommit,
    SourceVol(u32),
    SourceVolCommit,
    ToggleSinkMute,
    ToggleSourceMute,
    Applied(CmdResult),
    TestMic,
    TestDone(MicTest),
    Play,
    Done(CmdResult),
}

pub struct AudioState {
    pub sinks: Vec<DeviceInfo>,
    pub sources: Vec<DeviceInfo>,
    pub default_sink: Option<String>,
    pub default_source: Option<String>,
    pub jacks: Vec<(String, bool)>,
    pub scanned: bool,
    pub sink_vol: u32,
    pub source_vol: u32,
    sink_dragging: bool,
    source_dragging: bool,
    pub running: Option<String>,
    pub last_test: MicTest,
}

impl AudioState {
    pub fn new() -> Self {
        Self {
            sinks: Vec::new(),
            sources: Vec::new(),
            default_sink: None,
            default_source: None,
            jacks: Vec::new(),
            scanned: false,
            sink_vol: 0,
            source_vol: 0,
            sink_dragging: false,
            source_dragging: false,
            running: None,
            last_test: MicTest::None,
        }
    }

    fn default_sink_dev(&self) -> Option<&DeviceInfo> {
        let name = self.default_sink.as_deref()?;
        self.sinks.iter().find(|d| d.name == name)
    }

    fn default_source_dev(&self) -> Option<&DeviceInfo> {
        let name = self.default_source.as_deref()?;
        self.sources.iter().find(|d| d.name == name)
    }

    pub fn update(&mut self, msg: AudioMsg) -> (Task<AudioMsg>, Option<CmdResult>) {
        match msg {
            AudioMsg::Refresh => {
                let t = Task::perform(async { scan_audio().await }, AudioMsg::Refreshed);
                (t, None)
            }
            AudioMsg::Refreshed(s) => {
                self.sinks = s.sinks;
                self.sources = s.sources;
                self.default_sink = s.default_sink;
                self.default_source = s.default_source;
                self.jacks = s.jacks;
                self.scanned = true;
                // 슬라이더 드래그 중에는 자동 새로고침이 값을 덮어쓰지 않도록
                if !self.sink_dragging {
                    self.sink_vol = self.default_sink_dev().and_then(|d| d.volume_pct).unwrap_or(0);
                }
                if !self.source_dragging {
                    self.source_vol = self.default_source_dev().and_then(|d| d.volume_pct).unwrap_or(0);
                }
                (Task::none(), None)
            }
            AudioMsg::PickSink(c) => {
                // 기본 출력 변경 + 재생 중인 스트림도 함께 이동
                let script = format!(
                    "pactl set-default-sink '{id}' && \
                     pactl list short sink-inputs | cut -f1 | while read -r i; do \
                       pactl move-sink-input \"$i\" '{id}' 2>/dev/null || true; done",
                    id = c.id
                );
                (apply(script, format!("기본 출력 장치 → {}", c.label)), None)
            }
            AudioMsg::PickSource(c) => {
                let script = format!(
                    "pactl set-default-source '{id}' && \
                     pactl list short source-outputs | cut -f1 | while read -r i; do \
                       pactl move-source-output \"$i\" '{id}' 2>/dev/null || true; done",
                    id = c.id
                );
                (apply(script, format!("기본 입력 장치 → {}", c.label)), None)
            }
            AudioMsg::PickSinkPort(c) => {
                let Some(sink) = self.default_sink.clone() else { return (Task::none(), None) };
                let script = format!("pactl set-sink-port '{sink}' '{}'", c.id);
                (apply(script, format!("출력 포트 → {}", c.label)), None)
            }
            AudioMsg::PickSourcePort(c) => {
                let Some(src) = self.default_source.clone() else { return (Task::none(), None) };
                let script = format!("pactl set-source-port '{src}' '{}'", c.id);
                (apply(script, format!("입력 포트 → {}", c.label)), None)
            }
            AudioMsg::SinkVol(v) => { self.sink_vol = v; self.sink_dragging = true; (Task::none(), None) }
            AudioMsg::SourceVol(v) => { self.source_vol = v; self.source_dragging = true; (Task::none(), None) }
            AudioMsg::SinkVolCommit => {
                self.sink_dragging = false;
                let Some(sink) = self.default_sink.clone() else { return (Task::none(), None) };
                let v = self.sink_vol;
                let script = format!("pactl set-sink-volume '{sink}' {v}%");
                (apply(script, format!("출력 볼륨 → {v}%")), None)
            }
            AudioMsg::SourceVolCommit => {
                self.source_dragging = false;
                let Some(src) = self.default_source.clone() else { return (Task::none(), None) };
                let v = self.source_vol;
                let script = format!("pactl set-source-volume '{src}' {v}%");
                (apply(script, format!("입력 볼륨 → {v}%")), None)
            }
            AudioMsg::ToggleSinkMute => {
                let Some(sink) = self.default_sink.clone() else { return (Task::none(), None) };
                let muted = self.default_sink_dev().map(|d| d.muted).unwrap_or(false);
                let script = format!("pactl set-sink-mute '{sink}' toggle");
                let label = if muted { "출력 음소거 해제" } else { "출력 음소거" };
                (apply(script, label.into()), None)
            }
            AudioMsg::ToggleSourceMute => {
                let Some(src) = self.default_source.clone() else { return (Task::none(), None) };
                let muted = self.default_source_dev().map(|d| d.muted).unwrap_or(false);
                let script = format!("pactl set-source-mute '{src}' toggle");
                let label = if muted { "입력 음소거 해제" } else { "입력 음소거" };
                (apply(script, label.into()), None)
            }
            AudioMsg::Applied(r) => {
                // 설정 반영 후 상태 재스캔
                let t = Task::perform(async { scan_audio().await }, AudioMsg::Refreshed);
                (t, Some(r))
            }
            AudioMsg::TestMic => {
                self.running = Some("녹음 중 (2초)...".into());
                self.last_test = MicTest::Recording;
                let t = Task::perform(async { test_mic().await }, AudioMsg::TestDone);
                (t, None)
            }
            AudioMsg::TestDone(r) => {
                self.running = None;
                self.last_test = r.clone();
                let result = match &r {
                    MicTest::Ok { peak_pct, rms_pct, samples } => CmdResult {
                        success: *peak_pct > 1.0,
                        output: format!(
                            "마이크 테스트: peak {:.1}%, RMS {:.2}%, 샘플 {}",
                            peak_pct, rms_pct, samples
                        ),
                    },
                    MicTest::Failed(e) => CmdResult { success: false, output: format!("마이크 테스트 실패: {e}") },
                    _ => CmdResult { success: false, output: "테스트 미완료".into() },
                };
                (Task::none(), Some(result))
            }
            AudioMsg::Play => {
                self.running = Some("재생 중...".into());
                let script = format!("aplay {TEST_WAV} 2>&1");
                let t = Task::perform(async move { runner::run_sh(&script).await }, AudioMsg::Done);
                (t, None)
            }
            AudioMsg::Done(r) => {
                self.running = None;
                (Task::none(), Some(r))
            }
        }
    }

    pub fn view(&self) -> Element<'_, AudioMsg> {
        let is_running = self.running.is_some();

        let mut col = column![
            text("오디오").size(20),
            Space::with_height(6),
            text("입력/출력 장치와 포트(스피커·헤드폰·핀마이크 등)를 선택하고 마이크를 검증합니다.")
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

        // 출력 장치 카드
        col = col.push(device_card(
            "출력 (스피커 / 헤드폰)",
            &self.sinks,
            self.default_sink_dev(),
            self.sink_vol,
            100,
            AudioMsg::PickSink,
            AudioMsg::PickSinkPort,
            AudioMsg::SinkVol,
            AudioMsg::SinkVolCommit,
            AudioMsg::ToggleSinkMute,
            None,
        ));
        col = col.push(Space::with_height(12));

        // 잭 감지 카드 (ALSA 레벨 — PipeWire가 못 알려주는 물리 연결 상태)
        col = col.push(jack_card(&self.jacks));
        col = col.push(Space::with_height(12));

        // 입력 장치 카드
        let mic_plugged = self.jacks.iter().any(|(n, on)| *on && n.contains("Mic"));
        let active_input_port = self.default_source_dev().and_then(|d| d.active_port.as_deref());
        let input_hint = if mic_plugged && active_input_port == Some("analog-input-internal-mic") {
            "잭에 마이크가 감지되었습니다 — 지금은 노트북 내장 마이크로 녹음됩니다. \
             포트를 '마이크'로 바꾸면 꽂은 핀마이크를 사용합니다."
        } else {
            "3.5mm 잭에 꽂은 핀마이크가 인식되지 않으면 포트를 '마이크' 또는 '헤드셋 마이크'로 바꿔보세요."
        };
        col = col.push(device_card(
            "입력 (마이크)",
            &self.sources,
            self.default_source_dev(),
            self.source_vol,
            150,
            AudioMsg::PickSource,
            AudioMsg::PickSourcePort,
            AudioMsg::SourceVol,
            AudioMsg::SourceVolCommit,
            AudioMsg::ToggleSourceMute,
            Some(input_hint),
        ));
        col = col.push(Space::with_height(14));

        // 테스트 결과
        let result_card = test_result_card(&self.last_test);
        col = col.push(result_card);
        col = col.push(Space::with_height(16));

        // 액션 버튼
        let has_recording = matches!(self.last_test, MicTest::Ok { .. });
        let mut actions = row![
            Space::with_width(Length::Fill),
            action_btn("새로고침", AudioMsg::Refresh, !is_running, Color::from_rgb(0.3, 0.3, 0.4)),
            Space::with_width(8),
            action_btn("마이크 테스트 (2초)", AudioMsg::TestMic, !is_running, C_BLUE),
        ].align_y(iced::Alignment::Center);
        if has_recording {
            actions = actions.push(Space::with_width(8));
            actions = actions.push(action_btn("재생", AudioMsg::Play, !is_running, Color::from_rgb(0.25, 0.5, 0.3)));
        }
        col = col.push(actions);

        scrollable(container(col).padding([4, 0])).into()
    }
}

fn device_card<'a>(
    title: &'a str,
    devices: &[DeviceInfo],
    default_dev: Option<&DeviceInfo>,
    vol: u32,
    vol_max: u32,
    pick_dev: fn(Choice) -> AudioMsg,
    pick_port: fn(Choice) -> AudioMsg,
    on_vol: fn(u32) -> AudioMsg,
    vol_commit: AudioMsg,
    toggle_mute: AudioMsg,
    hint: Option<&'a str>,
) -> Element<'a, AudioMsg> {
    let mut body = column![
        text(title).size(13).color(Color::from_rgb(0.7, 0.7, 0.8)),
        Space::with_height(8),
    ];

    if devices.is_empty() {
        body = body.push(text("장치 없음").size(12).color(C_DIM));
        return card(body);
    }

    // 장치 선택
    let dev_opts: Vec<Choice> = devices.iter().map(device_choice).collect();
    let dev_selected = default_dev.map(device_choice);
    body = body.push(
        row![
            container(text("장치").size(12).color(C_DIM)).width(50),
            pick_list(dev_opts, dev_selected, pick_dev)
                .text_size(12)
                .width(Length::Fill),
        ]
        .align_y(iced::Alignment::Center)
    );

    if let Some(dev) = default_dev {
        // 포트 선택 (포트가 있는 장치만 — 블루투스/USB는 포트가 없을 수 있음)
        if !dev.ports.is_empty() {
            let port_opts: Vec<Choice> = dev.ports.iter().map(port_choice).collect();
            let port_selected = dev.active_port.as_deref().and_then(|ap| {
                dev.ports.iter().find(|p| p.name == ap).map(port_choice)
            });
            body = body.push(Space::with_height(6));
            body = body.push(
                row![
                    container(text("포트").size(12).color(C_DIM)).width(50),
                    pick_list(port_opts, port_selected, pick_port)
                        .text_size(12)
                        .width(Length::Fill),
                ]
                .align_y(iced::Alignment::Center)
            );
        }

        // 볼륨 + 음소거
        let mute_label = if dev.muted { "음소거 해제" } else { "음소거" };
        let mute_color = if dev.muted { C_ERR } else { Color::from_rgb(0.3, 0.3, 0.4) };
        body = body.push(Space::with_height(8));
        body = body.push(
            row![
                container(text("볼륨").size(12).color(C_DIM)).width(50),
                slider(0..=vol_max, vol, on_vol).on_release(vol_commit).width(Length::Fill),
                Space::with_width(8),
                container(
                    text(format!("{vol}%")).size(12)
                        .color(if dev.muted { C_ERR } else { Color::WHITE })
                ).width(40),
                action_btn(mute_label, toggle_mute, true, mute_color),
            ]
            .align_y(iced::Alignment::Center)
        );
        if dev.muted {
            body = body.push(Space::with_height(4));
            body = body.push(text("현재 음소거 상태입니다.").size(11).color(C_ERR));
        }
    }

    if let Some(h) = hint {
        body = body.push(Space::with_height(8));
        body = body.push(text(h).size(11).color(C_WARN));
    }

    card(body)
}

fn jack_card(jacks: &[(String, bool)]) -> Element<'_, AudioMsg> {
    let mut body = column![
        text("3.5mm 잭 감지 (하드웨어)").size(13).color(Color::from_rgb(0.7, 0.7, 0.8)),
        Space::with_height(8),
    ];
    if jacks.is_empty() {
        body = body.push(text("잭 감지 정보 없음").size(12).color(C_DIM));
    } else {
        for (name, on) in jacks {
            let (mark, state, scol) = if *on {
                ("●", "꽂힘", C_OK)
            } else {
                ("○", "비어 있음", C_DIM)
            };
            body = body.push(
                row![
                    container(text(jack_label(name)).size(12)).width(180),
                    text(format!("{mark} {state}")).size(12).color(scol),
                ]
                .align_y(iced::Alignment::Center)
            );
        }
    }
    card(body)
}

fn jack_label(name: &str) -> String {
    match name {
        "Headphone Mic Jack" => "콤보 잭 (헤드폰/마이크)".into(),
        "Headphone Jack" => "헤드폰 잭".into(),
        "Headset Mic Jack" => "헤드셋 마이크 잭".into(),
        "Mic Jack" => "마이크 잭".into(),
        "Line Out Jack" => "라인 출력 잭".into(),
        "Line Jack" => "라인 입력 잭".into(),
        _ => name.trim_end_matches(" Jack").to_string(),
    }
}

fn device_choice(d: &DeviceInfo) -> Choice {
    let label = if d.desc.is_empty() { d.name.clone() } else { d.desc.clone() };
    Choice { id: d.name.clone(), label }
}

fn port_choice(p: &PortInfo) -> Choice {
    let mut label = if p.desc.is_empty() { p.name.clone() } else { p.desc.clone() };
    if p.available == Some(false) {
        label.push_str(" (연결 안 됨)");
    }
    Choice { id: p.name.clone(), label }
}

fn apply(script: String, ok_msg: String) -> Task<AudioMsg> {
    Task::perform(
        async move {
            let r = runner::run_sh(&script).await;
            if r.success && r.output.trim().is_empty() {
                CmdResult { success: true, output: ok_msg }
            } else if r.success {
                CmdResult { success: true, output: format!("{ok_msg}\n{}", r.output.trim()) }
            } else {
                r
            }
        },
        AudioMsg::Applied,
    )
}

fn test_result_card(test: &MicTest) -> Element<'_, AudioMsg> {
    match test {
        MicTest::None => card(
            text("테스트 버튼을 눌러 마이크 입력을 확인하세요.").size(12).color(C_DIM)
        ),
        MicTest::Recording => card(
            text("녹음 중...").size(12).color(C_WARN)
        ),
        MicTest::Failed(e) => card(
            column![
                text("[실패]").size(13).color(C_ERR),
                Space::with_height(4),
                text(e).size(11).color(C_DIM),
            ]
        ),
        MicTest::Ok { peak_pct, rms_pct, samples } => {
            let (verdict, vcol) = if *peak_pct < 1.0 {
                ("무음 — 마이크 입력이 거의 없음 (mute/볼륨/잭 확인)", C_ERR)
            } else if *peak_pct < 5.0 {
                ("약함 — 신호는 있지만 매우 낮음 (Mic Boost/거리 확인)", C_WARN)
            } else {
                ("정상 — 마이크 입력 잡힘", C_OK)
            };
            card(
                column![
                    row![
                        text("결과: ").size(13),
                        text(verdict).size(13).color(vcol),
                    ],
                    Space::with_height(8),
                    text(format!("Peak: {:.1}%   RMS: {:.2}%   샘플: {}", peak_pct, rms_pct, samples))
                        .size(11).color(C_DIM),
                    Space::with_height(4),
                    text(level_bar(*peak_pct)).size(12).color(vcol),
                ]
            )
        }
    }
}

fn level_bar(peak_pct: f64) -> String {
    let filled = (peak_pct / 5.0).round() as usize;
    let filled = filled.min(20);
    let empty = 20 - filled;
    format!("[{}{}] {:.1}%", "■".repeat(filled), "□".repeat(empty), peak_pct)
}

async fn scan_audio() -> AudioScan {
    let (sinks_r, sources_r, dsink_r, dsource_r, jacks_r) = tokio::join!(
        runner::run("pactl", &["list", "sinks"]),
        runner::run("pactl", &["list", "sources"]),
        runner::run("pactl", &["get-default-sink"]),
        runner::run("pactl", &["get-default-source"]),
        runner::run_sh("for d in /proc/asound/card[0-9]*; do amixer -c \"${d##*card}\" contents 2>/dev/null; done"),
    );

    let sinks = parse_devices(&sinks_r.output);
    let sources = parse_devices(&sources_r.output)
        .into_iter()
        .filter(|d| !d.name.ends_with(".monitor"))
        .collect();

    let trim_name = |r: &CmdResult| -> Option<String> {
        if !r.success { return None; }
        let s = r.output.trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    };

    AudioScan {
        sinks,
        sources,
        default_sink: trim_name(&dsink_r),
        default_source: trim_name(&dsource_r),
        jacks: parse_jacks(&jacks_r.output),
    }
}

/// `amixer contents`에서 iface=CARD 잭 감지 항목 추출.
/// Phantom(항상 on, 감지 불가)과 HDMI는 제외 — 3.5mm 물리 잭만.
fn parse_jacks(out: &str) -> Vec<(String, bool)> {
    let mut jacks = Vec::new();
    let mut cur: Option<String> = None;
    for line in out.lines() {
        let t = line.trim();
        if t.starts_with("numid=") {
            cur = None;
            if !t.contains("iface=CARD") { continue; }
            if let Some(i) = t.find("name='") {
                let rest = &t[i + 6..];
                if let Some(j) = rest.find('\'') {
                    let name = &rest[..j];
                    if name.ends_with("Jack") && !name.contains("Phantom") && !name.contains("HDMI") {
                        cur = Some(name.to_string());
                    }
                }
            }
        } else if let Some(name) = cur.take() {
            if let Some(v) = t.strip_prefix(": values=") {
                jacks.push((name, v.trim() == "on"));
            } else {
                cur = Some(name); // "; type=BOOLEAN..." 줄은 건너뜀
            }
        }
    }
    jacks
}

/// `pactl list sinks|sources` 텍스트 출력 파싱.
/// (pactl 16.1의 --format=json은 한글 description을 "(null)"로 깨뜨리는 버그가 있어 텍스트 파싱 사용)
fn parse_devices(out: &str) -> Vec<DeviceInfo> {
    let mut devs: Vec<DeviceInfo> = Vec::new();
    let mut cur: Option<DeviceInfo> = None;
    let mut in_ports = false;

    for line in out.lines() {
        let t = line.trim();

        if let Some(v) = t.strip_prefix("Name:") {
            if let Some(d) = cur.take() { devs.push(d); }
            in_ports = false;
            cur = Some(DeviceInfo {
                name: v.trim().to_string(),
                desc: String::new(),
                ports: Vec::new(),
                active_port: None,
                volume_pct: None,
                muted: false,
            });
            continue;
        }
        let Some(dev) = cur.as_mut() else { continue };

        if in_ports {
            if t.starts_with("Active Port:") || t.starts_with("Formats:") || t.starts_with("Properties:") {
                in_ports = false;
                // 아래 일반 파싱으로 계속 진행
            } else if let Some((pname, rest)) = t.split_once(':') {
                let (desc, available) = split_port_desc(rest.trim());
                dev.ports.push(PortInfo { name: pname.trim().to_string(), desc, available });
                continue;
            } else {
                continue;
            }
        }

        if let Some(v) = t.strip_prefix("Description:") {
            dev.desc = v.trim().to_string();
        } else if let Some(v) = t.strip_prefix("Mute:") {
            dev.muted = v.trim() == "yes";
        } else if t.starts_with("Volume:") && dev.volume_pct.is_none() {
            dev.volume_pct = parse_first_percent(t);
        } else if t == "Ports:" {
            in_ports = true;
        } else if let Some(v) = t.strip_prefix("Active Port:") {
            dev.active_port = Some(v.trim().to_string());
        }
    }
    if let Some(d) = cur.take() { devs.push(d); }
    devs
}

/// 포트 라인의 "설명 (type: ..., availability ...)" 부분 분리
fn split_port_desc(rest: &str) -> (String, Option<bool>) {
    let (desc, meta) = match rest.rfind("(type:") {
        Some(i) => (rest[..i].trim().to_string(), &rest[i..]),
        None => (rest.trim().to_string(), ""),
    };
    let available = if meta.contains("not available") {
        Some(false)
    } else if meta.contains("availability unknown") {
        None
    } else if meta.contains("available") {
        Some(true)
    } else {
        None
    };
    (desc, available)
}

fn parse_first_percent(s: &str) -> Option<u32> {
    let mut idx = 0;
    while let Some(p) = s[idx..].find('%') {
        let abs = idx + p;
        let start = s[..abs].rfind(|c: char| !c.is_ascii_digit())
            .map(|i| i + 1).unwrap_or(0);
        if start < abs {
            if let Ok(n) = s[start..abs].parse::<u32>() {
                return Some(n);
            }
        }
        idx = abs + 1;
    }
    None
}

async fn test_mic() -> MicTest {
    let _ = std::fs::remove_file(TEST_WAV);
    let r = runner::run("arecord", &[
        "-D", "pulse", "-f", "S16_LE", "-r", "48000", "-c", "1", "-d", "2", TEST_WAV
    ]).await;
    if !r.success {
        return MicTest::Failed(r.output.trim().to_string());
    }
    let bytes = match tokio::fs::read(TEST_WAV).await {
        Ok(b) => b,
        Err(e) => return MicTest::Failed(format!("파일 읽기 실패: {e}")),
    };
    if bytes.len() <= 44 {
        return MicTest::Failed("녹음 파일이 비어있거나 너무 짧음".into());
    }
    let samples_bytes = &bytes[44..];
    let n = samples_bytes.len() / 2;
    if n == 0 {
        return MicTest::Failed("샘플 0개".into());
    }
    let mut peak: i32 = 0;
    let mut sum_sq: u128 = 0;
    for chunk in samples_bytes.chunks_exact(2) {
        let s = i16::from_le_bytes([chunk[0], chunk[1]]) as i32;
        let a = s.abs();
        if a > peak { peak = a; }
        sum_sq += (s as i64 * s as i64) as u128;
    }
    let rms = ((sum_sq as f64) / (n as f64)).sqrt();
    let peak_pct = peak as f64 / 32768.0 * 100.0;
    let rms_pct = rms / 32768.0 * 100.0;
    MicTest::Ok { peak_pct, rms_pct, samples: n }
}
