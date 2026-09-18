#!/bin/sh
# Build a double-clickable Muxdock.app in target/ (macOS only).
set -e
cd "$(dirname "$0")/.."
cargo build --release
APP=target/Muxdock.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp target/release/muxdock "$APP/Contents/MacOS/muxdock"

# App icon: build an .icns from the PNG with the tools that ship with macOS.
mkdir -p "$APP/Contents/Resources"
ICONSET=target/muxdock.iconset
rm -rf "$ICONSET"; mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z $size $size assets/icon-512.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
done
sips -z 32 32 assets/icon-512.png --out "$ICONSET/icon_16x16@2x.png" >/dev/null
sips -z 64 64 assets/icon-512.png --out "$ICONSET/icon_32x32@2x.png" >/dev/null
sips -z 256 256 assets/icon-512.png --out "$ICONSET/icon_128x128@2x.png" >/dev/null
sips -z 512 512 assets/icon-512.png --out "$ICONSET/icon_256x256@2x.png" >/dev/null
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/muxdock.icns"
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
  <key>CFBundleIconFile</key><string>muxdock</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSAppleEventsUsageDescription</key><string>Muxdock opens tmux sessions in Terminal or iTerm.</string>
</dict>
</plist>
PLIST
echo "Built $APP. Drag it to /Applications or run: open $APP"
