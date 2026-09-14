use iced::{
    widget::{column, container, row, scrollable, text, Space},
    Element, Length, Task,
};
use crate::runner::{self, CmdResult};
use super::ime::{action_btn, card, running_bar, C_BLUE, C_DIM, C_ERR, C_OK, C_TEXT, C_WARN};

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
    pub screenshot_shortcuts: Vec<ShortcutHealth>,
    pub shotbox_source_exists: bool,
    pub shotbox_binary_exists: bool,
}

#[derive(Debug, Clone)]
pub struct ShortcutHealth {
    key: String,
    state: ShortcutState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutState {
    Missing,
    ExecutableMissing,
    Ok,
}

#[derive(Debug, Clone)]
pub enum CosmicMsg {
    Refresh,
    Refreshed(CosmicStatus),
    ApplyCopyPath,
    RemoveCopyPath,
    ApplyThreeFinger,
    RemoveThreeFinger,
    BuildShotbox,
    RegisterScreenshotShortcuts,
    Done(CmdResult),
}

pub struct CosmicState {
    pub status: Option<CosmicStatus>,
    pub running: Option<String>,
}

impl CosmicState {
    pub fn new() -> Self {
        Self {
            status: None,
            running: None,
        }
    }

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
            CosmicMsg::BuildShotbox => {
                self.running = Some("shotbox 빌드 중...".into());
                let t = Task::perform(async { build_shotbox().await }, CosmicMsg::Done);
                (t, None)
            }
            CosmicMsg::RegisterScreenshotShortcuts => {
                self.running = Some("스크린샷 단축키 등록/갱신 중...".into());
                let t = Task::perform(
                    async { register_screenshot_shortcuts().await },
                    CosmicMsg::Done,
                );
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

        if let Some(status) = &self.status {
            col = col.push(screenshot_shortcut_card(status, is_running));
            col = col.push(Space::with_height(12));
        }

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
        action_btn("패치 제거", remove_msg, !disabled, C_ERR)
    } else {
        action_btn("패치 적용", apply_msg, !disabled, C_BLUE)
    };

