use super::ime::{TYPE_SCREEN_TITLE, TYPE_BODY, TYPE_CAPTION, TYPE_CHIP, FONT_BOLD, FONT_SEMIBOLD};
use iced::{
    widget::{column, container, qr_code, row, scrollable, text, Space},
    Element, Length, Task,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_BTN2, C_DIM, C_ERR, C_OK, C_TEXT, C_TEXT2, C_WARN};

/// 패치·설치 대상 flatpak 앱 ID (Weylus Community Edition).
const APPID: &str = "io.github.electronstudio.WeylusCommunityEdition";

// ─── 설정 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Encoder { Cpu, Vaapi, Nvenc }

impl Default for Encoder {
    fn default() -> Self { Encoder::Vaapi }
}

impl Encoder {
    /// GPU 인코더일 때 flatpak run 에 붙일 `--try-*` 플래그.
    fn flag(self) -> Option<&'static str> {
        match self {
            Encoder::Cpu => None,
            Encoder::Vaapi => Some("--try-vaapi"),
            Encoder::Nvenc => Some("--try-nvenc"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddressMode { Lan, Tailscale }

impl Default for AddressMode {
    fn default() -> Self { AddressMode::Lan }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TabletConfig {
    pub port: u16,
    pub access_code: String,
    pub encoder: Encoder,
    pub address: AddressMode,
}

impl Default for TabletConfig {
    fn default() -> Self {
        Self { port: 1701, access_code: String::new(), encoder: Encoder::Vaapi, address: AddressMode::Lan }
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("popmgr/tablet.json")
}

fn load_config() -> TabletConfig {
    let mut cfg: TabletConfig = std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if cfg.access_code.trim().is_empty() {
        cfg.access_code = random_access_code();
        save_config(&cfg);
    }
    cfg
}

fn save_config(cfg: &TabletConfig) {
    let path = config_path();
    if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(path, json);
    }
}

/// /dev/urandom(실패 시 시각) 기반 6자리 숫자 접속 코드.
fn random_access_code() -> String {
    let mut buf = [0u8; 4];
    let ok = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| { use std::io::Read; f.read_exact(&mut buf) })
        .is_ok();
    if !ok {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        buf = nanos.to_ne_bytes();
    }
    format!("{:06}", u32::from_ne_bytes(buf) % 1_000_000)
}

// ─── 경로 ────────────────────────────────────────────────────────────────

fn weylus_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("Applications/weylus-cosmic")
}
fn weylus_bin_path() -> PathBuf { weylus_dir().join("weylus") }
fn patch_marker_path() -> PathBuf { weylus_dir().join("patched-from.sha256") }
fn ffmpeg_lib_file() -> PathBuf { weylus_dir().join("ffmpeg/lib/libavcodec.so.62") }
fn log_path() -> PathBuf { dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("popmgr/weylus.log") }
fn pid_path() -> PathBuf { dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("popmgr/weylus.pid") }

// ─── 순수 함수 ───────────────────────────────────────────────────────────

/// BGRx→BGRA, RGBx→RGBA 바이트 치환. 정확히 5곳이 아니면 다른 버전으로 보고 실패시킨다
/// (엉뚱한 바이너리를 덮어쓰지 않도록 이 검사가 유일한 안전장치).
pub fn patch_bytes(input: &[u8]) -> Result<(Vec<u8>, usize), String> {
    let mut out = Vec::with_capacity(input.len());
    let mut count = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        if input[i..].starts_with(b"BGRx") {
            out.extend_from_slice(b"BGRA");
            count += 1;
            i += 4;
        } else if input[i..].starts_with(b"RGBx") {
            out.extend_from_slice(b"RGBA");
            count += 1;
            i += 4;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    if count != 5 {
        return Err(format!(
            "치환 수가 5가 아닙니다({count}개) — 다른 버전의 바이너리로 보여 저장하지 않습니다."
        ));
    }
    Ok((out, count))
}

/// flatpak run 인자 조립. encoder≠cpu 이고 GPU ffmpeg 가 있을 때만 env·--try-* 를 붙인다.
pub fn build_args(cfg: &TabletConfig, dir: &Path, gpu_ffmpeg_present: bool) -> Vec<String> {
    let dir_s = dir.display().to_string();
    let use_gpu = cfg.encoder != Encoder::Cpu && gpu_ffmpeg_present;

    let mut args = vec!["run".to_string(), format!("--filesystem={dir_s}:ro")];
    if use_gpu {
        args.push(format!("--env=LD_LIBRARY_PATH={dir_s}/ffmpeg/lib:/app/lib"));
    }
    args.push(format!("--command={dir_s}/weylus"));
    args.push(APPID.to_string());
    args.push("--no-gui".to_string());
    args.push("--auto-start".to_string());
    args.push("--wayland-support".to_string());
    args.push("--web-port".to_string());
    args.push(cfg.port.to_string());
    args.push("--access-code".to_string());
    args.push(cfg.access_code.clone());
    if use_gpu {
        if let Some(flag) = cfg.encoder.flag() { args.push(flag.to_string()); }
    }
    args
}

/// 접속 URL 조립.
pub fn build_url(ip: &str, port: u16, code: &str) -> String {
    format!("http://{ip}:{port}/?access_code={code}")
}

/// IPv4 를 /24 서브넷 표기(방화벽 규칙용)로 바꾼다.
pub fn subnet_24(ip: &str) -> Option<String> {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 { return None; }
    for p in &parts { p.parse::<u8>().ok()?; }
    Some(format!("{}.{}.{}.0/24", parts[0], parts[1], parts[2]))
}

fn parse_route_src(out: &str) -> Option<String> {
    let idx = out.find(" src ")?;
    out[idx + " src ".len()..].split_whitespace().next().map(|s| s.to_string())
}

fn parse_tailscale_ip(out: &str) -> Option<String> {
    out.lines().find_map(|l| {
        let rest = l.trim().strip_prefix("inet ")?;
        rest.split('/').next().map(|s| s.to_string())
    })
}

/// `ss -Htn state established '( sport = :PORT )'` 출력에서 마지막 컬럼(Peer)의 IP 만 뽑는다.
fn parse_ss_clients(out: &str) -> Vec<String> {
    out.lines().filter_map(|l| {
        let last = l.split_whitespace().last()?;
        if let Some(stripped) = last.strip_prefix('[') {
            stripped.split(']').next().map(|s| s.to_string())
        } else {
            last.rsplit_once(':').map(|(ip, _)| ip.to_string())
        }
    }).collect()
}

/// 로그 마지막 "Video: WxH@인코더 ..." 줄에서 인코더 이름(토큰 하나)만 뽑는다.
/// 실제 로그는 "Video: 1912x1074@h264_vaapi pix_fmt: vaapi (nv12)" 처럼 꼬리가 붙는다.
fn parse_current_encoder(log: &str) -> Option<String> {
    log.lines().rev().find_map(|l| {
        let idx = l.find("Video:")?;
        let at = l[idx..].rfind('@')?;
        l[idx + at + 1..].split_whitespace().next().map(|s| s.to_string())
    })
}

fn pid_alive(pid: u32) -> bool { Path::new(&format!("/proc/{pid}")).exists() }

fn comm_is_weylus(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).map(|s| s.trim() == "weylus").unwrap_or(false)
}

fn recorded_pid_if_alive() -> Option<u32> {
    let pid: u32 = std::fs::read_to_string(pid_path()).ok()?.trim().parse().ok()?;
    if pid_alive(pid) && comm_is_weylus(pid) { Some(pid) } else { None }
}

async fn pgrep_weylus_pids() -> Vec<u32> {
    // 패턴에 배포 경로를 포함시켜 이 명령을 grep 하는 자기 셸이 걸리지 않게 하고,
    // 그래도 남는 후보는 comm 이 정확히 weylus 인 것만 인정한다.
    let r = runner::run("pgrep", &["-f", "weylus-cosmic/weylus"]).await;
    r.output.lines().filter_map(|l| l.trim().parse::<u32>().ok()).filter(|pid| comm_is_weylus(*pid)).collect()
}

// 기록된 PID 와 pgrep 결과를 합친다. 중복은 제거하고 순서는 유지한다.
fn merge_pids(recorded: Option<u32>, found: &[u32]) -> Vec<u32> {
    let mut pids: Vec<u32> = Vec::new();
    for pid in recorded.into_iter().chain(found.iter().copied()) {
        if !pids.contains(&pid) { pids.push(pid); }
    }
    pids
}

// `ss -Hltn` 출력에 공백 아닌 줄이 하나라도 있으면 포트가 점유된 것이다.
fn port_in_use(ss_output: &str) -> bool {
    ss_output.lines().any(|l| !l.trim().is_empty())
}

// 최대 max_ms 동안 100ms 간격으로 pid 가 죽길 기다린다.
async fn wait_dead(pid: u32, max_ms: u64) -> bool {
    let mut waited = 0;
    while pid_alive(pid) && waited < max_ms {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        waited += 100;
    }
    !pid_alive(pid)
}

// ─── 상태 조회 ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct TabletStatus {
    pub installed: bool,
    pub install_dir: Option<String>,
    pub patched_exists: bool,
    pub patch_stale: bool,
    pub gpu_ffmpeg_present: bool,
    pub running: bool,
    pub running_pid: Option<u32>,
    pub clients: Vec<String>,
    pub ip_lan: Option<String>,
    pub ip_tailscale: Option<String>,
    pub current_encoder: Option<String>,
}

async fn flatpak_location() -> Option<String> {
    let loc = runner::run("flatpak", &["info", "--user", "--show-location", APPID]).await;
    if !loc.success { return None; }
    let dir = loc.output.trim();
    if dir.is_empty() { None } else { Some(dir.to_string()) }
}

async fn scan_status(cfg: &TabletConfig) -> TabletStatus {
    let mut st = TabletStatus::default();

    let info = runner::run("flatpak", &["info", "--user", APPID]).await;
    st.installed = info.success;
    if st.installed { st.install_dir = flatpak_location().await; }

    st.patched_exists = weylus_bin_path().exists();
    if st.patched_exists {
        if let Some(install_dir) = &st.install_dir {
            let orig = format!("{install_dir}/files/bin/weylus");
            let sum = runner::run("sha256sum", &[&orig]).await;
            let current = sum.output.split_whitespace().next().map(|s| s.to_string());
            let recorded = std::fs::read_to_string(patch_marker_path()).ok()
                .and_then(|s| s.split_whitespace().next().map(|s| s.to_string()));
            st.patch_stale = matches!((current, recorded), (Some(c), Some(r)) if c != r);
        }
    }

    st.gpu_ffmpeg_present = ffmpeg_lib_file().exists();

    st.running_pid = recorded_pid_if_alive();
    if st.running_pid.is_none() { st.running_pid = pgrep_weylus_pids().await.first().copied(); }
    st.running = st.running_pid.is_some();

    if st.running {
        let ss = runner::run_sh(&format!(
            "ss -Htn state established '( sport = :{} )' 2>/dev/null", cfg.port
        )).await;
        st.clients = parse_ss_clients(&ss.output);
    }

    let route = runner::run_sh("ip -4 route get 1.1.1.1 2>/dev/null").await;
    st.ip_lan = parse_route_src(&route.output);
    let ts = runner::run_sh("ip -4 addr show tailscale0 2>/dev/null").await;
    st.ip_tailscale = parse_tailscale_ip(&ts.output);

    let log = std::fs::read_to_string(log_path()).unwrap_or_default();
    st.current_encoder = parse_current_encoder(&log);

    st
}

// ─── 패치·시작·중지 ──────────────────────────────────────────────────────

async fn do_patch() -> CmdResult {
    let Some(install_dir) = flatpak_location().await else {
        return CmdResult { success: false, output: "flatpak 설치 위치를 찾지 못했습니다. 먼저 Weylus CE를 설치하세요.".into() };
    };
    let orig_path = format!("{install_dir}/files/bin/weylus");
    let bytes = match std::fs::read(&orig_path) {
        Ok(b) => b,
        Err(e) => return CmdResult { success: false, output: format!("원본 읽기 실패({orig_path}): {e}") },
    };
    let (patched, count) = match patch_bytes(&bytes) {
        Ok(v) => v,
        Err(e) => return CmdResult { success: false, output: format!("패치 실패: {e}") },
    };
    let dir = weylus_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return CmdResult { success: false, output: format!("디렉터리 생성 실패: {e}") };
    }
    let bin_path = weylus_bin_path();
    if let Err(e) = std::fs::write(&bin_path, &patched) {
        return CmdResult { success: false, output: format!("패치본 쓰기 실패: {e}") };
    }
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&bin_path, std::fs::Permissions::from_mode(0o755));
    }
    let sum = runner::run("sha256sum", &[orig_path.as_str()]).await;
    let hash = sum.output.split_whitespace().next().unwrap_or_default();
    if let Err(e) = std::fs::write(patch_marker_path(), format!("{hash}  {orig_path}\n")) {
        return CmdResult { success: false, output: format!("해시 기록 실패: {e}") };
    }
    CmdResult { success: true, output: format!("패치 완료 (치환 {count}곳): {}", bin_path.display()) }
}

