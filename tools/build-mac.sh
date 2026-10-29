#!/bin/bash
# Build KOOL SKULL STUDIO on macOS, from the repository root.
# The app stays inside this checkout so a drawing can open through the Python bridge.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "tools/build-mac.sh runs on macOS. This machine is $(uname -s)."
  exit 1
fi

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing $1. $2"
    exit 1
  fi
}

need python3 "Install Python 3.12 or newer."
need cargo "Install Rust from https://rustup.rs and open a new terminal."
need clang "Install the Xcode command line tools: xcode-select --install"

py_ok="$(python3 -c 'import sys; print(int(sys.version_info >= (3, 12)))')"
if [ "$py_ok" != "1" ]; then
  echo "Python 3.12 or newer is required. This python3 is $(python3 --version 2>&1)."
  exit 1
fi

if [ ! -x .venv/bin/python ]; then
  python3 -m venv .venv
fi
.venv/bin/python -m pip install --upgrade pip
.venv/bin/python -m pip install -r requirements.txt

clang -O3 -shared -fPIC -o vmi_studio/lzf_d.so vmi_studio/lzf_d.c
cargo build --locked --release --manifest-path tools/vmi-clip-vector/Cargo.toml
cargo build --locked -p vmi-app --release

app="$root/dist/KOOL SKULL STUDIO.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp -f target/release/vmi-studio "$app/Contents/MacOS/vmi-studio"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>KOOL SKULL STUDIO</string>
  <key>CFBundleDisplayName</key>
  <string>KOOL SKULL STUDIO</string>
  <key>CFBundleIdentifier</key>
  <string>com.beetlelabs.vmistudio</string>
  <key>CFBundleExecutable</key>
  <string>vmi-studio</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.03</string>
  <key>CFBundleVersion</key>
  <string>0.03</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key>
  <true/>
</dict>
</plist>
PLIST

codesign --force --sign - "$app/Contents/MacOS/vmi-studio"
codesign --force --sign - "$app"

echo "Built $app"
echo "Open it from this checkout. A blueprint opens on its own. Any other drawing uses .venv."
echo "  open \"$app\""
