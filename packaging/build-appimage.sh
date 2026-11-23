#!/usr/bin/env bash
# Build a self-contained BurnRust-x86_64.AppImage.
#
# No FUSE required: packs the AppDir with mksquashfs and concatenates the
# AppImage type-2 runtime (downloaded once and cached). Run from the repo root
# or anywhere; paths are resolved relative to this script.
#
# Requirements: cargo, mksquashfs (squashfs-tools), wget, strip (optional).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
DIST="$ROOT/dist"
BUILD="$ROOT/target/appimage"
RUNTIME="$BUILD/runtime-x86_64"
APPDIR="$BUILD/BurnRust.AppDir"
OUT="$DIST/BurnRust-x86_64.AppImage"
RUNTIME_URL="https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64"

echo ">> Compilando release..."
( cd "$ROOT" && cargo build --release )

echo ">> Preparando AppDir..."
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps"
cp "$ROOT/target/release/BurnRust" "$APPDIR/usr/bin/BurnRust"
strip "$APPDIR/usr/bin/BurnRust" 2>/dev/null || true
cp "$HERE/AppRun"           "$APPDIR/AppRun"
chmod +x                    "$APPDIR/AppRun"
cp "$HERE/BurnRust.desktop" "$APPDIR/BurnRust.desktop"
cp "$HERE/BurnRust.desktop" "$APPDIR/usr/share/applications/BurnRust.desktop"
cp "$HERE/BurnRust.png"     "$APPDIR/BurnRust.png"
cp "$HERE/BurnRust.png"     "$APPDIR/.DirIcon"
cp "$HERE/BurnRust.png"     "$APPDIR/usr/share/icons/hicolor/256x256/apps/BurnRust.png"

if [ ! -f "$RUNTIME" ]; then
  echo ">> Descargando runtime AppImage..."
  mkdir -p "$BUILD"
  wget -q "$RUNTIME_URL" -O "$RUNTIME"
fi

echo ">> Empaquetando squashfs..."
rm -f "$BUILD/BurnRust.sqfs"
mksquashfs "$APPDIR" "$BUILD/BurnRust.sqfs" \
  -root-owned -noappend -comp zstd -Xcompression-level 19 -b 1M >/dev/null

echo ">> Generando AppImage..."
mkdir -p "$DIST"
cat "$RUNTIME" "$BUILD/BurnRust.sqfs" > "$OUT"
chmod +x "$OUT"

echo ">> Listo: $OUT ($(du -h "$OUT" | awk '{print $1}'))"
echo "   Ejecuta:  $OUT"
echo "   Sin FUSE: $OUT --appimage-extract-and-run"