async fn do_start(cfg: TabletConfig) -> CmdResult {
    if !weylus_bin_path().exists() {
        return CmdResult { success: false, output: "패치본이 없습니다. 먼저 패치를 실행하세요.".into() };
    }
    let alive = merge_pids(recorded_pid_if_alive(), &pgrep_weylus_pids().await);
    if !alive.is_empty() {
        let list = alive.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
        return CmdResult { success: false, output: format!("이미 실행 중입니다 (PID {list}). 먼저 중지하세요.") };
    }
    let ss = runner::run_sh(&format!("ss -Hltn '( sport = :{} )' 2>/dev/null", cfg.port)).await;
    if port_in_use(&ss.output) {
        return CmdResult { success: false, output: format!("포트 {} 을(를) 다른 프로세스가 사용 중입니다.", cfg.port) };
    }
    let dir = weylus_dir();
    let gpu = ffmpeg_lib_file().exists();
    let args = build_args(&cfg, &dir, gpu);

    let log_file = log_path();
    if let Some(parent) = log_file.parent() { let _ = std::fs::create_dir_all(parent); }
    let out_f = match std::fs::OpenOptions::new().create(true).append(true).open(&log_file) {
        Ok(f) => f,
        Err(e) => return CmdResult { success: false, output: format!("로그 파일 열기 실패: {e}") },
    };
    let err_f = match out_f.try_clone() {
        Ok(f) => f,
        Err(e) => return CmdResult { success: false, output: format!("로그 파일 복제 실패: {e}") },
    };

    // setsid 로 새 세션을 만들어 popmgr 를 종료해도 살아남게 한다.
    let spawned = std::process::Command::new("setsid")
        .arg("flatpak").args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(out_f).stderr(err_f)
        .spawn();
    match spawned {
        Ok(mut child) => {
            let pid = child.id();
            let _ = std::fs::write(pid_path(), pid.to_string());
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            if let Ok(Some(_)) = child.try_wait() {
                let _ = std::fs::remove_file(pid_path());
                return CmdResult { success: false, output: format!("Weylus 가 바로 종료됐습니다. 로그: {}", log_file.display()) };
            }
            CmdResult { success: true, output: format!("Weylus 시작됨 (PID {pid}, 포트 {})", cfg.port) }
        }
        Err(e) => CmdResult { success: false, output: format!("실행 실패: {e}") },
    }
}

