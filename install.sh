#!/bin/bash
set -e

REPO_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
COSMIC_FILES_SRC="/tmp/popmgr-cosmic-files-src"
COSMIC_FILES_PATCH="/tmp/popmgr-cosmic-files-patch"
COSMIC_COMP_SRC="/tmp/popmgr-cosmic-comp-src"
COSMIC_COMP_PATCH="/tmp/popmgr-cosmic-comp-patch"

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
cp "$REPO_DIR/popmgr.desktop" "$APP_DIR/popmgr.desktop"
echo "  -> $APP_DIR/popmgr.desktop"

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
git checkout bf01bb3 2>&1
git apply "$COSMIC_FILES_PATCH/cosmic-files-copy-path.patch" 2>&1
export LIBCLANG_PATH=/usr/lib/llvm-18/lib
cargo build --release 2>&1
pkexec bash -c "cp -a /usr/bin/cosmic-files /usr/bin/cosmic-files.bak 2>/dev/null || true; install -Dm0755 $COSMIC_FILES_SRC/target/release/cosmic-files /usr/bin/cosmic-files"
echo "  -> copy-path 패치 완료"

# 5. cosmic-comp 3-finger 패치
echo ""
echo "[5/5] COSMIC Comp 3-finger 패치 적용..."
echo "  (빌드 시간: 약 5~10분)"
sudo apt-get install -y libinput-dev libudev-dev libgbm-dev libseat-dev libwayland-dev libpixman-1-dev 2>&1 | grep -E "^(Reading|Setting|Unpacking|Get:)" || true

rm -rf "$COSMIC_COMP_SRC" "$COSMIC_COMP_PATCH"
git clone --depth 1 https://github.com/eondcom/cosmic-three-finger-gesture "$COSMIC_COMP_PATCH" 2>&1
git clone https://github.com/pop-os/cosmic-comp "$COSMIC_COMP_SRC" 2>&1
cd "$COSMIC_COMP_SRC"
git checkout 22fe419 2>&1
git apply "$COSMIC_COMP_PATCH/three-finger-gesture.patch" 2>&1
cargo build --release 2>&1
pkexec bash -c "cp -a /usr/bin/cosmic-comp /usr/bin/cosmic-comp.bak 2>/dev/null || true; install -Dm0755 $COSMIC_COMP_SRC/target/release/cosmic-comp /usr/bin/cosmic-comp"
echo "  -> 3-finger 패치 완료"

# 패치 마커 기록
MARKER_DIR="$HOME/.local/share/popmgr"
mkdir -p "$MARKER_DIR"
echo '{"copy_path":true,"three_finger":true}' > "$MARKER_DIR/patches.json"

echo ""
echo "=== 설치 완료 ==="
echo ""
echo "  실행: popmgr"
echo "  또는 앱 런처에서 'popmgr' 검색"
echo ""
echo "  복구 (패치 제거):"
echo "    sudo cp /usr/bin/cosmic-files.bak /usr/bin/cosmic-files"
echo "    sudo cp /usr/bin/cosmic-comp.bak  /usr/bin/cosmic-comp"