    let ver_label = format!("설치 버전: {version}");
    card(
        column![
            row![
                column![
                    text(title).size(13).color(C_TEXT),
                    Space::with_height(3),
                    text(desc).size(11).color(C_DIM),
                    Space::with_height(4),
                    text(ver_label).size(11).color(C_DIM),
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
        screenshot_shortcuts: scan_screenshot_shortcuts(),
        shotbox_source_exists: shotbox_source_path().is_some_and(|path| path.exists()),
        shotbox_binary_exists: shotbox_binary_path().is_some_and(|path| path.exists()),
    }
}

fn screenshot_shortcut_card(
    status: &CosmicStatus,
    disabled: bool,
) -> Element<'static, CosmicMsg> {
    let mut details = column![
        text("스크린샷 단축키").size(13).color(C_TEXT),
        Space::with_height(4),
        text("Ctrl+Shift+3 전체 · 4 영역 · 5 옵션").size(11).color(C_DIM),
        Space::with_height(6),
    ];

    for shortcut in &status.screenshot_shortcuts {
        let (label, color) = match shortcut.state {
            ShortcutState::Missing => ("미등록", C_ERR),
            ShortcutState::ExecutableMissing => ("등록됨·실행 파일 없음", C_WARN),
            ShortcutState::Ok => ("정상", C_OK),
        };
        details = details.push(
            text(format!("{}: {}", shortcut.key, label)).size(11).color(color),
        );
    }

    let source = if status.shotbox_source_exists { "있음" } else { "없음" };
    let binary = if status.shotbox_binary_exists { "있음" } else { "없음" };
    details = details.push(Space::with_height(6));
    details = details.push(
        text(format!("shotbox 소스: {source} · 릴리스 바이너리: {binary}"))
            .size(11)
            .color(C_DIM),
    );

    let mut buttons = column![action_btn(
        "단축키 등록/갱신",
        CosmicMsg::RegisterScreenshotShortcuts,
        !disabled,
        C_BLUE,
    )]
    .align_x(iced::Alignment::End)
    .spacing(8);
    if status.shotbox_source_exists && !status.shotbox_binary_exists {
        buttons = buttons.push(action_btn(
            "shotbox 빌드",
            CosmicMsg::BuildShotbox,
            !disabled,
            C_WARN,
        ));
    }

    card(row![details.width(Length::Fill), buttons])
}

fn home_dir() -> Option<std::path::PathBuf> {
    dirs::home_dir()
}

fn shortcuts_path() -> Option<std::path::PathBuf> {
    home_dir().map(|home| {
        home.join(".config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom")
    })
}

fn shotbox_source_path() -> Option<std::path::PathBuf> {
    home_dir().map(|home| home.join("dev/shotbox/Cargo.toml"))
}

fn shotbox_binary_path() -> Option<std::path::PathBuf> {
    home_dir().map(|home| home.join("dev/shotbox/target/release/shotbox"))
}

fn scan_screenshot_shortcuts() -> Vec<ShortcutHealth> {
    let content = shortcuts_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    let entries = parse_shortcut_entries(&content);
    ["3", "4", "5"]
        .into_iter()
        .map(|key| {
            let state = entries.iter().find(|entry| is_target_shortcut(entry, key)).map_or(
                ShortcutState::Missing,
                |entry| match entry.spawn.as_deref() {
                    Some(command) if executable_exists(command) => ShortcutState::Ok,
                    _ => ShortcutState::ExecutableMissing,
                },
            );
            ShortcutHealth {
                key: format!("Ctrl+Shift+{key}"),
                state,
            }
        })
        .collect()
}

async fn build_shotbox() -> CmdResult {
    let Some(manifest) = shotbox_source_path() else {
        return CmdResult {
            success: false,
            output: "홈 디렉터리를 찾을 수 없습니다.".into(),
        };
    };
    if !manifest.exists() {
        return CmdResult {
            success: false,
            output: "shotbox 소스(Cargo.toml)를 찾을 수 없습니다.".into(),
        };
    }
    let script = format!(
        "CARGO_BUILD_JOBS=4 cargo build --release --manifest-path {}",
        shell_quote(&manifest.to_string_lossy()),
    );
    runner::run_stream(&script).await
}

async fn register_screenshot_shortcuts() -> CmdResult {
    let Some(path) = shortcuts_path() else {
        return CmdResult {
            success: false,
            output: "홈 디렉터리를 찾을 수 없습니다.".into(),
        };
    };
    let updates = shortcut_updates(shotbox_binary_path().as_deref());
    match write_shortcut_updates(&path, &updates) {
        Ok(()) => CmdResult {
            success: true,
            output: "스크린샷 단축키를 등록/갱신했습니다. COSMIC이 즉시 반영하지 않으면 \
                     로그아웃 후 다시 로그인하세요.".into(),
        },
        Err(error) => CmdResult {
            success: false,
            output: format!("스크린샷 단축키 저장 실패: {error}"),
        },
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Debug, Clone)]
pub(crate) struct ShortcutEntry {
    modifiers: Vec<String>,
    key: String,
    spawn: Option<String>,
    range: std::ops::Range<usize>,
}

pub(crate) fn parse_shortcut_entries(content: &str) -> Vec<ShortcutEntry> {
    let Some((open, close)) = shortcut_map_bounds(content) else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    let mut start = open + 1;
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, ch) in content[open + 1..close].char_indices() {
        let index = open + 1 + offset;
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '"' => quoted = true,
            '(' => parens += 1,
            ')' => parens = parens.saturating_sub(1),
            '[' => brackets += 1,
            ']' => brackets = brackets.saturating_sub(1),
            '{' => braces += 1,
            '}' => braces = braces.saturating_sub(1),
            ',' if parens == 0 && brackets == 0 && braces == 0 => {
                let end = index + ch.len_utf8();
                if let Some(entry) = parse_shortcut_entry(content, start..end) {
                    entries.push(entry);
                }
                start = end;
            }
            _ => {}
        }
    }
    if let Some(entry) = parse_shortcut_entry(content, start..close) {
        entries.push(entry);
    }
    entries
}

fn shortcut_map_bounds(content: &str) -> Option<(usize, usize)> {
    let open = content.find('{')?;
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, ch) in content[open..].char_indices() {
        let index = open + offset;
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '"' => quoted = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((open, index));
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_shortcut_entry(content: &str, range: std::ops::Range<usize>) -> Option<ShortcutEntry> {
    let segment = &content[range.clone()];
    let modifiers = field_list(segment, "modifiers:")?;
    let key = field_string(segment, "key:")?;
    Some(ShortcutEntry {
        modifiers,
        key,
        spawn: spawn_command(segment),
        range,
    })
}

fn field_list(segment: &str, field: &str) -> Option<Vec<String>> {
    let after = segment.get(segment.find(field)? + field.len()..)?.trim_start();
    let end = after.find(']')?;
    Some(
        after.get(1..end)?
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
    )
}

fn field_string(segment: &str, field: &str) -> Option<String> {
    let after = segment.get(segment.find(field)? + field.len()..)?.trim_start();
    parse_quoted(after)
}

fn spawn_command(segment: &str) -> Option<String> {
    let after = segment.get(segment.find("Spawn(")? + "Spawn(".len()..)?.trim_start();
    parse_quoted(after)
}

fn parse_quoted(value: &str) -> Option<String> {
    let value = value.strip_prefix('"')?;
    let mut result = String::new();
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            result.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(result);
        } else {
            result.push(ch);
        }
    }
    None
}