async fn do_stop() -> CmdResult {
    let pids = merge_pids(recorded_pid_if_alive(), &pgrep_weylus_pids().await);
    if pids.is_empty() {
        return CmdResult { success: false, output: "실행 중인 Weylus 프로세스를 찾지 못했습니다.".into() };
    }
    for pid in &pids { let _ = runner::run("kill", &["-TERM", &pid.to_string()]).await; }
    let mut out = String::new();
    let mut ok = true;
    for pid in &pids {
        if wait_dead(*pid, 3000).await {
            out.push_str(&format!("PID {pid} 종료됨\n"));
            continue;
        }
        let _ = runner::run("kill", &["-KILL", &pid.to_string()]).await;
        if wait_dead(*pid, 1000).await {
            out.push_str(&format!("PID {pid} 응답 없어 강제 종료(SIGKILL)\n"));
        } else {
            ok = false;
            out.push_str(&format!("PID {pid} 종료 실패\n"));
        }
    }
    if ok { let _ = std::fs::remove_file(pid_path()); }
    CmdResult { success: ok, output: out }
}

// ─── CLI ─────────────────────────────────────────────────────────────────

fn tokio_rt() -> Option<tokio::runtime::Runtime> {
    match tokio::runtime::Runtime::new() {
        Ok(rt) => Some(rt),
        Err(e) => { eprintln!("런타임 생성 실패: {e}"); None }
    }
}

