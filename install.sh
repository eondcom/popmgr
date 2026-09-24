#!/bin/bash
set -e

REPO_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
COSMIC_FILES_SRC="/tmp/popmgr-cosmic-files-src"
COSMIC_FILES_PATCH="/tmp/popmgr-cosmic-files-patch"

echo "=== popmgr 설치 시작 ==="

# 의존성 확인
for cmd in cargo git pkexec; do
    if ! command -v "$cmd" &>/dev/null; then
        echo "[오류] $cmd 가 필요합니다."
        echo "  sudo apt-get install -y cargo rustc git policykit-1"
        exit 1
    fi
done

# 1. popmgr 빌드
echo ""
echo "[1/5] popmgr 빌드..."
cd "$REPO_DIR"
cargo build --release 2>&1

# 2. 바이너리 설치
echo ""
echo "[2/5] 바이너리 설치..."
mkdir -p "$BIN_DIR"
cp target/release/popmgr "$BIN_DIR/popmgr"
echo "  -> $BIN_DIR/popmgr"

# 3. 아이콘 + .desktop 등록
echo ""
echo "[3/5] 아이콘 및 앱 런처 등록..."
mkdir -p "$APP_DIR"
DESKTOP_ID="com.eondcom.Popmgr.desktop"
cp "$REPO_DIR/$DESKTOP_ID" "$APP_DIR/$DESKTOP_ID"
echo "  -> $APP_DIR/$DESKTOP_ID"

# 구버전은 popmgr.desktop 으로 설치돼 앱 런처에 항목이 중복으로 뜬다.
# StartupWMClass 도 실제 app id(com.eondcom.Popmgr)와 달라 창이 독 아이콘에 붙지 않았다.
if [ -f "$APP_DIR/popmgr.desktop" ]; then
    rm -f "$APP_DIR/popmgr.desktop"
    echo "  -> 구버전 중복 항목 제거: $APP_DIR/popmgr.desktop"
fi

for SIZE in 16 32 48 64 128 256; do
    ICON_DIR="$HOME/.local/share/icons/hicolor/${SIZE}x${SIZE}/apps"
    mkdir -p "$ICON_DIR"
    convert "$REPO_DIR/assets/popmgr-icon.png" -resize ${SIZE}x${SIZE} "$ICON_DIR/popmgr.png" 2>/dev/null || \
        cp "$REPO_DIR/assets/popmgr-icon.png" "$ICON_DIR/popmgr.png"
done
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor/" 2>/dev/null || true
update-desktop-database "$APP_DIR" 2>/dev/null || true
echo "  -> 아이콘 설치 완료 (hicolor 16~256px)"

# 4. cosmic-files copy-path 패치
echo ""
echo "[4/5] COSMIC Files copy-path 패치 적용..."
echo "  (빌드 시간: 약 3~5분)"
sudo apt-get install -y libclang-dev libglib2.0-dev libxkbcommon-dev pkg-config 2>&1 | grep -E "^(Reading|Setting|Unpacking|Get:)" || true

rm -rf "$COSMIC_FILES_SRC" "$COSMIC_FILES_PATCH"
git clone --depth 1 https://github.com/eondcom/cosmic-files-copy-path "$COSMIC_FILES_PATCH" 2>&1
git clone https://github.com/pop-os/cosmic-files "$COSMIC_FILES_SRC" 2>&1
cd "$COSMIC_FILES_SRC"
# 설치된 패키지와 같은 커밋으로 빌드한다 (고정 커밋이면 파일 관리자가 구버전으로 내려간다)
CF_VER=$(dpkg -l cosmic-files 2>/dev/null | awk '/^ii/ {print $3}')
CF_COMMIT=$(echo "$CF_VER" | rev | cut -d'~' -f1 | rev)
git checkout "$CF_COMMIT" 2>&1
# fuzz 를 키우지 않는다 — 컨텍스트가 무시돼 다른 블록에 조용히 붙을 수 있다
patch -p1 < "$COSMIC_FILES_PATCH/cosmic-files-copy-path.patch"
N=$(grep -A1 'menu_item(fl!("copy"), Action::Copy)' src/menu.rs | grep -c 'Action::CopyPath' || true)
if [ "$N" != "2" ]; then
    echo "  !! 패치 검증 실패(메뉴 2곳 중 $N 곳) — cosmic-files 는 설치하지 않습니다"
else
    LIBCLANG_PATH=$(ls -d /usr/lib/llvm-*/lib 2>/dev/null | sort -V | tail -1)
    export LIBCLANG_PATH
    cargo build --release 2>&1
    pkexec bash -c "cp -a /usr/bin/cosmic-files /usr/bin/cosmic-files.bak 2>/dev/null || true; install -Dm0755 $COSMIC_FILES_SRC/target/release/cosmic-files /usr/bin/cosmic-files"
    echo "  -> copy-path 패치 완료"
    MARKER_DIR="$HOME/.local/share/popmgr"
    mkdir -p "$MARKER_DIR"
    echo "{\"copy_path\":true,\"three_finger\":false,\"copy_path_ver\":\"$CF_VER\"}" > "$MARKER_DIR/patches.json"
fi

# 5. 3손가락 제스처 — 컴포지터 패치 대신 popmgr 사용자 서비스 (2026-09-24 변경)
echo ""
echo "[5/5] 3손가락 제스처: popmgr → COSMIC 탭 → '3손가락 제스처' 켜기 (root·재빌드 불필요)"

echo ""
echo "=== 설치 완료 ==="
echo ""
echo "  실행: popmgr"
echo "  또는 앱 런처에서 'popmgr' 검색"
echo ""
echo "  복구 (패치 제거):"
echo "    sudo cp /usr/bin/cosmic-files.bak /usr/bin/cosmic-files"
echo "    (예전 3-finger 컴포지터 패치가 있다면) sudo apt-get install --reinstall cosmic-comp"