pub(crate) fn is_target_shortcut(entry: &ShortcutEntry, key: &str) -> bool {
    entry.key == key
        && entry.modifiers.len() == 2
        && entry.modifiers.iter().any(|modifier| modifier == "Ctrl" || modifier == "Control")
        && entry.modifiers.iter().any(|modifier| modifier == "Shift")
}

fn executable_exists(command: &str) -> bool {
    let Some(program) = command.split_whitespace().next() else {
        return false;
    };
    let path = std::path::Path::new(program);
    if path.is_absolute() {
        return path.exists();
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join(program).exists())
    })
}

fn shortcut_updates(binary: Option<&std::path::Path>) -> Vec<(&'static str, String)> {
    if let Some(binary) = binary.filter(|path| path.exists()) {
        let quoted = binary.to_string_lossy();
        return vec![
            ("3", format!("{quoted} capture full")),
            ("4", format!("{quoted} capture region")),
            ("5", format!("{quoted} options")),
        ];
    }
    vec![
        ("3", "cosmic-screenshot --interactive=false".into()),
        ("4", "cosmic-screenshot --interactive".into()),
    ]
}

pub(crate) fn write_shortcut_updates(
    path: &std::path::Path,
    updates: &[(&str, String)],
) -> std::io::Result<()> {
    let original = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "{}".into(),
        Err(error) => return Err(error),
    };
    if path.exists() {
        std::fs::copy(path, path.with_file_name("custom.popmgr.bak"))?;
    }
    let updated = replace_shortcut_entries(&original, updates);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, updated)
}

pub(crate) fn replace_shortcut_entries(content: &str, updates: &[(&str, String)]) -> String {
    let entries = parse_shortcut_entries(content);
    let Some((_, close)) = shortcut_map_bounds(content) else {
        return content.to_string();
    };
    let mut replacements = Vec::new();
    let mut missing = Vec::new();
    for (key, command) in updates {
        let matches = entries
            .iter()
            .filter(|entry| is_target_shortcut(entry, key))
            .collect::<Vec<_>>();
        if matches.is_empty() {
            missing.push(render_shortcut_entry(key, command));
        } else {
            for entry in matches {
                replacements.push((entry.range.clone(), render_shortcut_entry(key, command)));
            }
        }
    }
    replacements.sort_by_key(|(range, _)| range.start);
    let mut result = String::new();
    let mut cursor = 0usize;
    for (range, replacement) in replacements {
        result.push_str(&content[cursor..range.start]);
        let segment = &content[range.clone()];
        let leading_end = segment.find('(').unwrap_or(0);
        let core_end = segment
            .trim_end_matches(|ch: char| ch.is_whitespace() || ch == ',')
            .len();
        result.push_str(&segment[..leading_end]);
        result.push_str(&replacement);
        result.push_str(&segment[core_end..]);
        cursor = range.end;
    }
    result.push_str(&content[cursor..close]);
    if !missing.is_empty() {
        let needs_comma = !entries.is_empty() && !result.trim_end().ends_with(',');
        if needs_comma {
            result.push(',');
        }
        for entry in missing {
            result.push_str("\n    ");
            result.push_str(&entry);
            result.push(',');
        }
        result.push('\n');
    }
    result.push_str(&content[close..]);
    result
}

