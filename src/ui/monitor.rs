use super::ime::{TYPE_SCREEN_TITLE, TYPE_BODY, TYPE_CAPTION, FONT_BOLD, FONT_SEMIBOLD};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iced::{
    widget::{column, container, row, text, Space},
    Element, Length, Task
};
use serde::{Deserialize, Serialize};

use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_DIM, C_ERR, C_OK, C_TEXT, C_WARN};

pub const TURBO_OFF_TEMP_C: f32 = 90.0;
pub const TURBO_ON_TEMP_C: f32 = 78.0;
pub const COOL_HOLD_SECS: u64 = 60;

const TURBO_HELPER: &str = "/usr/local/bin/popmgr-turbo";
const TURBO_HELPER_SCRIPT: &str = r#"#!/bin/bash
set -euo pipefail
case "${1:-}" in
  on) v_pstate=0; v_boost=1 ;;
  off) v_pstate=1; v_boost=0 ;;
  *) echo "usage: popmgr-turbo on|off" >&2; exit 2 ;;
esac
if [ -w /sys/devices/system/cpu/intel_pstate/no_turbo ]; then echo "$v_pstate" > /sys/devices/system/cpu/intel_pstate/no_turbo
elif [ -w /sys/devices/system/cpu/cpufreq/boost ]; then echo "$v_boost" > /sys/devices/system/cpu/cpufreq/boost
else echo "turbo control not available" >&2; exit 1; fi
if [ -r /sys/devices/system/cpu/intel_pstate/no_turbo ]; then cat /sys/devices/system/cpu/intel_pstate/no_turbo
else cat /sys/devices/system/cpu/cpufreq/boost; fi
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurboAction {
    None,
    TurnOff,
    TurnOn
}