pub fn tablet_status_cli() -> i32 {
    let Some(rt) = tokio_rt() else { return 1 };
    let cfg = load_config();
    let status = rt.block_on(scan_status(&cfg));
    let v = serde_json::json!({
        "installed": status.installed,
        "install_dir": status.install_dir,
        "patched_exists": status.patched_exists,
        "patch_stale": status.patch_stale,
        "gpu_ffmpeg_present": status.gpu_ffmpeg_present,
        "running": status.running,
        "running_pid": status.running_pid,
        "clients": status.clients,
        "ip_lan": status.ip_lan,
        "ip_tailscale": status.ip_tailscale,
        "current_encoder": status.current_encoder,
        "port": cfg.port,
        "encoder": cfg.encoder,
        "address": cfg.address,
    });
    println!("{v}");
    0
}

pub fn tablet_start_cli() -> i32 {
    let Some(rt) = tokio_rt() else { return 1 };
    let r = rt.block_on(do_start(load_config()));
    println!("{}", r.output.trim());
    if r.success { 0 } else { 1 }
}

pub fn tablet_stop_cli() -> i32 {
    let Some(rt) = tokio_rt() else { return 1 };
    let r = rt.block_on(do_stop());
    println!("{}", r.output.trim());
    if r.success { 0 } else { 1 }
}

// ─── iced 탭 ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum TabletMsg {
    Refresh,
    Refreshed(TabletStatus),
    Patch,
    Patched(CmdResult),
    Start,
    Started(CmdResult),
    Stop,
    Stopped(CmdResult),
    SelectEncoder(Encoder),
    ApplyEncoder,
    EncoderApplied(CmdResult),
    SetAddress(AddressMode),
    RegenerateCode,
    CopyUrl,
    Firewall,
    FirewallDone(CmdResult),
}

pub struct TabletState {
    cfg: TabletConfig,
    status: Option<TabletStatus>,
    running: Option<String>,
    qr: Option<qr_code::Data>,
    // 버튼으로 고른 인코더(설명 표시용). "적용" 을 눌러야 cfg.encoder 에 저장·재시작된다.
    pending_encoder: Encoder,
}

impl TabletState {
    pub fn new() -> Self {
        let cfg = load_config();
        let pending_encoder = cfg.encoder;
        let mut s = Self { cfg, status: None, running: None, qr: None, pending_encoder };
        s.refresh_qr();
        s
    }

    fn refresh_qr(&mut self) {
        self.qr = qr_code::Data::new(self.access_url()).ok();
    }

    fn current_ip(&self) -> Option<String> {
        let st = self.status.as_ref()?;
        match self.cfg.address {
            AddressMode::Lan => st.ip_lan.clone(),
            AddressMode::Tailscale => st.ip_tailscale.clone(),
        }
    }

    fn access_url(&self) -> String {
        let ip = self.current_ip().unwrap_or_else(|| "0.0.0.0".to_string());
        build_url(&ip, self.cfg.port, &self.cfg.access_code)
    }

    fn trigger_refresh(&self) -> Task<TabletMsg> {
        let cfg = self.cfg.clone();
        Task::perform(async move { scan_status(&cfg).await }, TabletMsg::Refreshed)
    }