fn render_shortcut_entry(key: &str, command: &str) -> String {
    format!("(modifiers: [Ctrl, Shift], key: \"{key}\"): Spawn(\"{command}\")")
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

#[cfg(test)]
mod tests {
    use super::*;

    const SHORTCUT_FIXTURE: &str = r#"{
    (
        modifiers: [Ctrl],
        key: "F12",
        description: Some("Volume up"),
    ): Spawn("pactl set-sink-volume @DEFAULT_SINK@ +5%"),
    (modifiers: [Ctrl], key: "F11"): Spawn("pactl set-sink-volume @DEFAULT_SINK@ -5%"),
    (
        modifiers: [Ctrl],
        key: "F7",
        description: Some("Clear"),
    ): Spawn("clea"),
    (modifiers: [Super], key: "t"): Spawn("waveterm"),
    (modifiers: [Ctrl, Shift], key: "3", description: Some("Full")): Spawn("/fake/dev/shotbox/target/release/shotbox capture full"),
    (modifiers: [Ctrl, Shift], key: "4"): Spawn("/fake/dev/shotbox/target/release/shotbox capture region"),
    (
        modifiers: [Ctrl, Shift],
        key: "5",
        description: Some("Options"),
    ): Spawn("/fake/dev/shotbox/target/release/shotbox options"),
}"#;

    fn entry_texts(content: &str) -> Vec<String> {
        parse_shortcut_entries(content).into_iter()
            .map(|entry| content[entry.range].to_string())
            .collect()
    }

    #[test]
    fn parses_all_seven_shortcut_fixture_entries() {
        let entries = parse_shortcut_entries(SHORTCUT_FIXTURE);
        assert_eq!(entries.len(), 7);
        assert_eq!(entries[0].spawn.as_deref(), Some("pactl set-sink-volume @DEFAULT_SINK@ +5%"));
        assert_eq!(entries[3].spawn.as_deref(), Some("waveterm"));
        assert_eq!(entries[6].spawn.as_deref(), Some("/fake/dev/shotbox/target/release/shotbox options"));
    }

    #[test]
    fn replaces_only_key_four_and_keeps_other_entries_byte_identical_in_order() {
        let updated = replace_shortcut_entries(
            SHORTCUT_FIXTURE,
            &[("4", "cosmic-screenshot --interactive".into())],
        );
        let before = entry_texts(SHORTCUT_FIXTURE);
        let after = entry_texts(&updated);
        assert_eq!(after.len(), 7);
        assert!(after[5].contains("cosmic-screenshot --interactive"));
        assert_eq!(&after[..5], &before[..5]);
        assert_eq!(&after[6..], &before[6..]);
    }

    #[test]
    fn adds_key_four_when_it_does_not_exist() {
        let without_four = SHORTCUT_FIXTURE.replace(
            "key: \"4\"",
            "key: \"6\"",
        );
        let updated = replace_shortcut_entries(
            &without_four,
            &[("4", "cosmic-screenshot --interactive".into())],
        );
        let entries = parse_shortcut_entries(&updated);
        assert_eq!(entries.len(), 8);
        assert!(entries.iter().any(|entry| is_target_shortcut(entry, "4")));
    }

    #[test]
    fn parses_single_line_ron_shortcut_variant() {
        let content = r#"{(modifiers: [Super, Alt], key: "Escape"): Terminate,}"#;
        let entries = parse_shortcut_entries(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].modifiers, ["Super", "Alt"]);
        assert_eq!(entries[0].key, "Escape");
        assert_eq!(entries[0].spawn, None);
    }

    #[test]
    fn detects_spawn_first_token_executable_presence() {
        assert!(!executable_exists("/definitely/not/a/real/popmgr-command capture"));
        assert!(executable_exists("sh -c true"));
    }

    #[test]
    fn writes_updates_and_backup_only_in_temp_directory() {
        let root = std::env::temp_dir().join(format!(
            "popmgr-shortcut-test-{}",
            std::process::id(),
        ));
        let path = root.join("custom");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, SHORTCUT_FIXTURE).unwrap();
        write_shortcut_updates(&path, &[("4", "cosmic-screenshot --interactive".into())]).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("custom.popmgr.bak")).unwrap(), SHORTCUT_FIXTURE);
        assert!(std::fs::read_to_string(&path).unwrap().contains("cosmic-screenshot --interactive"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