pub fn turbo_decision(
    temp_c: f32,
    turbo_off_by_us: bool,
    cool_since: Option<Instant>,
    now: Instant
) -> (TurboAction, Option<Instant>) {
    if temp_c >= TURBO_OFF_TEMP_C && !turbo_off_by_us {
        return (TurboAction::TurnOff, None);
    }
    if turbo_off_by_us && temp_c <= TURBO_ON_TEMP_C {
        let since = cool_since.unwrap_or(now);
        if now.duration_since(since) >= Duration::from_secs(COOL_HOLD_SECS) {
            (TurboAction::TurnOn, Some(since))
        } else {
            (TurboAction::None, Some(since))
        }
    } else if turbo_off_by_us {
        (TurboAction::None, None)
    } else {
        (TurboAction::None, cool_since)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MonitorSaved {
    protect: bool,
    turbo_off_by_popmgr: bool
}
impl Default for MonitorSaved {
    fn default() -> Self {
        Self {
            protect: false,
            turbo_off_by_popmgr: false
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HwSample {
    pub package_temp: Option<f32>,
    pub core_max_temp: Option<f32>,
    pub cpu_usage: Option<f32>,
    pub clock_mhz: Option<f32>,
    pub turbo_disabled: Option<bool>,
    pub throttle_count: Option<u64>,
    pub throttle_per_sec: Option<f32>,
    pub fan_rpm: Option<u64>,
    pub profile: Option<String>,
    pub nvme_temp: Option<f32>,
    pub disk_usage: Option<f32>,
    pub io_psi: Option<f32>,
    pub mem_used_gib: Option<f32>,
    pub mem_total_gib: Option<f32>,
    pub mem_percent: Option<f32>,
    pub swap_used_gib: Option<f32>,
    pub tmem_temp: Option<f32>,
    pub mem_psi: Option<f32>,
    pub gpu_sleeping: Option<bool>,
    pub gpu_temp: Option<f32>,
    pub gpu_usage: Option<f32>,
    cpu: Option<CpuCounters>,
    disk_io_ms: Option<u64>,
    sampled_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
struct CpuCounters {
    total: u64,
    idle: u64
}

#[derive(Debug, Clone)]
pub enum MonitorMsg {
    Tick,
    Sampled(HwSample),
    ToggleProtect,
    TurnOffNow,
    TurnOnNow,
    TurboDone(TurboAction, CmdResult),
    InstallHelper,
    InstallFinished(CmdResult),
    HelperChecked(bool)
}

pub struct MonitorState {
    pub sample: Option<HwSample>,
    pub running: Option<String>,
    pub helper_installed: bool,
    saved: MonitorSaved,
    cool_since: Option<Instant>,
    retry_after: Option<Instant>,
    last_cpu: Option<CpuCounters>,
    last_disk: Option<(u64, Instant)>,
    root_disk: Option<String>,
    sample_count: u64,
    last_action: Option<String>,
    last_throttle: Option<(u64, Instant)>,
}

impl MonitorState {
    pub fn new() -> Self {
        let saved = load_saved();
        let root_disk = root_disk_name();
        Self { sample: None, running: None, helper_installed: false, saved, cool_since: None, retry_after: None, last_cpu: None, last_disk: None, root_disk, sample_count: 0, last_action: None, last_throttle: None }
    }
    pub fn protect_enabled(&self) -> bool { self.saved.protect }
    pub fn update(&mut self, msg: MonitorMsg) -> (Task<MonitorMsg>, Option<CmdResult>) {
        match msg {
            MonitorMsg::Tick => {
                self.sample_count += 1;
                let disk = self.root_disk.clone(); let profile = self.sample_count % 10 == 0;
                (Task::perform(async move { sample_hw_with(disk.as_deref(), profile).await }, MonitorMsg::Sampled), None)
            }
            MonitorMsg::Sampled(mut s) => {
                let now = s.sampled_at.unwrap_or_else(Instant::now);
                if let (Some(prev), Some(cur)) = (self.last_cpu, s.cpu) { s.cpu_usage = cpu_usage(prev, cur); }
                self.last_cpu = s.cpu;
                if let (Some((old, at)), Some(cur)) = (self.last_disk, s.disk_io_ms) { s.disk_usage = disk_usage(old, cur, now.duration_since(at)); }
                self.last_disk = s.disk_io_ms.map(|v| (v, now));
                if let (Some((old, at)), Some(cur)) = (self.last_throttle, s.throttle_count) {
                    let secs = now.duration_since(at).as_secs_f32();
                    if secs > 0.0 { s.throttle_per_sec = Some(cur.saturating_sub(old) as f32 / secs); }
                }
                self.last_throttle = s.throttle_count.map(|v| (v, now));
                if s.profile.is_none() { s.profile = self.sample.as_ref().and_then(|x| x.profile.clone()); }
                let temp = s.package_temp; self.sample = Some(s);
                if self.saved.protect && self.helper_installed && self.retry_after.is_none_or(|t| now >= t) {
                    if let Some(t) = temp {
                        let (action, cool) = turbo_decision(t, self.saved.turbo_off_by_popmgr, self.cool_since, now);
                        self.cool_since = cool;
                        if action != TurboAction::None { return self.start_turbo(action, format!("{t:.0}°C 감지")); }
                    }
                }
                (Task::none(), None)
            }
            MonitorMsg::ToggleProtect => { self.saved.protect = !self.saved.protect; save_saved(&self.saved); (Task::none(), None) }
            MonitorMsg::TurnOffNow => self.start_turbo(TurboAction::TurnOff, "수동 요청".into()),
            MonitorMsg::TurnOnNow => self.start_turbo(TurboAction::TurnOn, "수동 요청".into()),
            MonitorMsg::TurboDone(action, r) => {
                self.running = None;
                if r.success {
                    self.saved.turbo_off_by_popmgr = action == TurboAction::TurnOff;
                    self.cool_since = None; save_saved(&self.saved);
                    self.last_action = Some(match action { TurboAction::TurnOff => "터보 차단 완료".into(), TurboAction::TurnOn => "터보 복원 완료".into(), TurboAction::None => String::new() });
                } else {
                    self.retry_after = Some(Instant::now() + Duration::from_secs(60));
                    if r.output.to_lowercase().contains("password") { self.helper_installed = false; self.last_action = Some("권한 설정 필요".into()); }
                }
                (Task::none(), Some(r))
            }
            MonitorMsg::InstallHelper => {
                self.running = Some("권한 헬퍼 설치 중 (암호 입력)...".into());
                let user = std::env::var("USER").unwrap_or_else(|_| "dell".into()); let b64 = base64_encode(TURBO_HELPER_SCRIPT.as_bytes());
                let script = format!("pkexec bash -c 'set -e; echo {b64} | base64 -d > {TURBO_HELPER}; chown root:root {TURBO_HELPER}; chmod 0755 {TURBO_HELPER}; printf \"%s ALL=(root) NOPASSWD: {TURBO_HELPER}\\n\" {user} > /etc/sudoers.d/popmgr-turbo; chmod 0440 /etc/sudoers.d/popmgr-turbo; visudo -cf /etc/sudoers.d/popmgr-turbo'");
                (Task::perform(async move { runner::run_sh(&script).await }, MonitorMsg::InstallFinished), None)
            }
            MonitorMsg::InstallFinished(result) => {
                if !result.success { self.running = None; return (Task::none(), Some(result)); }
                (Task::perform(async { helper_ok().await }, MonitorMsg::HelperChecked), None)
            }
            MonitorMsg::HelperChecked(ok) => { self.running = None; self.helper_installed = ok; (Task::none(), Some(CmdResult { success: ok, output: if ok { "터보 권한 헬퍼 설치됨".into() } else { "터보 권한 헬퍼 설치/확인 실패".into() } })) }
        }
    }
    fn start_turbo(&mut self, action: TurboAction, reason: String) -> (Task<MonitorMsg>, Option<CmdResult>) {
        self.running = Some(if action == TurboAction::TurnOff { "터보 차단 중...".into() } else { "터보 복원 중...".into() });
        self.last_action = Some(format!("{reason} → {}", if action == TurboAction::TurnOff { "터보 차단" } else { "터보 복원" }));
        (Task::perform(async move { (action, run_turbo(action).await) }, |(a, r)| MonitorMsg::TurboDone(a, r)), None)
    }
    pub fn initial_check() -> Task<MonitorMsg> { Task::perform(async { helper_ok().await }, MonitorMsg::HelperChecked) }
    pub fn view(&self) -> Element<'_, MonitorMsg> {
        let s = self.sample.as_ref(); let val = |v: Option<f32>, unit: &str| v.map(|x| format!("{x:.1}{unit}")).unwrap_or_else(|| "—".into());
        let temp_color = |v: Option<f32>| if v.unwrap_or(0.0) >= 90.0 { C_ERR } else if v.unwrap_or(0.0) >= 80.0 { C_WARN } else { C_OK };
        let mut col = column![text("하드웨어 모니터").size(TYPE_SCREEN_TITLE).font(FONT_BOLD), Space::with_height(6), text("2초 간격 센서 상태 및 과열 보호").size(TYPE_CAPTION).color(C_DIM), Space::with_height(14)].spacing(0);
        if let Some(r) = &self.running { col = col.push(running_bar(r)).push(Space::with_height(10)); }
        col = col.push(card(column![text("CPU").size(TYPE_BODY).font(FONT_SEMIBOLD), text(val(s.and_then(|x| x.package_temp), "°C")).size(28).color(temp_color(s.and_then(|x| x.package_temp))), text(format!("코어 최대 {} · 사용률 {} · 클럭 {}", val(s.and_then(|x| x.core_max_temp), "°C"), val(s.and_then(|x| x.cpu_usage), "%"), val(s.and_then(|x| x.clock_mhz), " MHz"))).size(TYPE_CAPTION).color(C_DIM), text(format!("터보 {} · 스로틀 {} ({}) · 팬 {} · 프로파일 {}", s.and_then(|x| x.turbo_disabled).map(|b| if b { "꺼짐" } else { "켜짐" }).unwrap_or("지원 안 함"), s.and_then(|x| x.throttle_count).map(|v| v.to_string()).unwrap_or_else(|| "—".into()), val(s.and_then(|x| x.throttle_per_sec), "/초"), s.and_then(|x| x.fan_rpm).map(|v| format!("{v} RPM")).unwrap_or_else(|| "—".into()), s.and_then(|x| x.profile.as_deref()).unwrap_or("—"))).size(TYPE_CAPTION).color(C_DIM)]));
        col = col.push(Space::with_height(10)).push(card(column![text("디스크").size(TYPE_BODY).font(FONT_SEMIBOLD), text(format!("NVMe {} · 사용률 {} · IO 정지 avg10 {}", val(s.and_then(|x| x.nvme_temp), "°C"), val(s.and_then(|x| x.disk_usage), "%"), val(s.and_then(|x| x.io_psi), ""))).size(TYPE_CAPTION).color(C_TEXT)]));
        col = col.push(Space::with_height(10)).push(card(column![text("메모리").size(TYPE_BODY).font(FONT_SEMIBOLD), text(format!("{} / {} ({}) · 스왑 {} · TMEM {} · PSI {}", val(s.and_then(|x| x.mem_used_gib), " GiB"), val(s.and_then(|x| x.mem_total_gib), " GiB"), val(s.and_then(|x| x.mem_percent), "%"), val(s.and_then(|x| x.swap_used_gib), " GiB"), val(s.and_then(|x| x.tmem_temp), "°C"), val(s.and_then(|x| x.mem_psi), ""))).size(TYPE_CAPTION).color(C_TEXT)]));
        let gpu = match s.and_then(|x| x.gpu_sleeping) { Some(true) => "절전(꺼짐)".into(), Some(false) => format!("온도 {} · 사용률 {}", val(s.and_then(|x| x.gpu_temp), "°C"), val(s.and_then(|x| x.gpu_usage), "%")), None => "—".into() };
        col = col.push(Space::with_height(10)).push(card(column![text("GPU").size(TYPE_BODY).font(FONT_SEMIBOLD), text(gpu).size(TYPE_CAPTION).color(C_TEXT)]));
        let enabled = self.running.is_none(); let protect = if self.saved.protect { "자동 차단 끄기" } else { "자동 차단 켜기" };
        let helper_extra: Element<'_, MonitorMsg> = if !self.helper_installed {
            column![Space::with_height(8), text("권한 설정이 필요합니다.").size(TYPE_CAPTION).color(C_DIM), action_btn("권한 설정 (pkexec, 1회)", MonitorMsg::InstallHelper, enabled, C_BLUE)].into()
        } else { Space::with_height(0).into() };
        col = col.push(Space::with_height(10)).push(card(column![text("과열 보호").size(TYPE_BODY).font(FONT_SEMIBOLD), text("90°C 이상이면 터보 차단, 78°C 이하 60초 유지 시 복원").size(TYPE_CAPTION).color(C_DIM), text(format!("현재: {} · {}", s.and_then(|x| x.turbo_disabled).map(|b| if b { "터보 꺼짐" } else { "터보 켜짐" }).unwrap_or("지원 안 함"), if self.saved.turbo_off_by_popmgr { "popmgr가 차단함" } else { "사용자 상태 유지" })).size(TYPE_CAPTION).color(C_TEXT), self.last_action.as_deref().map(|x| text(x).size(TYPE_CAPTION).color(C_DIM)).unwrap_or_else(|| text("").size(1)), Space::with_height(8), row![action_btn(protect, MonitorMsg::ToggleProtect, enabled, C_BLUE), Space::with_width(8), action_btn("터보 끄기 지금", MonitorMsg::TurnOffNow, enabled && self.helper_installed, C_WARN), Space::with_width(8), action_btn("터보 켜기", MonitorMsg::TurnOnNow, enabled && self.helper_installed, C_OK),].spacing(0), helper_extra]));
        container(col).padding([4, 0]).width(Length::Fill).into()
    }
}

pub async fn sample_hw() -> HwSample { sample_hw_with(root_disk_name().as_deref(), true).await }
pub async fn hw_sample_text_async() -> String { hw_sample_text(&sample_hw().await) }
pub fn turbo_cli(args: &[String]) -> i32 {
    let Some(mode) = args.get(2).filter(|x| x.as_str() == "on" || x.as_str() == "off") else {
        eprintln!("사용법: popmgr --turbo on|off");
        return 2;
    };
    if args.len() != 3 { eprintln!("사용법: popmgr --turbo on|off"); return 2; }
    let runtime = match tokio::runtime::Runtime::new() { Ok(x) => x, Err(e) => { eprintln!("런타임 생성 실패: {e}"); return 1; } };
    let result = runtime.block_on(runner::run_sh(&format!("sudo -n {TURBO_HELPER} {mode} 2>&1")));
    if result.success { print!("{}", result.output); 0 } else { eprintln!("{}", result.output.trim()); 1 }
}
async fn sample_hw_with(root_disk: Option<&str>, profile: bool) -> HwSample {
    let mut s = HwSample { sampled_at: Some(Instant::now()), ..Default::default() };
    let hwmons = std::fs::read_dir("/sys/class/hwmon").ok().into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
    for p in &hwmons { let name = read_trim(p.join("name")); match name.as_deref() { Some("coretemp") => { s.package_temp = read_num(p.join("temp1_input")).map(|v| v / 1000.0); s.core_max_temp = (2..16).filter_map(|n| read_num(p.join(format!("temp{n}_input"))).map(|v| v / 1000.0)).max_by(f32::total_cmp); }, Some("nvme") => s.nvme_temp = read_num(p.join("temp1_input")).map(|v| v / 1000.0), Some("dell_smm") => s.fan_rpm = read_trim(p.join("fan1_input")).and_then(|v| v.parse().ok()), _ => {} } }
    for e in std::fs::read_dir("/sys/class/thermal").ok().into_iter().flatten().flatten() { let p=e.path(); let ty=read_trim(p.join("type")); if s.package_temp.is_none() && ty.as_deref()==Some("x86_pkg_temp") { s.package_temp=read_num(p.join("temp")).map(|v|v/1000.0); } if ty.as_deref()==Some("TMEM") { s.tmem_temp=read_num(p.join("temp")).map(|v|v/1000.0); } }
    s.cpu = parse_proc_stat(&std::fs::read_to_string("/proc/stat").unwrap_or_default());
    let freqs: Vec<f32> = std::fs::read_dir("/sys/devices/system/cpu").ok().into_iter().flatten().flatten().filter_map(|e| read_num(e.path().join("cpufreq/scaling_cur_freq")).map(|v|v/1000.0)).collect(); s.clock_mhz = if freqs.is_empty() { cpuinfo_clock(&std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default()) } else { Some(freqs.iter().sum::<f32>() / freqs.len() as f32) };
    s.turbo_disabled = read_trim("/sys/devices/system/cpu/intel_pstate/no_turbo").and_then(|v| match v.as_str(){"0"=>Some(false),"1"=>Some(true),_=>None}).or_else(|| read_trim("/sys/devices/system/cpu/cpufreq/boost").and_then(|v|match v.as_str(){"0"=>Some(true),"1"=>Some(false),_=>None}));
    s.throttle_count=read_trim("/sys/devices/system/cpu/cpu0/thermal_throttle/package_throttle_count").and_then(|v|v.parse().ok());
    s.disk_io_ms = root_disk.and_then(|d| disk_io_ms(&std::fs::read_to_string("/proc/diskstats").unwrap_or_default(), d)); s.io_psi = psi_avg10(&std::fs::read_to_string("/proc/pressure/io").unwrap_or_default(), "full"); s.mem_psi=psi_avg10(&std::fs::read_to_string("/proc/pressure/memory").unwrap_or_default(), "some");
    if let Some((used,total,pct,swap))=parse_meminfo(&std::fs::read_to_string("/proc/meminfo").unwrap_or_default()) { s.mem_used_gib=Some(used);s.mem_total_gib=Some(total);s.mem_percent=Some(pct);s.swap_used_gib=Some(swap); }
    if profile { let r=runner::run("system76-power", &["profile"]).await; s.profile=r.output.lines().find_map(|l| l.trim().strip_prefix("Power Profile:").map(|v|v.trim().to_string())); }
    let active = nvidia_active(); s.gpu_sleeping=active.map(|b|!b); if active==Some(true) && command_exists("nvidia-smi") { let r=runner::run("nvidia-smi", &["--query-gpu=temperature.gpu,utilization.gpu", "--format=csv,noheader,nounits"]).await; if let Some(line)=r.output.lines().next() { let mut x=line.split(',').map(|v|v.trim().parse::<f32>().ok()); s.gpu_temp=x.next().flatten();s.gpu_usage=x.next().flatten(); } }
    s
}

fn read_trim<P: AsRef<Path>>(p:P)->Option<String>{std::fs::read_to_string(p).ok().map(|s|s.trim().to_string())} fn read_num<P:AsRef<Path>>(p:P)->Option<f32>{read_trim(p)?.parse().ok()}
fn parse_proc_stat(s:&str)->Option<CpuCounters>{let x=s.lines().next()?.split_whitespace().collect::<Vec<&str>>();if x.first()!=Some(&"cpu"){return None} let n=|i:usize|x.get(i).and_then(|v|v.parse::<u64>().ok()).unwrap_or(0);Some(CpuCounters{total:(1..x.len()).map(n).sum(),idle:n(4)+n(5)})}
fn cpu_usage(a:CpuCounters,b:CpuCounters)->Option<f32>{let t=b.total.checked_sub(a.total)?;if t==0{return None}Some((1.0-(b.idle.saturating_sub(a.idle)as f32/t as f32))*100.0)}
fn parse_meminfo(s:&str)->Option<(f32,f32,f32,f32)>{let get=|key|s.lines().find_map(|l|l.strip_prefix(key).and_then(|x|x.split_whitespace().next()).and_then(|x|x.parse::<f32>().ok()));let total=get("MemTotal:")?/1048576.0;let avail=get("MemAvailable:")?/1048576.0;let swap_total=get("SwapTotal:").unwrap_or(0.0)/1048576.0;let swap_free=get("SwapFree:").unwrap_or(0.0)/1048576.0;let used=total-avail;Some((used,total,used/total*100.0,swap_total-swap_free))}
fn disk_io_ms(s:&str,disk:&str)->Option<u64>{s.lines().find_map(|l|{let x=l.split_whitespace().collect::<Vec<_>>();if x.get(2)==Some(&disk){x.get(12)?.parse().ok()}else{None}})} fn disk_usage(a:u64,b:u64,d:Duration)->Option<f32>{let ms=b.checked_sub(a)? as f32;let elapsed=d.as_millis()as f32;if elapsed==0.0{None}else{Some((ms/elapsed*100.0).min(100.0))}}
fn disk_name(part:&str)->String{if let Some(i)=part.rfind('p'){if part[..i].chars().last().is_some_and(|c|c.is_ascii_digit())&&part[i+1..].chars().all(|c|c.is_ascii_digit()){return part[..i].into()}}part.trim_end_matches(|c:char|c.is_ascii_digit()).into()}
fn root_disk_name()->Option<String>{let out=std::process::Command::new("findmnt").args(["-no","SOURCE","/"]).output().ok()?;let src=String::from_utf8_lossy(&out.stdout);let name=Path::new(src.trim()).file_name()?.to_str()?;Some(disk_name(name))}
fn psi_avg10(s:&str,kind:&str)->Option<f32>{s.lines().find(|l|l.starts_with(kind)).and_then(|l|l.split_whitespace().find_map(|x|x.strip_prefix("avg10=").and_then(|v|v.parse().ok())))} fn cpuinfo_clock(s:&str)->Option<f32>{let x=s.lines().filter_map(|l|l.split(':').nth(1).filter(|_|l.starts_with("cpu MHz")).and_then(|v|v.trim().parse::<f32>().ok())).collect::<Vec<_>>();(!x.is_empty()).then(||x.iter().sum::<f32>()/x.len()as f32)}
fn nvidia_active()->Option<bool>{for e in std::fs::read_dir("/sys/bus/pci/devices").ok()?.flatten(){let p=e.path();if read_trim(p.join("vendor")).as_deref()==Some("0x10de")&&read_trim(p.join("class")).is_some_and(|c|c.starts_with("0x03")){return read_trim(p.join("power/runtime_status")).map(|s|s=="active")}}None} fn command_exists(cmd:&str)->bool{std::env::var_os("PATH").is_some_and(|p|std::env::split_paths(&p).any(|d|d.join(cmd).is_file()))}
fn saved_path()->PathBuf{dirs::data_local_dir().unwrap_or_else(||PathBuf::from("/tmp")).join("popmgr/monitor.json")} fn load_saved()->MonitorSaved{std::fs::read_to_string(saved_path()).ok().and_then(|x|serde_json::from_str(&x).ok()).unwrap_or_default()} fn save_saved(s:&MonitorSaved){let p=saved_path();if let Some(d)=p.parent(){let _=std::fs::create_dir_all(d);}let _=std::fs::write(p,serde_json::to_string(s).unwrap_or_default());}
async fn helper_ok()->bool{if !Path::new(TURBO_HELPER).exists(){return false} runner::run_sh("sudo -n -l /usr/local/bin/popmgr-turbo >/dev/null 2>&1").await.success} async fn run_turbo(a:TurboAction)->CmdResult{runner::run_sh(if a==TurboAction::TurnOff{"sudo -n /usr/local/bin/popmgr-turbo off 2>&1"}else{"sudo -n /usr/local/bin/popmgr-turbo on 2>&1"}).await}
fn base64_encode(data:&[u8])->String{const T:&[u8;64]=b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";let mut o=String::new();for c in data.chunks(3){let b=[c[0],*c.get(1).unwrap_or(&0),*c.get(2).unwrap_or(&0)];let n=(b[0]as u32)<<16|(b[1]as u32)<<8|b[2]as u32;o.push(T[(n>>18&63)as usize]as char);o.push(T[(n>>12&63)as usize]as char);o.push(if c.len()>1{T[(n>>6&63)as usize]as char}else{'='});o.push(if c.len()>2{T[(n&63)as usize]as char}else{'='});}o}
pub fn hw_sample_text(s:&HwSample)->String{format!("CPU: temp={} usage={} clock={} turbo={}\n디스크: nvme={} usage={} io_psi={}\n메모리: used={} / {} GiB ({}) swap={}\nGPU: {}",s.package_temp.map(|v|format!("{v:.1}°C")).unwrap_or_else(||"—".into()),s.cpu_usage.map(|v|format!("{v:.1}%")).unwrap_or_else(||"—".into()),s.clock_mhz.map(|v|format!("{v:.0}MHz")).unwrap_or_else(||"—".into()),s.turbo_disabled.map(|v|if v{"off"}else{"on"}).unwrap_or("unsupported"),s.nvme_temp.map(|v|format!("{v:.1}°C")).unwrap_or_else(||"—".into()),s.disk_usage.map(|v|format!("{v:.1}%")).unwrap_or_else(||"—".into()),s.io_psi.map(|v|v.to_string()).unwrap_or_else(||"—".into()),s.mem_used_gib.map(|v|format!("{v:.1}")).unwrap_or_else(||"—".into()),s.mem_total_gib.map(|v|format!("{v:.1}")).unwrap_or_else(||"—".into()),s.mem_percent.map(|v|format!("{v:.1}%")).unwrap_or_else(||"—".into()),s.swap_used_gib.map(|v|format!("{v:.1}")).unwrap_or_else(||"—".into()),match s.gpu_sleeping{Some(true)=>"절전(꺼짐)".into(),Some(false)=>format!("temp={} usage={}",s.gpu_temp.map(|v|format!("{v:.1}°C")).unwrap_or_else(||"—".into()),s.gpu_usage.map(|v|format!("{v:.1}%")).unwrap_or_else(||"—".into())),None=>"—".into()})}

#[cfg(test)] mod tests { use super::*;
 #[test] fn turbo_hysteresis(){let n=Instant::now();assert_eq!(turbo_decision(92.,false,None,n).0,TurboAction::TurnOff);assert_eq!(turbo_decision(85.,true,Some(n),n).1,None);let(a,c)=turbo_decision(75.,true,None,n);assert_eq!(a,TurboAction::None);assert_eq!(c,Some(n));assert_eq!(turbo_decision(75.,true,Some(n),n+Duration::from_secs(61)).0,TurboAction::TurnOn);assert_eq!(turbo_decision(70.,false,None,n).0,TurboAction::None);}
 #[test] fn proc_stat_delta(){let a=parse_proc_stat("cpu  100 0 100 800 0 0 0 0\n").unwrap();let b=parse_proc_stat("cpu  300 0 300 1200 0 0 0 0\n").unwrap();assert_eq!(cpu_usage(a,b).unwrap().round(),50.);}
 #[test] fn meminfo_parse(){let x=parse_meminfo("MemTotal: 10485760 kB\nMemAvailable: 5242880 kB\nSwapTotal: 2097152 kB\nSwapFree: 1048576 kB\n").unwrap();assert_eq!(x.0,5.);assert_eq!(x.1,10.);assert_eq!(x.2,50.);assert_eq!(x.3,1.);}
 #[test] fn diskstats_delta(){let a="259 0 nvme0n1 0 0 0 0 0 0 0 0 0 100\n";let b="259 0 nvme0n1 0 0 0 0 0 0 0 0 0 300\n";assert_eq!(disk_io_ms(a,"nvme0n1"),Some(100));assert_eq!(disk_usage(disk_io_ms(a,"nvme0n1").unwrap(),disk_io_ms(b,"nvme0n1").unwrap(),Duration::from_secs(1)),Some(20.));}
 #[test] fn partition_names(){assert_eq!(disk_name("nvme0n1p3"),"nvme0n1");assert_eq!(disk_name("sda2"),"sda");assert_eq!(disk_name("mmcblk0p1"),"mmcblk0");}
 #[test] fn helper_script_is_fixed(){assert!(TURBO_HELPER_SCRIPT.contains("case \"${1:-}\" in"));assert!(TURBO_HELPER_SCRIPT.contains("no_turbo"));assert!(TURBO_HELPER_SCRIPT.contains("boost"));for path in TURBO_HELPER_SCRIPT.split_whitespace().filter(|x|x.contains("/sys/")){assert!(path.contains("intel_pstate/no_turbo")||path.contains("cpufreq/boost"));}}
 #[test] fn sudoers_line(){let u="dell";assert_eq!(format!("{u} ALL=(root) NOPASSWD: {TURBO_HELPER}"),"dell ALL=(root) NOPASSWD: /usr/local/bin/popmgr-turbo");}
 #[test] fn saved_round_trip(){let x=MonitorSaved{protect:true,turbo_off_by_popmgr:true};let y:MonitorSaved=serde_json::from_str(&serde_json::to_string(&x).unwrap()).unwrap();assert!(y.protect&&y.turbo_off_by_popmgr);}
}