    pub fn update(&mut self, msg: TabletMsg) -> (Task<TabletMsg>, Option<CmdResult>) {
        match msg {
            TabletMsg::Refresh => (self.trigger_refresh(), None),
            TabletMsg::Refreshed(s) => {
                self.status = Some(s);
                self.refresh_qr();
                (Task::none(), None)
            }
            TabletMsg::Patch => {
                self.running = Some("패치본 만드는 중...".into());
                (Task::perform(async { do_patch().await }, TabletMsg::Patched), None)
            }
            TabletMsg::Patched(r) => { self.running = None; (self.trigger_refresh(), Some(r)) }
            TabletMsg::Start => {
                self.running = Some("Weylus 시작 중...".into());
                let cfg = self.cfg.clone();
                (Task::perform(async move { do_start(cfg).await }, TabletMsg::Started), None)
            }
            TabletMsg::Started(r) => { self.running = None; (self.trigger_refresh(), Some(r)) }
            TabletMsg::Stop => {
                self.running = Some("Weylus 중지 중...".into());
                (Task::perform(async { do_stop().await }, TabletMsg::Stopped), None)
            }
            TabletMsg::Stopped(r) => { self.running = None; (self.trigger_refresh(), Some(r)) }
            TabletMsg::SelectEncoder(e) => {
                self.pending_encoder = e;
                (Task::none(), None)
            }
            TabletMsg::ApplyEncoder => {
                self.cfg.encoder = self.pending_encoder;
                save_config(&self.cfg);
                let was_running = self.status.as_ref().is_some_and(|s| s.running);
                if !was_running {
                    let msg = format!("인코더를 {} 로 저장했습니다. 다음 시작부터 적용됩니다.", encoder_label(self.cfg.encoder));
                    return (Task::none(), Some(CmdResult { success: true, output: msg }));
                }
                self.running = Some("인코더 적용 중... (Weylus 재시작)".into());
                let cfg = self.cfg.clone();
                (Task::perform(async move { restart_weylus(cfg).await }, TabletMsg::EncoderApplied), None)
            }
            TabletMsg::EncoderApplied(r) => { self.running = None; (self.trigger_refresh(), Some(r)) }
            TabletMsg::SetAddress(a) => {
                self.cfg.address = a;
                save_config(&self.cfg);
                self.refresh_qr();
                (Task::none(), None)
            }
            TabletMsg::RegenerateCode => {
                self.cfg.access_code = random_access_code();
                save_config(&self.cfg);
                self.refresh_qr();
                let msg = "접속 코드를 새로 만들었습니다. 실행 중이면 재시작해야 반영됩니다.".into();
                (Task::none(), Some(CmdResult { success: true, output: msg }))
            }
            TabletMsg::CopyUrl => (iced::clipboard::write(self.access_url()), None),
            TabletMsg::Firewall => {
                let lan = self.status.as_ref().and_then(|s| s.ip_lan.as_deref());
                let Some(subnet) = lan.and_then(subnet_24) else {
                    let out = "LAN IP 를 확인하지 못해 서브넷을 계산할 수 없습니다.".into();
                    return (Task::none(), Some(CmdResult { success: false, output: out }));
                };
                self.running = Some("방화벽 규칙 추가 중... (관리자 인증)".into());
                let script = format!(
                    "pkexec bash -c 'ufw allow from {subnet} to any port {} proto tcp'", self.cfg.port
                );
                (Task::perform(async move { runner::run_sh(&script).await }, TabletMsg::FirewallDone), None)
            }
            TabletMsg::FirewallDone(r) => { self.running = None; (Task::none(), Some(r)) }
        }
    }

    pub fn view(&self) -> Element<'_, TabletMsg> {
        let mut col = column![
            text("아이패드").size(TYPE_SCREEN_TITLE).font(FONT_BOLD),
            Space::with_height(6),
            text("Weylus(COSMIC 패치판)로 아이패드를 보조 화면 + 터치 입력으로 씁니다.")
                .size(TYPE_CAPTION).color(C_DIM),
            Space::with_height(16),
        ];

        if let Some(label) = &self.running {
            col = col.push(running_bar(label)).push(Space::with_height(10));
        }

        let Some(st) = self.status.as_ref() else {
            col = col.push(text("상태 확인 중...").size(TYPE_BODY).color(C_DIM));
            return scrollable(container(col).padding([4, 0])).into();
        };

        let busy = self.running.is_some();

        col = col.push(install_card(st, busy));
        col = col.push(Space::with_height(10));

        if st.patched_exists && !st.patch_stale {
            col = col.push(connection_card(self, st, busy));
            col = col.push(Space::with_height(10));
            col = col.push(encoder_card(self, st, busy));
            col = col.push(Space::with_height(10));
            col = col.push(firewall_card(st, busy));
            col = col.push(Space::with_height(10));
        }

        col = col.push(tips_card());
        col = col.push(Space::with_height(10));
        col = col.push(row![
            Space::with_width(Length::Fill),
            action_btn("새로고침", TabletMsg::Refresh, !busy, C_BTN2),
        ]);

        scrollable(container(col).padding([4, 0])).into()
    }
}

fn install_card(st: &TabletStatus, busy: bool) -> Element<'_, TabletMsg> {
    let (status_txt, status_col): (String, iced::Color) = if !st.installed {
        ("○ Weylus CE 미설치 — flatpak install 로 먼저 설치하세요".into(), C_ERR)
    } else if st.patch_stale {
        ("⚠ flatpak 이 업데이트됨 — 다시 패치가 필요합니다".into(), C_WARN)
    } else if st.patched_exists {
        ("● 패치본 준비됨".into(), C_OK)
    } else {
        ("○ 패치본 없음 — 아래에서 패치를 실행하세요".into(), C_WARN)
    };

    let mut body = column![
        text("설치 · 패치 상태").size(TYPE_BODY).font(FONT_SEMIBOLD),
        Space::with_height(6),
        text(status_txt).size(TYPE_CAPTION).color(status_col),
    ];
    if let Some(dir) = &st.install_dir {
        body = body.push(Space::with_height(4));
        body = body.push(text(format!("설치 위치: {dir}")).size(TYPE_CHIP).color(C_DIM));
    }
    body = body.push(Space::with_height(4));
    let gpu_txt = if st.gpu_ffmpeg_present {
        "GPU 인코딩 ffmpeg 라이브러리 있음"
    } else {
        "GPU 인코딩 ffmpeg 라이브러리 없음 (CPU 인코딩만 가능)"
    };
    body = body.push(text(gpu_txt).size(TYPE_CHIP).color(C_DIM));

    body = body.push(Space::with_height(10));
    let btn_label = if st.patched_exists { "다시 패치" } else { "패치본 만들기" };
    body = body.push(row![
        Space::with_width(Length::Fill),
        action_btn(btn_label, TabletMsg::Patch, !busy && st.installed, C_BLUE),
    ]);

    card(body)
}

