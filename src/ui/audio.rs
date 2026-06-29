use iced::{
    widget::{column, container, pick_list, row, scrollable, slider, text, Space},
    Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_BTN2, C_DIM, C_ERR, C_GREEN, C_OK, C_TEXT, C_WARN};

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
    pub vref: Option<VrefInfo>,
    /// 마이크 부스트 현재값: ("Headset Mic Boost", 2) 등 — amixer 0~3 (×10dB)
    pub boosts: Vec<(String, u32)>,
    /// 노이즈 억제(echo-cancel) 모듈 ID — Some이면 켜져 있음
    pub denoise_module: Option<String>,
}

/// 고정된 입력 프로파일 — 잭 재연결/재부팅으로 설정이 리셋되면 자동 복원
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AudioLock {
    pub source: String,
    pub port: String,
    pub volume_pct: u32,
    pub boost_ctl: String,
    pub boost_val: u32,
}

fn lock_path() -> std::path::PathBuf {
    dirs::config_dir().unwrap_or_else(|| "/tmp".into()).join("popmgr/audio-lock.json")
}

fn load_lock() -> Option<AudioLock> {
    serde_json::from_str(&std::fs::read_to_string(lock_path()).ok()?).ok()
}

fn save_lock(l: &AudioLock) -> std::io::Result<()> {
    let p = lock_path();
    if let Some(d) = p.parent() { let _ = std::fs::create_dir_all(d); }
    std::fs::write(p, serde_json::to_string_pretty(l).unwrap_or_default())
}

/// 사용자가 수동으로 저장하는 전체 오디오 프로필 (출력+입력+부스트+노이즈억제).
/// "고정"(AudioLock, 잭 재연결 자동복원)과 달리 명시적 저장/불러오기 용도.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AudioProfile {
    pub saved_at: String,
    pub sink: String,
    pub sink_desc: String,
    pub sink_port: Option<String>,
    pub sink_volume_pct: u32,
    pub source: String,
    pub source_desc: String,
    pub source_port: Option<String>,
    pub source_volume_pct: u32,
    pub boost_ctl: String,
    pub boost_val: u32,
    pub denoise: bool,
}

fn profile_path() -> std::path::PathBuf {
    dirs::config_dir().unwrap_or_else(|| "/tmp".into()).join("popmgr/audio-profile.json")
}

fn load_profile() -> Option<AudioProfile> {
    serde_json::from_str(&std::fs::read_to_string(profile_path()).ok()?).ok()
}

fn save_profile(p: &AudioProfile) -> std::io::Result<()> {
    let path = profile_path();
    if let Some(d) = path.parent() { let _ = std::fs::create_dir_all(d); }
    std::fs::write(path, serde_json::to_string_pretty(p).unwrap_or_default())
}

/// 가상 장치(노이즈 억제가 만든) 여부 — 프로필엔 실제 하드웨어를 저장한다
fn is_virtual_dev(name: &str) -> bool {
    name == "popmgr_denoise" || name.starts_with("echo-cancel")
}

/// 입력 포트 → amixer 부스트 컨트롤 이름
fn boost_ctl_for_port(port: &str) -> &'static str {
    match port {
        "analog-input-internal-mic" => "Internal Mic Boost",
        "analog-input-headphone-mic" => "Headphone Mic Boost",
        "analog-input-headset-mic" => "Headset Mic Boost",
        _ => "",
    }
}

/// 출력 포트까지 포함한 한글 포트 라벨
fn port_label_any(port: &str) -> String {
    match port {
        "analog-output-speaker" => "스피커".to_string(),
        "analog-output-headphones" | "analog-output-headphone" => "헤드폰".to_string(),
        _ => port_label_kr(port).to_string(),
    }
}

fn profile_summary(p: &AudioProfile) -> String {
    let sname = if p.sink_desc.is_empty() { p.sink.clone() } else { p.sink_desc.trim().to_string() };
    let rname = if p.source_desc.is_empty() { p.source.clone() } else { p.source_desc.trim().to_string() };
    let sport = p.sink_port.as_deref().map(|x| format!(" ({})", port_label_any(x))).unwrap_or_default();
    let rport = p.source_port.as_deref().map(|x| format!(" ({})", port_label_any(x))).unwrap_or_default();
    format!(
        "출력: {sname}{sport} {sv}%\n입력: {rname}{rport} {rv}% · 부스트 +{b}dB · 노이즈억제 {dn}",
        sv = p.sink_volume_pct, rv = p.source_volume_pct, b = p.boost_val * 10,
        dn = if p.denoise { "켜짐" } else { "꺼짐" },
    )
}

/// 저장된 프로필을 한 번에 적용하는 셸 스크립트 생성
fn build_load_script(p: &AudioProfile) -> String {
    let sink_port = p.sink_port.as_deref()
        .map(|pt| format!("pactl set-sink-port '{}' '{}'; ", p.sink, pt)).unwrap_or_default();
    let src_port = p.source_port.as_deref()
        .map(|pt| format!("pactl set-source-port '{}' '{}'; ", p.source, pt)).unwrap_or_default();
    let boost = if p.boost_ctl.is_empty() { String::new() }
        else { format!("amixer -c0 sset '{}' {} >/dev/null; ", p.boost_ctl, p.boost_val) };
    let want = if p.denoise { 1 } else { 0 };
    format!(
        "pactl set-default-sink '{sink}'; \
         pactl list short sink-inputs | cut -f1 | while read -r i; do pactl move-sink-input \"$i\" '{sink}' 2>/dev/null||true; done; \
         {sink_port}pactl set-sink-volume '{sink}' {svol}%; \
         {src_port}pactl set-source-volume '{src}' {srcvol}%; {boost}\
         WANT={want}; CUR=$(pactl list short modules | awk '/module-echo-cancel/{{print $1; exit}}'); \
         if [ \"$WANT\" = 1 ]; then \
           if [ -z \"$CUR\" ]; then \
             pactl set-default-source '{src}'; \
             pactl load-module module-echo-cancel 'source_name=popmgr_denoise aec_method=webrtc aec_args=\"analog_gain_control=0 digital_gain_control=1 noise_suppression=1\"' >/dev/null; sleep 0.3; \
           fi; \
           pactl set-default-source popmgr_denoise; \
           pactl list short source-outputs | cut -f1 | while read -r i; do pactl move-source-output \"$i\" popmgr_denoise 2>/dev/null||true; done; \
         else \
           if [ -n \"$CUR\" ]; then pactl unload-module $CUR; sleep 0.3; fi; \
           pactl set-default-source '{src}'; \
           pactl list short source-outputs | cut -f1 | while read -r i; do pactl move-source-output \"$i\" '{src}' 2>/dev/null||true; done; \
         fi; \
         echo '오디오 설정 불러옴 (저장 {ts})'",
        sink = p.sink, src = p.source, svol = p.sink_volume_pct, srcvol = p.source_volume_pct,
        ts = p.saved_at,
    )
}

pub fn port_label_kr(port: &str) -> &str {
    match port {
        "analog-input-internal-mic" => "내부 마이크",
        "analog-input-headphone-mic" => "마이크 (잭)",
        "analog-input-headset-mic" => "헤드셋 마이크",
        _ => port,
    }
}

