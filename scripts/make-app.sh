#!/bin/sh
# Build a double-clickable Muxdock.app in target/ (macOS only).
set -e
cd "$(dirname "$0")/.."
cargo build --release
APP=target/Muxdock.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp target/release/muxdock "$APP/Contents/MacOS/muxdock"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Muxdock</string>
  <key>CFBundleDisplayName</key><string>Muxdock</string>
  <key>CFBundleIdentifier</key><string>com.markkimura.muxdock</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>muxdock</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSAppleEventsUsageDescription</key><string>Muxdock opens tmux sessions in Terminal or iTerm.</string>
</dict>
</plist>
PLIST
echo "Built $APP. Drag it to /Applications or run: open $APP"
