use iced::{
    widget::{column, container, row, scrollable, text, Space},
    Color, Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_DIM, C_ERR, C_OK, C_WARN};

// 패치 적용 여부를 ~/.local/share/popmgr/patches.json 에 기록
const MARKER_FILE: &str = "patches.json";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct PatchMarkers {
    copy_path: bool,
    three_finger: bool,
}

#[derive(Debug, Clone)]
pub struct CosmicStatus {
    pub copy_path_patched: bool,
    pub three_finger_patched: bool,
    pub cosmic_files_ver: String,
    pub cosmic_comp_ver: String,
}

#[derive(Debug, Clone)]
pub enum CosmicMsg {
    Refresh,
    Refreshed(CosmicStatus),
    ApplyCopyPath,
    RemoveCopyPath,
    ApplyThreeFinger,
    RemoveThreeFinger,
    Done(CmdResult),
}

pub struct CosmicState {
    pub status: Option<CosmicStatus>,
    pub running: Option<String>,
}

impl CosmicState {
    pub fn new() -> Self { Self { status: None, running: None } }

    pub fn update(&mut self, msg: CosmicMsg) -> (Task<CosmicMsg>, Option<CmdResult>) {
        match msg {
            CosmicMsg::Refresh => {
                let t = Task::perform(async { scan_cosmic().await }, CosmicMsg::Refreshed);
                (t, None)
            }
            CosmicMsg::Refreshed(s) => { self.status = Some(s); (Task::none(), None) }
            CosmicMsg::ApplyCopyPath => {
                self.running = Some("copy-path 패치 빌드 중 (수 분 소요)...".into());
                let t = Task::perform(async { apply_copy_path_patch().await }, CosmicMsg::Done);
                (t, None)
            }
            CosmicMsg::RemoveCopyPath => {
                self.running = Some("copy-path 패치 제거 중...".into());
                let t = Task::perform(async { remove_copy_path_patch().await }, CosmicMsg::Done);
                (t, None)
            }
            CosmicMsg::ApplyThreeFinger => {
                self.running = Some("3-finger 패치 빌드 중 (수 분 소요)...".into());
                let t = Task::perform(async { apply_three_finger_patch().await }, CosmicMsg::Done);
                (t, None)
            }
            CosmicMsg::RemoveThreeFinger => {
                self.running = Some("3-finger 패치 제거 중...".into());
                let t = Task::perform(async { remove_three_finger_patch().await }, CosmicMsg::Done);
                (t, None)
            }
            CosmicMsg::Done(r) => {
                self.running = None;
                let refresh = Task::perform(async { scan_cosmic().await }, CosmicMsg::Refreshed);
                (refresh, Some(r))
            }
        }
    }

    pub fn view(&self) -> Element<'_, CosmicMsg> {
        let is_running = self.running.is_some();
        let mut col = column![
            text("COSMIC 트윅").size(20),
            Space::with_height(6),
            text("패치 적용 시 소스를 클론하고 cargo build --release로 빌드합니다 (수 분 소요).")
                .size(11)
                .color(C_DIM),
            Space::with_height(16),
        ];

        if let Some(label) = &self.running {
            col = col.push(running_bar(label)).push(Space::with_height(12));
        }

        let (cp_patched, cp_ver, tf_patched, tf_ver) = if let Some(st) = &self.status {
            (st.copy_path_patched, st.cosmic_files_ver.clone(),
             st.three_finger_patched, st.cosmic_comp_ver.clone())
        } else {
            (false, "확인 중...".to_string(), false, "확인 중...".to_string())
        };

        // copy-path 패치 카드
        col = col.push(patch_card(
            "cosmic-files: Copy Path 항상 표시",
            "우클릭 메뉴에서 Shift 없이 '경로 복사'를 항상 표시합니다.\n(eondcom/cosmic-files-copy-path)",
            cp_ver,
            cp_patched,
            CosmicMsg::ApplyCopyPath,
            CosmicMsg::RemoveCopyPath,
            is_running,
        ));

        col = col.push(Space::with_height(12));

        // 3-finger 패치 카드
        col = col.push(patch_card(
            "cosmic-comp: 3손가락 워크스페이스 전환",
            "터치패드 3손가락 위 스와이프로 COSMIC 워크스페이스 오버뷰를 엽니다.\n(eondcom/cosmic-three-finger-gesture)",
            tf_ver,
            tf_patched,
            CosmicMsg::ApplyThreeFinger,
            CosmicMsg::RemoveThreeFinger,
            is_running,
        ));

        scrollable(container(col).padding([4, 0])).into()
    }
}