fn connection_card<'a>(state: &'a TabletState, st: &'a TabletStatus, busy: bool) -> Element<'a, TabletMsg> {
    let ip_known = state.current_ip().is_some();
    let url = state.access_url();
    let toggle_label = if state.cfg.address == AddressMode::Lan { "LAN" } else { "Tailscale" };
    let toggle_target = match state.cfg.address {
        AddressMode::Lan => AddressMode::Tailscale,
        AddressMode::Tailscale => AddressMode::Lan,
    };

    let mut body = column![
        row![
            text("접속").size(TYPE_BODY).font(FONT_SEMIBOLD),
            Space::with_width(Length::Fill),
            action_btn(toggle_label, TabletMsg::SetAddress(toggle_target), !busy, C_BTN2),
        ].align_y(iced::Alignment::Center),
        Space::with_height(8),
    ];

    if !ip_known {
        body = body.push(text("현재 주소 모드의 IP 를 확인하지 못했습니다.").size(TYPE_CAPTION).color(C_WARN));
        body = body.push(Space::with_height(8));
    }

    body = body.push(text(url).size(TYPE_BODY).color(C_TEXT));
    body = body.push(Space::with_height(10));

    let qr_element: Element<'a, TabletMsg> = match &state.qr {
        Some(data) => qr_code(data).into(),
        None => text("QR 코드를 생성하지 못했습니다.").size(TYPE_CAPTION).color(C_DIM).into(),
    };
    body = body.push(container(qr_element).padding(8));
    body = body.push(Space::with_height(8));

    body = body.push(row![
        action_btn("주소 복사", TabletMsg::CopyUrl, true, C_BTN2),
        Space::with_width(8),
        action_btn("접속 코드 새로 만들기", TabletMsg::RegenerateCode, !busy, C_BTN2),
    ]);
    body = body.push(Space::with_height(10));

    if st.running {
        let pid_txt = st.running_pid.map(|p| p.to_string()).unwrap_or_default();
        body = body.push(text(format!("실행 중 (PID {pid_txt})")).size(TYPE_CAPTION).color(C_OK));
        if let Some(enc) = &st.current_encoder {
            body = body.push(text(format!("현재 인코더: {enc}")).size(TYPE_CAPTION).color(C_DIM));
        }
        let clients_txt = if st.clients.is_empty() {
            "접속 중인 클라이언트 없음".to_string()
        } else {
            format!("접속 중: {}", st.clients.join(", "))
        };
        body = body.push(text(clients_txt).size(TYPE_CAPTION).color(C_TEXT));
        body = body.push(Space::with_height(8));
        body = body.push(row![Space::with_width(Length::Fill), action_btn("중지", TabletMsg::Stop, !busy, C_ERR)]);
    } else {
        body = body.push(row![Space::with_width(Length::Fill), action_btn("시작", TabletMsg::Start, !busy, C_BLUE)]);
    }

    card(body)
}

fn encoder_label(e: Encoder) -> &'static str {
    match e {
        Encoder::Cpu => "CPU",
        Encoder::Vaapi => "Intel GPU (VAAPI)",
        Encoder::Nvenc => "NVIDIA (NVENC)",
    }
}

/// 인코더별 설명 줄. 이 노트북은 냉각이 약해(스로틀 잦음) 발열을 1순위 기준으로 설명한다.
pub fn encoder_desc(e: Encoder) -> &'static [&'static str] {
    match e {
        Encoder::Cpu => &[
            "메인 프로세서가 직접 압축합니다(x264). 어디서나 동작하는 대신 가장 무겁습니다.",
            "발열: 높음 — 이 노트북 실측 CPU 123~145%. 열이 쌓이면 스로틀로 전체가 느려지고 화면도 끊깁니다.",
            "GPU 방식이 검은 화면·오류일 때만 쓰세요.",
        ],
        Encoder::Vaapi => &[
            "내장 인텔 그래픽(UHD 630)의 압축 전용 회로가 처리합니다. 화면을 그리는 칩과 같아 가장 효율적입니다.",
            "발열: 낮음 — 이 노트북 실측 CPU 60%(CPU 방식의 절반 이하). 남은 몫은 화면 복사·색 변환입니다.",
            "추천 — 이 노트북 기본값.",
        ],
        Encoder::Nvenc => &[
            "외장 GTX 1050 Ti 의 압축 회로가 처리합니다. CPU 는 가볍지만 외장 GPU 가 깨어나 전력·발열이 늘어납니다.",
            "화면은 인텔 쪽에 있어 두 칩 사이 복사가 더해집니다. 이 노트북에선 실기 확인 전입니다.",
            "실패하면 Weylus 가 CPU 로 자동 전환됩니다. 특별한 이유가 없으면 VAAPI 를 쓰세요.",
        ],
    }
}

async fn restart_weylus(cfg: TabletConfig) -> CmdResult {
    let stop = do_stop().await;
    // "찾지 못함" 은 원래 안 돌던 경우라 계속 진행하고, 그 외 실패는 start 하지 않는다.
    if !stop.success && !stop.output.contains("찾지 못했습니다") {
        return CmdResult { success: false, output: stop.output.trim().to_string() };
    }
    let start = do_start(cfg).await;
    let output = format!("{}\n{}\n아이패드에서 새로고침 후 공유를 다시 허용하세요.", stop.output.trim(), start.output.trim());
    CmdResult { success: start.success, output }
}