/// 잭 마이크(핀마이크) 바이어스 전원 상태 — /proc/asound 코덱 덤프에서 파싱
#[derive(Debug, Clone, PartialEq)]
pub struct VrefInfo {
    pub hwdev: String,      // /dev/snd/hwC0D0
    pub nid: u32,           // Headphone Mic 핀 (예: 0x1a)
    pub bias_on: bool,      // VREF_50/80/100이면 true (HIZ/GRD면 false)
    pub vendor_id: String,  // 0x10ec0298
    pub subsys_id: String,  // 0x1028087c
    pub codec_addr: u32,
    pub boot_patch: bool,   // 부팅 영구 패치 설치 여부
    pub nopass: bool,       // 비번 없는 sudo 헬퍼 설치 여부 (자동 재적용 가능)
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
    /// 스피커→마이크 루프 테스트 결과 (1kHz 톤 검출)
    Loop { snr_db: f64, peak_pct: f64, rms_pct: f64 },
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum AudioMsg {
    Refresh,
    Refreshed(AudioScan),
    /// 사용자가 명시적으로 "장치 다시 읽기"를 누름 — 빈 결과여도 강제 반영 + 상태를 로그에 출력
    Reload,
    Reloaded(AudioScan),
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
    VrefOn,
    VrefSetupNopass,
    VrefBootInstall,
    VrefBootRemove,
    PickTestTarget(Choice),
    PickBoost(Choice),
    ToggleDenoise,
    LockProfile,
    UnlockProfile,
    SaveProfile,
    ProfileSaved(AudioProfile, CmdResult),
    LoadProfile,
    TestMic,
    LoopTest,
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
    pub vref: Option<VrefInfo>,
    pub boosts: Vec<(String, u32)>,
    pub denoise_module: Option<String>,
    pub lock: Option<AudioLock>,
    pub profile: Option<AudioProfile>,
    enforcing: bool,
    vref_fixing: bool,
    /// 앱 시작 시 고정 설정 1회 전체 적용 (재부팅 후 볼륨/부스트 어긋남 보정)
    startup_enforced: bool,
    pub scanned: bool,
    pub sink_vol: u32,
    pub source_vol: u32,
    sink_dragging: bool,
    source_dragging: bool,
    pub running: Option<String>,
    pub last_test: MicTest,
    /// 테스트 녹음 대상 (id = "소스이름\t포트이름")
    pub test_target: Option<Choice>,
    pub last_test_label: String,
}

impl AudioState {
    pub fn new() -> Self {
        Self {
            sinks: Vec::new(),
            sources: Vec::new(),
            default_sink: None,
            default_source: None,
            jacks: Vec::new(),
            vref: None,
            boosts: Vec::new(),
            denoise_module: None,
            lock: load_lock(),
            profile: load_profile(),
            enforcing: false,
            vref_fixing: false,
            startup_enforced: false,
            scanned: false,
            sink_vol: 0,
            source_vol: 0,
            sink_dragging: false,
            source_dragging: false,
            running: None,
            last_test: MicTest::None,
            test_target: None,
            last_test_label: String::new(),
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

    /// 활성 입력 포트에 해당하는 마이크 부스트 선택 UI
    fn boost_row(&self) -> Option<Element<'_, AudioMsg>> {
        let dev = self.default_source_dev()?;
        let ctl = match dev.active_port.as_deref()? {
            "analog-input-internal-mic" => "Internal Mic Boost",
            "analog-input-headphone-mic" => "Headphone Mic Boost",
            "analog-input-headset-mic" => "Headset Mic Boost",
            _ => return None,
        };
        let cur = self.boosts.iter().find(|(n, _)| n == ctl).map(|(_, v)| *v)?;
        let opts: Vec<Choice> = (0..=3u32).map(|v| Choice {
            id: format!("{ctl}\t{v}"),
            label: if v == 0 { "0 dB (부스트 끔)".to_string() } else { format!("+{} dB", v * 10) },
        }).collect();
        let selected = opts.iter().find(|c| c.id == format!("{ctl}\t{cur}")).cloned();
        Some(card(
            column![
                text("마이크 부스트 (아날로그 게인)").size(13).color(C_TEXT),
                Space::with_height(6),
                row![
                    container(text("부스트").size(12).color(C_DIM)).width(50),
                    pick_list(opts, selected, AudioMsg::PickBoost)
                        .text_size(12)
                        .width(Length::Fill),
                ].align_y(iced::Alignment::Center),
                Space::with_height(4),
                text("녹음이 깨지거나 너무 크면 낮추고, 너무 작으면 한 단계씩 올리세요. (볼륨 슬라이더를 움직이면 부스트가 자동 재조정될 수 있음)")
                    .size(11).color(C_DIM),
            ]
        ))
    }

    /// 테스트 녹음 대상 후보: 모든 입력 장치 × 포트 조합
    fn test_target_choices(&self) -> Vec<Choice> {
        let mut out = Vec::new();
        for d in &self.sources {
            if d.ports.is_empty() {
                out.push(test_choice(d, None));
            } else {
                for p in &d.ports {
                    out.push(test_choice(d, Some(p)));
                }
            }
        }
        out
    }

    /// 스캔 결과를 상태 필드에 반영.
    /// force=false: 빈 목록은 무시(자동 새로고침 시 "장치 없음" 깜빡임 방지)
    /// force=true: 빈 결과여도 그대로 반영(명시적 다시 읽기 — 멈춘 화면 복구)
    fn absorb_scan(&mut self, s: AudioScan, force: bool) {
        if force || !s.sinks.is_empty() || self.sinks.is_empty() {
            self.sinks = s.sinks;
        }
        if force || !s.sources.is_empty() || self.sources.is_empty() {
            self.sources = s.sources;
        }
        self.default_sink = s.default_sink;
        self.default_source = s.default_source;
        self.jacks = s.jacks;
        self.vref = s.vref;
        self.boosts = s.boosts;
        self.denoise_module = s.denoise_module;
        self.scanned = true;
        // 슬라이더 드래그 중에는 자동 새로고침이 값을 덮어쓰지 않도록
        if !self.sink_dragging {
            self.sink_vol = self.default_sink_dev().and_then(|d| d.volume_pct).unwrap_or(0);
        }
        if !self.source_dragging {
            self.source_vol = self.default_source_dev().and_then(|d| d.volume_pct).unwrap_or(0);
        }
        // 테스트 대상 초기화/검증
        let valid: Vec<Choice> = self.test_target_choices();
        match &self.test_target {
            Some(t) if valid.contains(t) => {}
            _ => {
                // 기본: 현재 기본 소스의 활성 포트
                self.test_target = self.default_source_dev().map(|d| {
                    let port = d.active_port.as_deref()
                        .and_then(|ap| d.ports.iter().find(|p| p.name == ap));
                    test_choice(d, port)
                });
            }
        }
    }

    /// 현재 화면의 선택값으로 저장용 프로필 구성.
    /// 노이즈 억제가 켜진 상태면 기본 장치가 가상 장치이므로 실제 하드웨어로 대체.
    fn build_profile(&self) -> Option<AudioProfile> {
        let sink = self.default_sink_dev()
            .filter(|d| !is_virtual_dev(&d.name))
            .or_else(|| self.sinks.iter().find(|d| !is_virtual_dev(&d.name)))?;
        let source = self.default_source_dev()
            .filter(|d| !is_virtual_dev(&d.name))
            .or_else(|| self.sources.iter().find(|d| !is_virtual_dev(&d.name)))?;
        let boost_ctl = source.active_port.as_deref()
            .map(boost_ctl_for_port).unwrap_or("").to_string();
        let boost_val = self.boosts.iter()
            .find(|(n, _)| *n == boost_ctl).map(|(_, v)| *v).unwrap_or(0);
        Some(AudioProfile {
            saved_at: String::new(),
            sink: sink.name.clone(),
            sink_desc: sink.desc.clone(),
            sink_port: sink.active_port.clone(),
            sink_volume_pct: sink.volume_pct.unwrap_or(self.sink_vol),
            source: source.name.clone(),
            source_desc: source.desc.clone(),
            source_port: source.active_port.clone(),
            source_volume_pct: source.volume_pct.unwrap_or(self.source_vol),
            boost_ctl,
            boost_val,
            denoise: self.denoise_module.is_some(),
        })
    }

    pub fn update(&mut self, msg: AudioMsg) -> (Task<AudioMsg>, Option<CmdResult>) {
        match msg {
            AudioMsg::Refresh => {
                let t = Task::perform(async { scan_audio().await }, AudioMsg::Refreshed);
                (t, None)
            }
            AudioMsg::Reload => {
                self.running = Some("장치 다시 읽는 중...".into());
                (Task::perform(async { scan_audio().await }, AudioMsg::Reloaded), None)
            }
            AudioMsg::Reloaded(s) => {
                self.running = None;
                let summary = device_state_summary(&s);
                // 명시적 reload는 빈 결과여도 강제 반영 (멈춘 화면 복구)
                self.absorb_scan(s, true);
                (Task::none(), Some(CmdResult { success: true, output: summary }))
            }
            AudioMsg::Refreshed(s) => {
                self.absorb_scan(s, false);

                // 앱 시작 직후 1회: 고정 설정 전체 적용 (재부팅/로그인 후 상태 보정)
                if !self.startup_enforced {
                    self.startup_enforced = true;
                    if let Some(lock) = self.lock.clone() {
                        if self.sources.iter().any(|d| d.name == lock.source) {
                            let boost_cmd = if lock.boost_ctl.is_empty() { String::new() } else {
                                format!("amixer -c0 sset '{}' {} >/dev/null; ", lock.boost_ctl, lock.boost_val)
                            };
                            let script = format!(
                                "pactl set-source-port '{src}' '{port}'; \
                                 pactl set-source-volume '{src}' {vol}%; {boost_cmd}\
                                 pactl set-default-source '{src}'; \
                                 echo '시작 시 고정된 입력 설정 적용 ({label}, {vol}%, 부스트 +{bdb}dB)'",
                                src = lock.source, port = lock.port, vol = lock.volume_pct,
                                label = port_label_kr(&lock.port), bdb = lock.boost_val * 10,
                            );
                            return (apply(script, "고정 설정 적용됨".into()), None);
                        }
                    }
                }

                // 고정 프로파일 자동 복원: 잭 마이크가 꽂혀 있는데
                // 시스템이 포트를 내부 마이크로 되돌려놨으면 고정 설정 재적용
                if let Some(lock) = self.lock.clone() {
                    let plugged = self.jacks.iter().any(|(n, on)| *on && n.contains("Mic"));
                    let active = self.default_source_dev().and_then(|d| d.active_port.clone());
                    let reset_detected = lock.port != "analog-input-internal-mic"
                        && active.as_deref() == Some("analog-input-internal-mic");
                    if plugged && reset_detected && !self.enforcing && self.running.is_none() {
                        self.enforcing = true;
                        let move_streams = if self.denoise_module.is_some() {
                            String::new() // 노이즈 억제 중엔 denoise 소스가 기본 — 건드리지 않음
                        } else {
                            format!(
                                "pactl set-default-source '{src}'; \
                                 pactl list short source-outputs | cut -f1 | while read -r i; do \
                                   pactl move-source-output \"$i\" '{src}' 2>/dev/null || true; done; ",
                                src = lock.source
                            )
                        };
                        let boost_cmd = if lock.boost_ctl.is_empty() { String::new() } else {
                            format!("amixer -c0 sset '{}' {} >/dev/null; ", lock.boost_ctl, lock.boost_val)
                        };
                        let script = format!(
                            "pactl set-source-port '{src}' '{port}' && \
                             pactl set-source-volume '{src}' {vol}%; {boost_cmd}{move_streams}\
                             echo '잭 재연결 감지 — 고정된 입력 설정 자동 복원 ({label}, {vol}%, 부스트 +{bdb}dB)'",
                            src = lock.source, port = lock.port, vol = lock.volume_pct,
                            label = port_label_kr(&lock.port), bdb = lock.boost_val * 10,
                        );
                        return (apply(script, "고정 설정 복원됨".into()), None);
                    }
                    if !reset_detected {
                        self.enforcing = false;
                    }
                }

                // 핀마이크 전원 자동 재적용: 비번 없는 헬퍼가 설치되어 있고
                // '마이크(잭)' 포트인데 바이어스가 꺼져 있으면 조용히 다시 켬
                if let Some(v) = &self.vref {
                    let active = self.default_source_dev().and_then(|d| d.active_port.as_deref());
                    if v.nopass && !v.bias_on
                        && active == Some("analog-input-headphone-mic")
                        && !self.vref_fixing && self.running.is_none()
                    {
                        self.vref_fixing = true;
                        let script = format!(
                            "sudo -n /usr/local/bin/popmgr-helper --apply-vref '{}' 0x{:x} 0x24 2>&1",
                            v.hwdev, v.nid
                        );
                        return (apply(script, "핀마이크 전원 자동 복원됨".into()), None);
                    }
                    if v.bias_on {
                        self.vref_fixing = false;
                    }
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
            AudioMsg::VrefOn => {
                let Some(v) = self.vref.clone() else { return (Task::none(), None) };
                let exe = std::env::current_exe()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "popmgr".into());
                self.running = Some(if v.nopass {
                    "핀마이크 전원 켜는 중...".into()
                } else {
                    "핀마이크 전원 켜는 중 (관리자 인증 필요)...".into()
                });
                // 비번 없는 헬퍼가 있으면 먼저 시도, 없으면 pkexec
                let script = format!(
                    "sudo -n /usr/local/bin/popmgr-helper --apply-vref '{dev}' 0x{nid:x} 0x24 2>/dev/null \
                     || pkexec '{exe}' --apply-vref '{dev}' 0x{nid:x} 0x24 2>&1",
                    dev = v.hwdev, nid = v.nid
                );
                (apply(script, "핀마이크 바이어스 전원 켜짐 (VREF_80)".into()), None)
            }
            AudioMsg::VrefSetupNopass => {
                let Some(_v) = self.vref.clone() else { return (Task::none(), None) };
                let exe = std::env::current_exe()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "popmgr".into());
                let user = std::env::var("USER").unwrap_or_else(|_| "dell".into());
                self.running = Some("비번 없는 전원 제어 설정 중 (이번 한 번만 인증)...".into());
                // 루트 소유 사본 + 해당 명령만 NOPASSWD 허용 (사용자 쓰기 가능한 원본을 sudoers에 넣지 않음)
                let script = format!(
                    "pkexec bash -c \"install -m 0755 -o root -g root '{exe}' /usr/local/bin/popmgr-helper && \
                     printf '{user} ALL=(root) NOPASSWD: /usr/local/bin/popmgr-helper --apply-vref *\\n' > /etc/sudoers.d/popmgr-vref && \
                     chmod 0440 /etc/sudoers.d/popmgr-vref && \
                     echo '비번 없는 전원 제어 설정 완료 — 이제 자동으로 켜집니다.'\" 2>&1"
                );
                (apply(script, "비번 없는 전원 제어 설정됨".into()), None)
            }
            AudioMsg::VrefBootInstall => {
                let Some(v) = self.vref.clone() else { return (Task::none(), None) };
                self.running = Some("부팅 영구 패치 설치 중 (관리자 인증 필요)...".into());
                let fw = format!(
                    "[codec]\n{} {} {}\n\n[verb]\n0x{:x} 0x707 0x24\n",
                    v.vendor_id, v.subsys_id, v.codec_addr, v.nid
                );
                let script = format!(
                    "pkexec bash -c 'cat > /lib/firmware/popmgr-pinmic.fw <<\"EOF\"\n{fw}EOF\n\
                     cat > /etc/modprobe.d/popmgr-pinmic.conf <<\"EOF\"\noptions snd-hda-intel patch=popmgr-pinmic.fw\nEOF\n\
                     echo \"부팅 패치 설치됨 — 재부팅 후에도 핀마이크 전원이 유지됩니다.\"' 2>&1"
                );
                (apply(script, "부팅 패치 설치됨".into()), None)
            }
            AudioMsg::VrefBootRemove => {
                self.running = Some("부팅 패치 제거 중 (관리자 인증 필요)...".into());
                let script = "pkexec bash -c 'rm -f /lib/firmware/popmgr-pinmic.fw /etc/modprobe.d/popmgr-pinmic.conf && echo 부팅\\ 패치\\ 제거됨' 2>&1".to_string();
                (apply(script, "부팅 패치 제거됨".into()), None)
            }
            AudioMsg::Applied(r) => {
                self.running = None;
                // 설정 반영 후 상태 재스캔
                let t = Task::perform(async { scan_audio().await }, AudioMsg::Refreshed);
                (t, Some(r))
            }
            AudioMsg::PickTestTarget(c) => {
                self.test_target = Some(c);
                (Task::none(), None)
            }
            AudioMsg::PickBoost(c) => {
                let Some((ctl, val)) = c.id.split_once('\t') else { return (Task::none(), None) };
                let script = format!("amixer -c0 sset '{ctl}' {val} >/dev/null && echo '{} → {}'", ctl, c.label);
                (apply(script, format!("{ctl} → {}", c.label)), None)
            }
            AudioMsg::LoopTest => {
                let target = self.test_target.as_ref().map(|c| {
                    let (s, p) = c.id.split_once('\t').unwrap_or((c.id.as_str(), ""));
                    (s.to_string(), p.to_string())
                });
                let sink = self.default_sink.clone();
                self.last_test_label = self.test_target.as_ref()
                    .map(|c| c.label.clone()).unwrap_or_else(|| "기본 입력".into());
                self.running = Some(format!("루프 테스트 중 (4초, 비프음 재생)... [{}]", self.last_test_label));
                self.last_test = MicTest::Recording;
                let t = Task::perform(async move { loop_test(sink, target).await }, AudioMsg::TestDone);
                (t, None)
            }
            AudioMsg::LockProfile => {
                let Some(dev) = self.default_source_dev() else {
                    return (Task::none(), Some(CmdResult { success: false, output: "입력 장치 없음".into() }));
                };
                let Some(port) = dev.active_port.clone() else {
                    return (Task::none(), Some(CmdResult { success: false, output: "활성 포트 없음".into() }));
                };
                let boost_ctl = match port.as_str() {
                    "analog-input-internal-mic" => "Internal Mic Boost",
                    "analog-input-headphone-mic" => "Headphone Mic Boost",
                    "analog-input-headset-mic" => "Headset Mic Boost",
                    _ => "",
                }.to_string();
                let boost_val = self.boosts.iter()
                    .find(|(n, _)| *n == boost_ctl).map(|(_, v)| *v).unwrap_or(0);
                let lock = AudioLock {
                    source: dev.name.clone(),
                    port: port.clone(),
                    volume_pct: self.source_vol,
                    boost_ctl,
                    boost_val,
                };
                let res = match save_lock(&lock) {
                    Ok(()) => CmdResult {
                        success: true,
                        output: format!(
                            "입력 설정 고정됨: {} / 볼륨 {}% / 부스트 +{}dB — 잭을 다시 꽂으면 자동 복원됩니다.",
                            port_label_kr(&lock.port), lock.volume_pct, lock.boost_val * 10
                        ),
                    },
                    Err(e) => CmdResult { success: false, output: format!("고정 저장 실패: {e}") },
                };
                self.lock = Some(lock);
                (Task::none(), Some(res))
            }
            AudioMsg::UnlockProfile => {
                let _ = std::fs::remove_file(lock_path());
                self.lock = None;
                (Task::none(), Some(CmdResult { success: true, output: "입력 설정 고정 해제됨".into() }))
            }
            AudioMsg::SaveProfile => {
                let Some(profile) = self.build_profile() else {
                    return (Task::none(), Some(CmdResult {
                        success: false, output: "저장할 출력/입력 장치를 찾지 못했습니다".into()
                    }));
                };
                // 저장 시각은 시스템 로컬 시간(date)으로 — chrono 의존성 없이.
                let t = Task::perform(async move {
                    let ts = runner::run_sh("date '+%Y-%m-%d %H:%M'").await.output.trim().to_string();
                    let mut p = profile;
                    p.saved_at = if ts.is_empty() { "?".into() } else { ts };
                    let res = match save_profile(&p) {
                        Ok(()) => CmdResult {
                            success: true,
                            output: format!("오디오 설정 저장됨 ({})\n{}", p.saved_at, profile_summary(&p)),
                        },
                        Err(e) => CmdResult { success: false, output: format!("저장 실패: {e}") },
                    };
                    (p, res)
                }, |(p, res)| AudioMsg::ProfileSaved(p, res));
                (t, None)
            }
            AudioMsg::ProfileSaved(p, res) => {
                if res.success { self.profile = Some(p); }
                (Task::none(), Some(res))
            }
            AudioMsg::LoadProfile => {
                let Some(p) = self.profile.clone() else {
                    return (Task::none(), Some(CmdResult {
                        success: false, output: "저장된 오디오 설정이 없습니다".into()
                    }));
                };
                if !self.sinks.iter().any(|d| d.name == p.sink)
                    && !self.sources.iter().any(|d| d.name == p.source)
                {
                    return (Task::none(), Some(CmdResult {
                        success: false,
                        output: "저장된 장치가 현재 목록에 없습니다 — 장치 연결을 확인하세요".into(),
                    }));
                }
                self.running = Some("저장된 오디오 설정 불러오는 중...".into());
                (apply(build_load_script(&p), format!("오디오 설정 불러옴 (저장 {})", p.saved_at)), None)
            }
            AudioMsg::ToggleDenoise => {
                self.running = Some("노이즈 억제 전환 중...".into());
                let script = if let Some(id) = &self.denoise_module {
                    format!(
                        "pactl unload-module {id}; sleep 0.3; \
                         HW=$(pactl list short sources | awk -F'\\t' '$2 !~ /\\.monitor|popmgr_denoise/ {{print $2; exit}}'); \
                         if [ -n \"$HW\" ]; then pactl set-default-source \"$HW\"; \
                           pactl list short source-outputs | cut -f1 | while read -r i; do pactl move-source-output \"$i\" \"$HW\" 2>/dev/null || true; done; fi; \
                         echo '노이즈 억제 꺼짐 — 원본 마이크로 복귀'"
                    )
                } else {
                    "pactl load-module module-echo-cancel \
                     'source_name=popmgr_denoise aec_method=webrtc aec_args=\"analog_gain_control=0 digital_gain_control=1 noise_suppression=1\"' >/dev/null && \
                     sleep 0.3 && pactl set-default-source popmgr_denoise && \
                     pactl list short source-outputs | cut -f1 | while read -r i; do pactl move-source-output \"$i\" popmgr_denoise 2>/dev/null || true; done; \
                     echo '노이즈 억제 켜짐 — OBS 등 모든 앱이 잡음 제거된 마이크를 사용합니다'".to_string()
                };
                (apply(script, "노이즈 억제 전환됨".into()), None)
            }
            AudioMsg::TestMic => {
                let target = self.test_target.as_ref().map(|c| {
                    let (s, p) = c.id.split_once('\t').unwrap_or((c.id.as_str(), ""));
                    (s.to_string(), p.to_string())
                });
                self.last_test_label = self.test_target.as_ref()
                    .map(|c| c.label.clone()).unwrap_or_else(|| "기본 입력".into());
                self.running = Some(format!("녹음 중 (2초)... [{}]", self.last_test_label));
                self.last_test = MicTest::Recording;
                let t = Task::perform(async move { test_mic(target).await }, AudioMsg::TestDone);
                (t, None)
            }
            AudioMsg::TestDone(r) => {
                self.running = None;
                self.last_test = r.clone();
                let result = match &r {
                    MicTest::Ok { peak_pct, rms_pct, samples } => CmdResult {
                        success: *peak_pct > 1.0,
                        output: format!(
                            "마이크 테스트 [{}]: peak {:.1}%, RMS {:.2}%, 샘플 {}",
                            self.last_test_label, peak_pct, rms_pct, samples
                        ),
                    },
                    MicTest::Loop { snr_db, peak_pct, rms_pct } => CmdResult {
                        success: *snr_db > 15.0,
                        output: format!(
                            "루프 테스트 [{}]: 톤 SNR {:.1}dB, peak {:.1}%, RMS {:.2}%",
                            self.last_test_label, snr_db, peak_pct, rms_pct
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

        let mic_plugged = self.jacks.iter().any(|(n, on)| *on && n.contains("Mic"));
        let active_input_port = self.default_source_dev().and_then(|d| d.active_port.as_deref());

        // 핀마이크 전원(바이어스) 카드 — '마이크'(headphone-mic) 포트를 쓸 때만 의미 있음
        if let Some(v) = &self.vref {
            if active_input_port == Some("analog-input-headphone-mic") {
                col = col.push(vref_card(v, is_running));
                col = col.push(Space::with_height(12));
            }
        }

        // 입력 장치 카드
        let input_hint = if mic_plugged && active_input_port == Some("analog-input-internal-mic") {
            "잭에 마이크가 감지되었습니다 — 지금은 노트북 내장 마이크로 녹음됩니다. \
             포트를 '헤드셋 마이크'(4극 플러그) 또는 '마이크'(3극 플러그)로 바꾸면 핀마이크를 사용합니다."
        } else {
            "핀마이크가 지지직거리거나 안 잡히면: 4극(TRRS) 플러그는 '헤드셋 마이크', \
             3극(TRS)은 '마이크' 포트 + 아래 핀마이크 전원을 켜세요."
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
        col = col.push(Space::with_height(8));

        // 마이크 부스트 (아날로그 게인 — 녹음이 너무 크거나 작을 때 조절)
        if let Some(boost_row) = self.boost_row() {
            col = col.push(boost_row);
            col = col.push(Space::with_height(8));
        } else {
            col = col.push(Space::with_height(2));
        }

        // 노이즈 억제 (WebRTC noise suppression)
        col = col.push(denoise_card(self.denoise_module.is_some(), is_running));
        col = col.push(Space::with_height(8));

        // 입력 설정 고정 (잭 재연결 시 자동 복원)
        col = col.push(lock_card(self.lock.as_ref(), is_running));
        col = col.push(Space::with_height(8));

        // 오디오 설정 저장 / 불러오기 (출력+입력 전체)
        col = col.push(profile_card(self.profile.as_ref(), is_running));
        col = col.push(Space::with_height(14));

        // 테스트 대상 선택 (어느 입력으로 녹음할지)
        let test_opts = self.test_target_choices();
        if !test_opts.is_empty() {
            col = col.push(
                row![
                    container(text("테스트 입력").size(12).color(C_DIM)).width(80),
                    pick_list(test_opts, self.test_target.clone(), AudioMsg::PickTestTarget)
                        .text_size(12)
                        .width(Length::Fill),
                ]
                .align_y(iced::Alignment::Center)
            );
            col = col.push(Space::with_height(8));
        }

        // 테스트 결과
        let result_card = test_result_card(&self.last_test, &self.last_test_label);
        col = col.push(result_card);
        col = col.push(Space::with_height(16));

        // 액션 버튼
        let has_recording = matches!(self.last_test, MicTest::Ok { .. } | MicTest::Loop { .. });
        let mut actions = row![
            Space::with_width(Length::Fill),
            action_btn("장치 다시 읽기", AudioMsg::Reload, !is_running, C_BTN2),
            Space::with_width(8),
            action_btn("루프 테스트 (비프음)", AudioMsg::LoopTest, !is_running, C_BTN2),
            Space::with_width(8),
            action_btn("마이크 테스트 (2초)", AudioMsg::TestMic, !is_running, C_BLUE),
        ].align_y(iced::Alignment::Center);
        if has_recording {
            actions = actions.push(Space::with_width(8));
            actions = actions.push(action_btn("재생", AudioMsg::Play, !is_running, C_OK));
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
        text(title).size(13).color(C_TEXT),
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
        let mute_color = if dev.muted { C_ERR } else { C_BTN2 };
        body = body.push(Space::with_height(8));
        body = body.push(
            row![
                container(text("볼륨").size(12).color(C_DIM)).width(50),
                slider(0..=vol_max, vol, on_vol).on_release(vol_commit).width(Length::Fill),
                Space::with_width(8),
                container(
                    text(format!("{vol}%")).size(12)
                        .color(if dev.muted { C_ERR } else { C_TEXT })
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

fn lock_card<'a>(lock: Option<&'a AudioLock>, is_busy: bool) -> Element<'a, AudioMsg> {
    let mut body = column![
        text("입력 설정 고정 (잭 재연결 시 자동 복원)").size(13).color(C_TEXT),
        Space::with_height(6),
    ];
    match lock {
        Some(l) => {
            body = body.push(
                row![
                    container(
                        text(format!(
                            "● 고정됨: {} / 볼륨 {}% / 부스트 +{}dB",
                            port_label_kr(&l.port), l.volume_pct, l.boost_val * 10
                        )).size(12).color(C_OK)
                    ).width(Length::Fill),
                    action_btn("고정 해제", AudioMsg::UnlockProfile, !is_busy, C_BTN2),
                ].align_y(iced::Alignment::Center)
            );
            body = body.push(Space::with_height(4));
            body = body.push(
                text("잭을 뺐다 꽂거나 시스템이 설정을 되돌려도 popmgr가 2초 안에 자동 복원합니다.")
                    .size(11).color(C_DIM)
            );
        }
        None => {
            body = body.push(
                row![
                    container(
                        text("○ 고정 안 됨 — 잭을 다시 꽂으면 시스템이 내부 마이크로 되돌립니다")
                            .size(12).color(C_DIM)
                    ).width(Length::Fill),
                    action_btn("현재 설정 고정", AudioMsg::LockProfile, !is_busy, C_BLUE),
                ].align_y(iced::Alignment::Center)
            );
            body = body.push(Space::with_height(4));
            body = body.push(
                text("핀마이크가 잘 되는 상태에서 누르세요. 포트/볼륨/부스트가 저장됩니다.")
                    .size(11).color(C_DIM)
            );
        }
    }
    card(body)
}

fn profile_card<'a>(profile: Option<&'a AudioProfile>, is_busy: bool) -> Element<'a, AudioMsg> {
    let mut body = column![
        text("오디오 설정 저장 / 불러오기").size(13).color(C_TEXT),
        Space::with_height(6),
    ];
    match profile {
        Some(p) => {
            body = body.push(
                row![
                    container(text(format!("● 저장됨 ({})", p.saved_at)).size(12).color(C_OK))
                        .width(Length::Fill),
                    action_btn("불러오기", AudioMsg::LoadProfile, !is_busy, C_BLUE),
                    Space::with_width(8),
                    action_btn("덮어쓰기", AudioMsg::SaveProfile, !is_busy, C_BTN2),
                ].align_y(iced::Alignment::Center)
            );
            body = body.push(Space::with_height(6));
            body = body.push(text(profile_summary(p)).size(11).color(C_DIM));
        }
        None => {
            body = body.push(
                row![
                    container(text("○ 저장된 설정 없음").size(12).color(C_DIM)).width(Length::Fill),
                    action_btn("현재 설정 저장", AudioMsg::SaveProfile, !is_busy, C_BLUE),
                ].align_y(iced::Alignment::Center)
            );
            body = body.push(Space::with_height(4));
            body = body.push(
                text("지금의 출력/입력 장치·포트·볼륨·마이크 부스트·노이즈 억제 상태를 저장해 두고, 나중에 '불러오기'로 한 번에 복원합니다.")
                    .size(11).color(C_DIM)
            );
        }
    }
    card(body)
}

fn denoise_card<'a>(on: bool, is_busy: bool) -> Element<'a, AudioMsg> {
    let (mark, state, scol) = if on {
        ("●", "켜짐 — 모든 앱이 잡음 제거된 마이크(popmgr_denoise)를 사용 중", C_OK)
    } else {
        ("○", "꺼짐 — 노트북 바닥 잡음이 그대로 녹음됩니다", C_DIM)
    };
    let btn_label = if on { "끄기" } else { "켜기" };
    let btn_color = if on { C_BTN2 } else { C_GREEN };
    card(
        column![
            text("노이즈 억제 (주변/바닥 잡음 제거)").size(13).color(C_TEXT),
            Space::with_height(6),
            row![
                container(text(format!("{mark} {state}")).size(12).color(scol)).width(Length::Fill),
                action_btn(btn_label, AudioMsg::ToggleDenoise, !is_busy, btn_color),
            ].align_y(iced::Alignment::Center),
            Space::with_height(4),
            text("WebRTC 잡음 억제 필터를 마이크 앞단에 끼웁니다. 녹음/방송 앱(OBS 등)은 자동으로 이 필터를 거칩니다.")
                .size(11).color(C_DIM),
        ]
    )
}

fn vref_card(v: &VrefInfo, is_busy: bool) -> Element<'_, AudioMsg> {
    let (mark, state, scol) = if v.bias_on {
        ("●", "켜짐 (VREF_80) — 핀마이크에 전원 공급 중", C_OK)
    } else {
        ("○", "꺼짐 (HIZ) — 이 상태에선 핀마이크가 지지직만 녹음됩니다", C_ERR)
    };

    let mut body = column![
        text("핀마이크 전원 (잭 마이크 바이어스)").size(13).color(C_TEXT),
        Space::with_height(8),
        row![
            text(format!("{mark} {state}")).size(12).color(scol),
        ],
        Space::with_height(4),
        text(format!("코덱 핀 0x{:x} · {}", v.nid, if v.boot_patch { "부팅 패치 설치됨 (재부팅에도 유지)" } else { "부팅 패치 없음 (재부팅/절전 후 다시 켜야 함)" }))
            .size(11).color(C_DIM),
        Space::with_height(8),
    ];

    if v.nopass {
        body = body.push(
            text("비번 없는 자동 제어 활성 — 꺼지면 popmgr가 자동으로 다시 켭니다.")
                .size(11).color(C_OK)
        );
        body = body.push(Space::with_height(6));
    }

    let mut actions = row![Space::with_width(Length::Fill)].align_y(iced::Alignment::Center);
    if !v.bias_on {
        actions = actions.push(action_btn("지금 켜기", AudioMsg::VrefOn, !is_busy, C_BLUE));
        actions = actions.push(Space::with_width(8));
    }
    if !v.nopass {
        actions = actions.push(action_btn("비번 없이 자동제어 설정", AudioMsg::VrefSetupNopass, !is_busy, C_BTN2));
        actions = actions.push(Space::with_width(8));
    }
    if v.boot_patch {
        actions = actions.push(action_btn("부팅 패치 제거", AudioMsg::VrefBootRemove, !is_busy, C_BTN2));
    } else {
        actions = actions.push(action_btn("부팅 시 영구 적용", AudioMsg::VrefBootInstall, !is_busy, C_GREEN));
    }
    body = body.push(actions);

    card(body)
}

fn jack_card(jacks: &[(String, bool)]) -> Element<'_, AudioMsg> {
    let mut body = column![
        text("3.5mm 잭 감지 (하드웨어)").size(13).color(C_TEXT),
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

/// 장치 한 줄 요약: "내장 오디오 아날로그 스테레오 [내부 마이크] vol 30% (음소거)"
fn dev_brief(d: &DeviceInfo) -> String {
    let name = if d.desc.is_empty() { d.name.clone() } else { d.desc.trim().to_string() };
    let port = d.active_port.as_deref()
        .map(|p| format!(" [{}]", port_label_kr(p)))
        .unwrap_or_default();
    let vol = d.volume_pct.map(|v| format!(" vol {v}%")).unwrap_or_default();
    let mute = if d.muted { " (음소거)" } else { "" };
    format!("{name}{port}{vol}{mute}")
}

/// "장치 다시 읽기" 시 로그에 출력할 현재 오디오 장치 상태 요약
fn device_state_summary(s: &AudioScan) -> String {
    let mut lines = vec![format!(
        "장치 다시 읽음 — 출력 {}개 / 입력 {}개", s.sinks.len(), s.sources.len()
    )];

    let find = |list: &[DeviceInfo], def: &Option<String>| -> String {
        match def {
            Some(name) => match list.iter().find(|d| &d.name == name) {
                Some(d) => dev_brief(d),
                None => format!("{name} (목록에 없음)"),
            },
            None => "(설정 안 됨)".into(),
        }
    };
    lines.push(format!("기본 출력: {}", find(&s.sinks, &s.default_sink)));
    lines.push(format!("기본 입력: {}", find(&s.sources, &s.default_source)));

    if s.sinks.is_empty() && s.sources.is_empty() {
        lines.push("⚠ pactl가 장치를 반환하지 않음 — PipeWire/PulseAudio 상태를 확인하세요".into());
    } else {
        for d in &s.sinks { lines.push(format!("· 출력: {}", dev_brief(d))); }
        for d in &s.sources { lines.push(format!("· 입력: {}", dev_brief(d))); }
    }

    if !s.jacks.is_empty() {
        let j: Vec<String> = s.jacks.iter()
            .map(|(n, on)| format!("{}={}", jack_label(n), if *on { "꽂힘" } else { "빔" }))
            .collect();
        lines.push(format!("잭: {}", j.join(", ")));
    }
    if s.denoise_module.is_some() {
        lines.push("노이즈 억제: 켜짐 (기본 입출력이 가상 장치로 바뀜 — 시스템 사운드 패널엔 '없음'으로 보일 수 있음)".into());
    }
    lines.join("\n")
}

fn device_choice(d: &DeviceInfo) -> Choice {
    let label = if d.desc.is_empty() { d.name.clone() } else { d.desc.clone() };
    Choice { id: d.name.clone(), label }
}

fn test_choice(d: &DeviceInfo, p: Option<&PortInfo>) -> Choice {
    let dev_label = if d.desc.is_empty() { d.name.clone() } else { d.desc.clone() };
    match p {
        Some(p) => {
            let port_label = if p.desc.is_empty() { p.name.clone() } else { p.desc.clone() };
            Choice {
                id: format!("{}\t{}", d.name, p.name),
                label: format!("{port_label} — {dev_label}"),
            }
        }
        None => Choice { id: format!("{}\t", d.name), label: dev_label },
    }
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

fn test_result_card<'a>(test: &'a MicTest, label: &'a str) -> Element<'a, AudioMsg> {
    let target_line: Element<'a, AudioMsg> = if label.is_empty() {
        Space::with_height(0).into()
    } else {
        column![
            text(format!("대상: {label}")).size(11).color(C_DIM),
            Space::with_height(4),
        ].into()
    };
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
        MicTest::Loop { snr_db, peak_pct, rms_pct } => {
            let (verdict, vcol) = if *snr_db > 15.0 {
                ("정상 — 이 입력이 스피커 소리를 또렷하게 녹음함", C_OK)
            } else if *snr_db > 5.0 {
                ("약함 — 톤이 희미하게 잡힘 (부스트/볼륨/마이크 위치 확인)", C_WARN)
            } else {
                ("실패 — 톤이 안 잡힘, 노이즈만 녹음됨 (포트가 맞는지 확인)", C_ERR)
            };
            let clip = if *peak_pct >= 99.0 { "   ※ 클리핑 — 부스트/볼륨을 낮추세요" } else { "" };
            card(
                column![
                    target_line,
                    row![
                        text("루프 테스트: ").size(13),
                        text(verdict).size(13).color(vcol),
                    ],
                    Space::with_height(8),
                    text(format!("톤 SNR: {snr_db:.1} dB   Peak: {peak_pct:.1}%   RMS: {rms_pct:.2}%{clip}"))
                        .size(11).color(if *peak_pct >= 99.0 { C_WARN } else { C_DIM }),
                ]
            )
        }
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
                    target_line,
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
    // 한국어 등 비영어 로케일에선 `pactl list`가 필드 라벨을 번역(Name:→이름:)해
    // 파서가 깨진다. 파싱 대상 명령은 항상 LC_ALL=C로 영어 출력을 강제한다.
    let (sinks_r, sources_r, dsink_r, dsource_r, jacks_r, modules_r) = tokio::join!(
        runner::run_sh("LC_ALL=C pactl list sinks"),
        runner::run_sh("LC_ALL=C pactl list sources"),
        runner::run_sh("LC_ALL=C pactl get-default-sink"),
        runner::run_sh("LC_ALL=C pactl get-default-source"),
        runner::run_sh("for d in /proc/asound/card[0-9]*; do LC_ALL=C amixer -c \"${d##*card}\" contents 2>/dev/null; done"),
        runner::run_sh("LC_ALL=C pactl list short modules"),
    );

    let trim_name = |r: &CmdResult| -> Option<String> {
        if !r.success { return None; }
        let s = r.output.trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    };
    let default_sink = trim_name(&dsink_r);
    let default_source = trim_name(&dsource_r);

    let mut sinks = parse_devices(&sinks_r.output);
    let mut sources: Vec<DeviceInfo> = parse_devices(&sources_r.output)
        .into_iter()
        .filter(|d| !d.name.ends_with(".monitor"))
        .collect();

    // 노이즈 억제 토글·잭 재연결 직후엔 PipeWire가 장치를 재생성하는 찰나에
    // `pactl list`가 빈 결과를 돌려줄 때가 있다(기본 장치 이름은 캐시로 살아 있음).
    // 목록만 비고 기본 장치 이름은 있으면 잠깐 기다렸다 한 번 더 읽어 "장치 없음"을 피한다.
    if sinks.is_empty() && default_sink.is_some() {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        sinks = parse_devices(&runner::run_sh("LC_ALL=C pactl list sinks").await.output);
    }
    if sources.is_empty() && default_source.is_some() {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        sources = parse_devices(&runner::run_sh("LC_ALL=C pactl list sources").await.output)
            .into_iter()
            .filter(|d| !d.name.ends_with(".monitor"))
            .collect();
    }

    AudioScan {
        sinks,
        sources,
        default_sink,
        default_source,
        jacks: parse_jacks(&jacks_r.output),
        vref: scan_vref(),
        boosts: parse_boosts(&jacks_r.output),
        denoise_module: modules_r.output.lines()
            .find(|l| l.contains("module-echo-cancel"))
            .and_then(|l| l.split_whitespace().next())
            .map(|s| s.to_string()),
    }
}

/// `amixer contents`에서 '... Mic Boost Volume' 현재값 추출
fn parse_boosts(out: &str) -> Vec<(String, u32)> {
    let mut res = Vec::new();
    let mut cur: Option<String> = None;
    for line in out.lines() {
        let t = line.trim();
        if t.starts_with("numid=") {
            cur = None;
            if let Some(i) = t.find("name='") {
                let rest = &t[i + 6..];
                if let Some(j) = rest.find('\'') {
                    let name = &rest[..j];
                    if name.ends_with("Mic Boost Volume") {
                        cur = Some(name.trim_end_matches(" Volume").to_string());
                    }
                }
            }
        } else if let Some(name) = cur.take() {
            if let Some(v) = t.strip_prefix(": values=") {
                if let Some(first) = v.split(',').next().and_then(|x| x.trim().parse::<u32>().ok()) {
                    res.push((name, first));
                }
            } else {
                cur = Some(name); // "; type=..." 줄 건너뜀
            }
        }
    }
    res
}

/// /proc/asound/cardN/codec#M에서 Headphone Mic 핀의 바이어스(VREF) 상태 파싱.
/// hwdep verb 쓰기는 루트가 필요하지만 proc 읽기는 누구나 가능.
fn scan_vref() -> Option<VrefInfo> {
    for cardn in 0..8u32 {
        for codecn in 0..4u32 {
            let path = format!("/proc/asound/card{cardn}/codec#{codecn}");
            let Ok(txt) = std::fs::read_to_string(&path) else { continue };
            if !txt.contains("Headphone Mic Boost") { continue; }

            let mut vendor_id = String::new();
            let mut subsys_id = String::new();
            let mut codec_addr = 0u32;
            let mut cur_nid: Option<u32> = None;
            let mut target_nid: Option<u32> = None;
            let mut target_pinctl = String::new();

            for line in txt.lines() {
                let t = line.trim();
                if let Some(v) = t.strip_prefix("Vendor Id:") {
                    vendor_id = v.trim().to_string();
                } else if let Some(v) = t.strip_prefix("Subsystem Id:") {
                    subsys_id = v.trim().to_string();
                } else if let Some(v) = t.strip_prefix("Address:") {
                    codec_addr = v.trim().parse().unwrap_or(0);
                } else if let Some(rest) = t.strip_prefix("Node 0x") {
                    cur_nid = rest.split_whitespace().next()
                        .and_then(|h| u32::from_str_radix(h, 16).ok());
                } else if t.contains("name=\"Headphone Mic Boost Volume\"") {
                    target_nid = cur_nid;
                } else if let Some(v) = t.strip_prefix("Pin-ctls:") {
                    if cur_nid.is_some() && cur_nid == target_nid {
                        target_pinctl = v.trim().to_string();
                    }
                }
            }

            let nid = target_nid?;
            let bias_on = target_pinctl.contains("VREF_80")
                || target_pinctl.contains("VREF_100")
                || target_pinctl.contains("VREF_50");
            return Some(VrefInfo {
                hwdev: format!("/dev/snd/hwC{cardn}D{codec_addr}"),
                nid,
                bias_on,
                vendor_id,
                subsys_id,
                codec_addr,
                boot_patch: std::path::Path::new("/etc/modprobe.d/popmgr-pinmic.conf").exists(),
                nopass: std::path::Path::new("/etc/sudoers.d/popmgr-vref").exists(),
            });
        }
    }
    None
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

const TONE_WAV: &str = "/tmp/popmgr-tone.wav";

/// 1kHz 0.5초 on/off 버스트 4초 톤 생성 (루프 테스트용)
fn write_tone_wav() -> std::io::Result<()> {
    let sr = 48000u32;
    let n = sr * 4;
    let mut pcm = Vec::with_capacity((n * 2) as usize);
    for i in 0..n {
        let t = i as f64 / sr as f64;
        let on = (t * 2.0) as u32 % 2 == 0;
        let v = if on {
            (0.5 * 32767.0 * (2.0 * std::f64::consts::PI * 1000.0 * t).sin()) as i16
        } else {
            0
        };
        pcm.extend_from_slice(&v.to_le_bytes());
    }
    let mut w = Vec::with_capacity(44 + pcm.len());
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());      // PCM
    w.extend_from_slice(&1u16.to_le_bytes());      // mono
    w.extend_from_slice(&sr.to_le_bytes());
    w.extend_from_slice(&(sr * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    w.extend_from_slice(&pcm);
    std::fs::write(TONE_WAV, w)
}

fn goertzel(x: &[f64], f: f64, sr: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f / sr;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &q in x {
        let s0 = q + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    s1 * s1 + s2 * s2 - c * s1 * s2
}

/// 스피커로 톤 재생 + 선택 입력으로 동시 녹음 → 1kHz 검출로 입력 경로 검증
async fn loop_test(sink: Option<String>, target: Option<(String, String)>) -> MicTest {
    if let Err(e) = write_tone_wav() {
        return MicTest::Failed(format!("톤 생성 실패: {e}"));
    }
    let _ = std::fs::remove_file(TEST_WAV);

    let mut src_env = String::new();
    if let Some((s, p)) = &target {
        if !p.is_empty() {
            let pr = runner::run("pactl", &["set-source-port", s, p]).await;
            if !pr.success {
                return MicTest::Failed(format!("포트 전환 실패: {}", pr.output.trim()));
            }
        }
        src_env = format!("PULSE_SOURCE='{s}' ");
    }
    let sink_env = sink.map(|s| format!("PULSE_SINK='{s}' ")).unwrap_or_default();
    let script = format!(
        "{sink_env}paplay {TONE_WAV} & \
         {src_env}arecord -D pulse -f S16_LE -r 48000 -c 1 -d 4 {TEST_WAV}; wait"
    );
    let r = runner::run_sh(&script).await;
    if !r.success {
        return MicTest::Failed(r.output.trim().to_string());
    }

    let bytes = match tokio::fs::read(TEST_WAV).await {
        Ok(b) => b,
        Err(e) => return MicTest::Failed(format!("파일 읽기 실패: {e}")),
    };
    if bytes.len() <= 44 {
        return MicTest::Failed("녹음 파일이 비어있음".into());
    }
    let samples: Vec<f64> = bytes[44..]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f64)
        .collect();
    let n = samples.len();
    let sr = 48000.0;
    let win = 12000; // 0.25초
    let mut powers: Vec<f64> = samples
        .chunks(win)
        .filter(|c| c.len() == win)
        .map(|c| goertzel(c, 1000.0, sr) / win as f64)
        .collect();
    if powers.len() < 8 || n < 48000 {
        return MicTest::Failed("녹음이 너무 짧음".into());
    }
    powers.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let k = powers.len();
    let hi: f64 = powers[k - 4..].iter().sum::<f64>() / 4.0;
    let lo: f64 = powers[..4].iter().sum::<f64>() / 4.0 + 1.0;
    let snr_db = 10.0 * (hi / lo).log10();
    let peak = samples.iter().fold(0f64, |m, &v| m.max(v.abs()));
    let rms = (samples.iter().map(|v| v * v).sum::<f64>() / n as f64).sqrt();
    MicTest::Loop {
        snr_db,
        peak_pct: peak / 32768.0 * 100.0,
        rms_pct: rms / 32768.0 * 100.0,
    }
}

async fn test_mic(target: Option<(String, String)>) -> MicTest {
    let _ = std::fs::remove_file(TEST_WAV);
    let r = match &target {
        Some((src, port)) => {
            if !port.is_empty() {
                let pr = runner::run("pactl", &["set-source-port", src, port]).await;
                if !pr.success {
                    return MicTest::Failed(format!("포트 전환 실패: {}", pr.output.trim()));
                }
            }
            // 특정 소스에서 직접 녹음 (기본 입력과 무관하게)
            runner::run_sh(&format!(
                "PULSE_SOURCE='{src}' arecord -D pulse -f S16_LE -r 48000 -c 1 -d 2 {TEST_WAV}"
            )).await
        }
        None => runner::run("arecord", &[
            "-D", "pulse", "-f", "S16_LE", "-r", "48000", "-c", "1", "-d", "2", TEST_WAV
        ]).await,
    };
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
