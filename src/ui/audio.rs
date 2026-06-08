use iced::{
    widget::{column, container, row, scrollable, text, Space},
    Color, Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_DIM, C_ERR, C_OK, C_WARN};

const TEST_WAV: &str = "/tmp/popmgr-mictest.wav";

#[derive(Debug, Clone)]
pub struct AudioStatus {
    pub active_port: Option<String>,
    pub source_volume_pct: Option<u32>,
    pub source_muted: bool,
    pub default_source: Option<String>,
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
    Refreshed(AudioStatus),
    TestMic,
    TestDone(MicTest),
    Play,
    Done(CmdResult),
}

pub struct AudioState {
    pub status: Option<AudioStatus>,
    pub running: Option<String>,
    pub last_test: MicTest,
}

impl AudioState {
    pub fn new() -> Self {
        Self { status: None, running: None, last_test: MicTest::None }
    }

    pub fn update(&mut self, msg: AudioMsg) -> (Task<AudioMsg>, Option<CmdResult>) {
        match msg {
            AudioMsg::Refresh => {
                let t = Task::perform(async { scan_audio().await }, AudioMsg::Refreshed);
                (t, None)
            }
            AudioMsg::Refreshed(s) => { self.status = Some(s); (Task::none(), None) }
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
            text("3.5mm 잭/내장 마이크 입력을 빠르게 검증합니다.")
                .size(11)
                .color(C_DIM),
            Space::with_height(16),
        ];

        if let Some(label) = &self.running {
            col = col.push(running_bar(label)).push(Space::with_height(12));
        }

        // 현재 상태
        let status_card: Element<'_, AudioMsg> = if let Some(st) = &self.status {
            let port = st.active_port.as_deref().unwrap_or("(없음)");
            let vol = st.source_volume_pct.map(|v| format!("{v}%")).unwrap_or_else(|| "?".into());
            let mute_txt = if st.source_muted { "Muted" } else { "Unmuted" };
            let mute_col = if st.source_muted { C_ERR } else { C_OK };
            card(
                column![
                    text("기본 입력 장치").size(13).color(Color::from_rgb(0.7, 0.7, 0.8)),
                    Space::with_height(6),
                    text(format!("활성 포트: {port}")).size(12),
                    text(format!("볼륨: {vol}")).size(12),
                    row![
                        text("상태: ").size(12),
                        text(mute_txt).size(12).color(mute_col),
                    ],
                ]
            )
        } else {
            text("스캔 중...").size(13).color(C_DIM).into()
        };
        col = col.push(status_card);
        col = col.push(Space::with_height(14));

        // 테스트 결과
        let result_card = test_result_card(&self.last_test);
        col = col.push(result_card);
        col = col.push(Space::with_height(16));

        // 액션 버튼
        let has_recording = matches!(self.last_test, MicTest::Ok { .. });
        let mut actions = row![
            Space::with_width(Length::Fill),
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

async fn scan_audio() -> AudioStatus {
    let default_source = {
        let r = runner::run("pactl", &["get-default-source"]).await;
        if r.success {
            let s = r.output.trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        } else { None }
    };

    let mut active_port = None;
    let mut source_volume_pct = None;
    let mut source_muted = false;

    if let Some(ref name) = default_source {
        let r = runner::run("pactl", &["list", "sources"]).await;
        let mut in_target = false;
        for line in r.output.lines() {
            let t = line.trim();
            if t.starts_with("Name:") {
                let v = t.trim_start_matches("Name:").trim();
                in_target = v == name;
                continue;
            }
            if !in_target { continue; }
            if let Some(rest) = t.strip_prefix("Active Port:") {
                active_port = Some(rest.trim().to_string());
            } else if let Some(rest) = t.strip_prefix("Mute:") {
                source_muted = rest.trim() == "yes";
            } else if t.starts_with("Volume:") {
                if let Some(pct) = parse_first_percent(t) {
                    source_volume_pct = Some(pct);
                }
            }
        }
    }

    AudioStatus { active_port, source_volume_pct, source_muted, default_source }
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