fn encoder_button(target: Encoder, selected: Encoder, applied: Encoder, enabled: bool) -> Element<'static, TabletMsg> {
    let color = if target == selected { C_BLUE } else { C_BTN2 };
    let label = if target == applied { format!("{} · 사용 중", encoder_label(target)) } else { encoder_label(target).into() };
    action_btn(&label, TabletMsg::SelectEncoder(target), enabled, color)
}

fn encoder_card<'a>(state: &'a TabletState, st: &'a TabletStatus, busy: bool) -> Element<'a, TabletMsg> {
    let gpu = st.gpu_ffmpeg_present;
    let (sel, applied) = (state.pending_encoder, state.cfg.encoder);
    let mut body = column![
        text("인코더").size(TYPE_BODY).font(FONT_SEMIBOLD),
        Space::with_height(4),
        text("아이패드로 보낼 화면을 영상으로 압축하는 방식입니다. 화질은 같고, 어느 칩이 일하느냐(=발열)가 다릅니다.")
            .size(TYPE_CAPTION).color(C_DIM),
        Space::with_height(10),
        row![
            encoder_button(Encoder::Cpu, sel, applied, !busy),
            Space::with_width(8),
            encoder_button(Encoder::Vaapi, sel, applied, !busy && gpu),
            Space::with_width(8),
            encoder_button(Encoder::Nvenc, sel, applied, !busy && gpu),
        ],
        Space::with_height(12),
        text(encoder_label(sel)).size(TYPE_CAPTION).font(FONT_SEMIBOLD).color(C_TEXT),
        Space::with_height(4),
    ];
    for line in encoder_desc(sel) {
        body = body.push(text(*line).size(TYPE_CAPTION).color(C_TEXT2));
    }
    if !gpu {
        body = body.push(Space::with_height(8));
        body = body.push(text("GPU 인코딩 라이브러리 없음 — CPU 만 쓸 수 있습니다.").size(TYPE_CAPTION).color(C_DIM));
    }
    let apply_label = if st.running { "적용 (Weylus 재시작)" } else { "적용" };
    body = body.push(Space::with_height(10));
    body = body.push(row![
        Space::with_width(Length::Fill),
        action_btn(apply_label, TabletMsg::ApplyEncoder, !busy && sel != applied, C_BLUE),
    ]);
    card(body)
}

fn firewall_card(st: &TabletStatus, busy: bool) -> Element<'_, TabletMsg> {
    let subnet = st.ip_lan.as_deref().and_then(subnet_24);
    let mut body = column![
        text("방화벽").size(TYPE_BODY).font(FONT_SEMIBOLD),
        Space::with_height(6),
    ];
    let info = match &subnet {
        Some(s) => format!("LAN 대역 {s} 의 TCP 접속을 허용합니다 (ufw)."),
        None => "LAN IP 를 확인하지 못해 서브넷을 계산할 수 없습니다.".to_string(),
    };
    body = body.push(text(info).size(TYPE_CAPTION).color(C_DIM));
    body = body.push(Space::with_height(10));
    body = body.push(row![
        Space::with_width(Length::Fill),
        action_btn("방화벽 허용 (pkexec)", TabletMsg::Firewall, !busy && subnet.is_some(), C_BLUE),
    ]);
    card(body)
}

