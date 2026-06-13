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
    let files_ver = runner::run("bash", &["-c", "dpkg -l cosmic-files 2>/dev/null | grep '^ii' | awk '{print $3}'"]).await;
    let comp_ver  = runner::run("bash", &["-c", "dpkg -l cosmic-comp 2>/dev/null | grep '^ii' | awk '{print $3}'"]).await;

    // 마커 파일이 아니라 실제 바이너리 상태로 판단한다.
    // 시스템 업그레이드가 패치를 덮어쓰면 마커는 그대로라 "적용됨"으로 거짓 표시되던 버그를 차단.
    let copy_path_patched   = binary_patched("cosmic-files", "/usr/bin/cosmic-files").await;
    let three_finger_patched = binary_patched("cosmic-comp",  "/usr/bin/cosmic-comp").await;

    // 실제 상태로 마커 동기화 (apply/remove 경로의 기록과 어긋나지 않도록)
    let mut m = load_markers();
    if m.copy_path != copy_path_patched || m.three_finger != three_finger_patched {
        m.copy_path = copy_path_patched;
        m.three_finger = three_finger_patched;
        save_markers(&m);
    }

    CosmicStatus {
        copy_path_patched,
        three_finger_patched,
        cosmic_files_ver: files_ver.output.trim().to_string(),
        cosmic_comp_ver:  comp_ver.output.trim().to_string(),
    }
}

/// dpkg -V 는 패키지 md5 와 다른 파일만 출력한다.
/// popmgr 가 빌드해 덮어쓴 경우에만 차이가 생기므로, 차이가 있으면 패치 적용 상태로 본다.
/// 시스템 업그레이드/재설치로 스톡 바이너리가 복원되면 차이가 사라져 자동으로 "미적용"이 된다.
async fn binary_patched(pkg: &str, bin: &str) -> bool {
    let script = format!(
        "dpkg -V {pkg} 2>/dev/null | grep -qE '[[:space:]]{bin}$' && echo yes || echo no"
    );
    runner::run("bash", &["-c", &script]).await.output.trim() == "yes"
}

async fn apply_copy_path_patch() -> CmdResult {
    let src = "/tmp/popmgr-cosmic-files-src";
    let script = format!(
        r#"set -e
rm -rf {src}

echo "=== 설치된 버전 커밋 확인 ==="
CF_COMMIT=$(dpkg -l cosmic-files 2>/dev/null | grep '^ii' | awk '{{print $3}}' | rev | cut -d'~' -f1 | rev)
echo "cosmic-files 커밋: $CF_COMMIT"

echo "=== 소스 타르볼 다운로드 ==="
curl -fL "https://github.com/pop-os/cosmic-files/archive/${{CF_COMMIT}}.tar.gz" \
    -o /tmp/cosmic-files-src.tar.gz
tar xzf /tmp/cosmic-files-src.tar.gz -C /tmp/
SRCDIR=$(ls -d /tmp/cosmic-files-${{CF_COMMIT}}* 2>/dev/null | head -1)
mv "$SRCDIR" {src}

echo "=== 패치 파일 다운로드 ==="
curl -fL https://raw.githubusercontent.com/eondcom/cosmic-files-copy-path/main/cosmic-files-copy-path.patch \
    -o /tmp/cosmic-files-copy-path.patch

echo "=== 패치 적용 ==="
cd {src}
patch -p1 --fuzz 5 < /tmp/cosmic-files-copy-path.patch

echo "=== 빌드 ==="
LIBCLANG_PATH=$(ls -d /usr/lib/llvm-*/lib 2>/dev/null | sort -V | tail -1)
LIBCLANG_PATH=${{LIBCLANG_PATH:-/usr/lib}}
export LIBCLANG_PATH
echo "LIBCLANG_PATH=$LIBCLANG_PATH"
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
    let src = "/tmp/popmgr-cosmic-comp-src";
    let script = format!(
        r#"set -e
rm -rf {src}

echo "=== 설치된 버전 커밋 확인 ==="
CC_COMMIT=$(dpkg -l cosmic-comp 2>/dev/null | grep '^ii' | awk '{{print $3}}' | rev | cut -d'~' -f1 | rev)
echo "cosmic-comp 커밋: $CC_COMMIT"

echo "=== 소스 타르볼 다운로드 ==="
curl -fL "https://github.com/pop-os/cosmic-comp/archive/${{CC_COMMIT}}.tar.gz" \
    -o /tmp/cosmic-comp-src.tar.gz
tar xzf /tmp/cosmic-comp-src.tar.gz -C /tmp/
SRCDIR=$(ls -d /tmp/cosmic-comp-${{CC_COMMIT}}* 2>/dev/null | head -1)
mv "$SRCDIR" {src}

echo "=== 패치 파일 다운로드 ==="
curl -fL https://raw.githubusercontent.com/eondcom/cosmic-three-finger-gesture/main/three-finger-gesture.patch \
    -o /tmp/three-finger-gesture.patch

echo "=== 패치 적용 ==="
cd {src}
patch -p1 --fuzz 5 < /tmp/three-finger-gesture.patch

echo "=== 빌드 (10~20분 소요) ==="
cargo build --release 2>&1

echo "=== 설치 ==="
pkexec bash -c 'cp -a /usr/bin/cosmic-comp /usr/bin/cosmic-comp.bak 2>/dev/null || true; install -Dm0755 {src}/target/release/cosmic-comp /usr/bin/cosmic-comp'
echo "3-finger 패치 설치 완료 — 로그아웃 후 재로그인 필요"
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