fn patch_card(
    title: &'static str,
    desc: &'static str,
    version: String,
    patched: bool,
    apply_msg: CosmicMsg,
    remove_msg: CosmicMsg,
    disabled: bool,
) -> Element<'static, CosmicMsg> {
    let status_txt = if patched { "[적용됨]" } else { "[미적용]" };
    let status_col = if patched { C_OK } else { C_DIM };

    let btn: Element<'static, CosmicMsg> = if patched {
        action_btn("패치 제거", remove_msg, !disabled, Color::from_rgb(0.55, 0.18, 0.18))
    } else {
        action_btn("패치 적용", apply_msg, !disabled, Color::from_rgb(0.15, 0.45, 0.75))
    };

    let ver_label = format!("설치 버전: {version}");
    card(
        column![
            row![
                column![
                    text(title).size(13).color(Color::from_rgb(0.9, 0.9, 0.95)),
                    Space::with_height(3),
                    text(desc).size(11).color(C_DIM),
                    Space::with_height(4),
                    text(ver_label).size(11).color(Color::from_rgb(0.5, 0.5, 0.6)),
                ].width(Length::Fill),
                column![
                    text(status_txt).size(12).color(status_col),
                    Space::with_height(8),
                    btn,
                ].align_x(iced::Alignment::End),
            ],
        ]
    )
}

async fn scan_cosmic() -> CosmicStatus {
    let markers = load_markers();

    let files_ver = runner::run("bash", &["-c", "dpkg -l cosmic-files 2>/dev/null | grep '^ii' | awk '{print $3}'"]).await;
    let comp_ver  = runner::run("bash", &["-c", "dpkg -l cosmic-comp 2>/dev/null | grep '^ii' | awk '{print $3}'"]).await;

    CosmicStatus {
        copy_path_patched:   markers.copy_path,
        three_finger_patched: markers.three_finger,
        cosmic_files_ver: files_ver.output.trim().to_string(),
        cosmic_comp_ver:  comp_ver.output.trim().to_string(),
    }
}

async fn apply_copy_path_patch() -> CmdResult {
    let src  = "/tmp/popmgr-cosmic-files-src";
    let patch = "/tmp/popmgr-cosmic-files-patch";
    let script = format!(
        r#"set -e
rm -rf {src} {patch}
echo "=== 패치 파일 클론 ==="
git clone --depth 1 https://github.com/eondcom/cosmic-files-copy-path {patch} 2>&1
echo "=== 원본 소스 클론 (pop-os/cosmic-files bf01bb3) ==="
git clone https://github.com/pop-os/cosmic-files {src} 2>&1
cd {src}
git checkout bf01bb3 2>&1
echo "=== 패치 적용 ==="
git apply {patch}/cosmic-files-copy-path.patch 2>&1
echo "=== 빌드 ==="
export LIBCLANG_PATH=/usr/lib/llvm-18/lib
cargo build --release 2>&1
echo "=== 설치 ==="
pkexec bash -c 'cp -a /usr/bin/cosmic-files /usr/bin/cosmic-files.bak 2>/dev/null || true; install -Dm0755 {src}/target/release/cosmic-files /usr/bin/cosmic-files'
echo "copy-path 패치 설치 완료"
"#
    );
    let r = runner::run_stream(&script).await;
    if r.success {
        let mut m = load_markers();
        m.copy_path = true;
        save_markers(&m);
    }
    r
}

async fn remove_copy_path_patch() -> CmdResult {
    let r = runner::run_sh(
        "pkexec bash -c 'apt-get install --reinstall -y cosmic-files 2>&1'"
    ).await;
    if r.success {
        let mut m = load_markers();
        m.copy_path = false;
        save_markers(&m);
    }
    r
}

async fn apply_three_finger_patch() -> CmdResult {
    let src   = "/tmp/popmgr-cosmic-comp-src";
    let patch = "/tmp/popmgr-cosmic-comp-patch";
    let script = format!(
        r#"set -e
rm -rf {src} {patch}
echo "=== 패치 파일 클론 ==="
git clone --depth 1 https://github.com/eondcom/cosmic-three-finger-gesture {patch} 2>&1
echo "=== 원본 소스 클론 (pop-os/cosmic-comp 22fe419) ==="
git clone https://github.com/pop-os/cosmic-comp {src} 2>&1
cd {src}
git checkout 22fe419 2>&1
echo "=== 패치 적용 ==="
git apply {patch}/three-finger-gesture.patch 2>&1
echo "=== 빌드 ==="
cargo build --release 2>&1
echo "=== 설치 ==="
pkexec bash -c 'cp -a /usr/bin/cosmic-comp /usr/bin/cosmic-comp.bak 2>/dev/null || true; install -Dm0755 {src}/target/release/cosmic-comp /usr/bin/cosmic-comp'
echo "3-finger 패치 설치 완료"
"#
    );
    let r = runner::run_stream(&script).await;
    if r.success {
        let mut m = load_markers();
        m.three_finger = true;
        save_markers(&m);
    }
    r
}

async fn remove_three_finger_patch() -> CmdResult {
    let r = runner::run_sh(
        "pkexec bash -c 'apt-get install --reinstall -y cosmic-comp 2>&1'"
    ).await;
    if r.success {
        let mut m = load_markers();
        m.three_finger = false;
        save_markers(&m);
    }
    r
}

fn marker_path() -> std::path::PathBuf {
    let mut p = dirs::data_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    p.push("popmgr");
    std::fs::create_dir_all(&p).ok();
    p.push(MARKER_FILE);
    p
}

fn load_markers() -> PatchMarkers {
    std::fs::read_to_string(marker_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_markers(m: &PatchMarkers) {
    if let Ok(data) = serde_json::to_string_pretty(m) {
        std::fs::write(marker_path(), data).ok();
    }
}