fn tips_card<'a>() -> Element<'a, TabletMsg> {
    card(column![
        text("사용 팁").size(TYPE_BODY).font(FONT_SEMIBOLD),
        Space::with_height(6),
        text("공유 화면을 고를 때는 \"창\"이 아니라 \"화면(모니터)\"을 선택하세요 — 창이면 터치 좌표가 어긋납니다.")
            .size(TYPE_CAPTION).color(C_DIM),
        Space::with_height(4),
        text("아이패드 Safari 에서 공유 → 홈 화면에 추가하면 전체 화면으로 뜹니다.")
            .size(TYPE_CAPTION).color(C_DIM),
        Space::with_height(4),
        text("아이패드 자동 잠금을 \"안 함\"으로 두세요 — 잠기면 연결이 끊깁니다.")
            .size(TYPE_CAPTION).color(C_DIM),
        Space::with_height(4),
        text("소리는 전송되지 않습니다(화면·터치만 전달됩니다).").size(TYPE_CAPTION).color(C_DIM),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_pids_dedups_and_keeps_order() {
        assert_eq!(merge_pids(None, &[]), Vec::<u32>::new());
        assert_eq!(merge_pids(Some(5), &[5, 7]), vec![5, 7]);
        assert_eq!(merge_pids(Some(9), &[5]), vec![9, 5]);
    }

    #[test]
    fn port_in_use_needs_non_blank_line() {
        assert!(!port_in_use(""));
        assert!(!port_in_use("\n  \n"));
        assert!(port_in_use("LISTEN 0 128 0.0.0.0:1701 0.0.0.0:*\n"));
    }

    #[test]
    fn patch_bytes_replaces_five_occurrences() {
        let buf = b"header BGRx caps RGBx more BGRx tail RGBx last BGRx end".to_vec();
        let (out, count) = patch_bytes(&buf).unwrap();
        assert_eq!(count, 5);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("BGRx").count(), 0);
        assert_eq!(text.matches("RGBx").count(), 0);
        assert_eq!(text.matches("BGRA").count(), 3);
        assert_eq!(text.matches("RGBA").count(), 2);
    }

    #[test]
    fn patch_bytes_rejects_four_occurrences() {
        let buf = b"BGRx RGBx BGRx RGBx".to_vec();
        assert!(patch_bytes(&buf).is_err());
    }

    #[test]
    fn patch_bytes_rejects_already_patched() {
        let buf = b"BGRA RGBA BGRA RGBA BGRA".to_vec();
        assert!(patch_bytes(&buf).is_err());
    }

    #[test]
    fn build_args_cpu_has_no_gpu_flags() {
        let cfg = TabletConfig {
            port: 1701, access_code: "123456".into(), encoder: Encoder::Cpu, address: AddressMode::Lan,
        };
        let args = build_args(&cfg, Path::new("/home/x/Applications/weylus-cosmic"), true);
        assert!(!args.iter().any(|a| a.starts_with("--env=")));
        assert!(!args.iter().any(|a| a.starts_with("--try-")));
        assert!(args.contains(&"1701".to_string()));
        assert!(args.contains(&"123456".to_string()));
    }

    #[test]
    fn build_args_vaapi_with_ffmpeg_adds_env_and_try_flag() {
        let cfg = TabletConfig { encoder: Encoder::Vaapi, ..TabletConfig::default() };
        let dir = Path::new("/home/x/Applications/weylus-cosmic");
        let args = build_args(&cfg, dir, true);
        let want_env = "--env=LD_LIBRARY_PATH=/home/x/Applications/weylus-cosmic/ffmpeg/lib:/app/lib";
        assert!(args.iter().any(|a| a == want_env));
        assert!(args.contains(&"--try-vaapi".to_string()));
    }

    #[test]
    fn build_args_vaapi_without_ffmpeg_has_no_gpu_args() {
        let cfg = TabletConfig { encoder: Encoder::Vaapi, ..TabletConfig::default() };
        let args = build_args(&cfg, Path::new("/x"), false);
        assert!(!args.iter().any(|a| a.starts_with("--env=")));
        assert!(!args.contains(&"--try-vaapi".to_string()));
    }

    #[test]
    fn config_round_trip() {
        let cfg = TabletConfig {
            port: 9999, access_code: "654321".into(), encoder: Encoder::Nvenc, address: AddressMode::Tailscale,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: TabletConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.port, 9999);
        assert_eq!(back.encoder, Encoder::Nvenc);
        assert_eq!(back.address, AddressMode::Tailscale);
    }

    #[test]
    fn encoder_desc_mentions_heat_and_recommends_vaapi() {
        for e in [Encoder::Cpu, Encoder::Vaapi, Encoder::Nvenc] {
            assert!(!encoder_desc(e).is_empty());
            assert!(encoder_desc(e).iter().any(|l| l.contains("발열")), "{e:?} 설명에 발열 기준이 없음");
        }
        assert!(encoder_desc(Encoder::Vaapi).iter().any(|l| l.contains("추천")));
    }

    #[test]
    fn config_missing_fields_use_defaults() {
        let cfg: TabletConfig = serde_json::from_str(r#"{"port": 1234}"#).unwrap();
        assert_eq!(cfg.port, 1234);
        assert_eq!(cfg.encoder, Encoder::Vaapi);
        assert_eq!(cfg.address, AddressMode::Lan);
        assert_eq!(cfg.access_code, "");
    }

    #[test]
    fn url_assembly_has_access_code_query() {
        assert_eq!(build_url("192.168.1.20", 1701, "123456"), "http://192.168.1.20:1701/?access_code=123456");
    }

    #[test]
    fn subnet_24_from_ipv4() {
        assert_eq!(subnet_24("192.168.1.42"), Some("192.168.1.0/24".to_string()));
        assert_eq!(subnet_24("not-an-ip"), None);
    }

    #[test]
    fn parse_route_src_extracts_ip() {
        let out = "1.1.1.1 via 192.168.1.1 dev wlan0 src 192.168.1.42 uid 1000";
        assert_eq!(parse_route_src(out), Some("192.168.1.42".to_string()));
        assert_eq!(parse_route_src("no match here"), None);
    }

    #[test]
    fn parse_tailscale_ip_extracts_addr() {
        let out = "3: tailscale0: <POINTOPOINT,...>\n    inet 100.64.0.5/32 scope global tailscale0\n";
        assert_eq!(parse_tailscale_ip(out), Some("100.64.0.5".to_string()));
        assert_eq!(parse_tailscale_ip("no interface"), None);
    }

    #[test]
    fn parse_ss_clients_extracts_peer_ip() {
        let out = "ESTAB 0 0 192.168.1.20:1701 192.168.1.30:54321\n";
        assert_eq!(parse_ss_clients(out), vec!["192.168.1.30".to_string()]);
    }

    #[test]
    fn parse_current_encoder_finds_last_video_line() {
        let log = "startup\nVideo: 1920x1080@libx264\nother\nVideo: 1920x1080@h264_vaapi\n";
        assert_eq!(parse_current_encoder(log), Some("h264_vaapi".to_string()));
        assert_eq!(parse_current_encoder("no video line"), None);
    }

    #[test]
    fn parse_current_encoder_strips_trailing_pix_fmt_detail() {
        let log = "Video: 1912x1074@h264_vaapi pix_fmt: vaapi (nv12)\n";
        assert_eq!(parse_current_encoder(log), Some("h264_vaapi".to_string()));
    }
}
