#!/bin/bash

set -e

echo "🧬 Building Eä Nucleus System"

# Configuration
MUSCLES_DIR="muscles"
COMPILER_DIR="muscle-compiler" 
BUNDLES_DIR="bundles"
TARGET="aarch64"

# Create bundles directory
mkdir -p $BUNDLES_DIR

echo "🔨 Step 1: Building enhanced muscle compiler..."
cd $COMPILER_DIR
cargo build --release
cd ..

echo "🔨 Step 2: Compiling nucleus.ea to sealed blob..."
$COMPILER_DIR/target/release/muscle-compiler \
    $MUSCLES_DIR/nucleus.ea \
    --target $TARGET \
    --output $BUNDLES_DIR/nucleus.blob \
    --chaos-master $(openssl rand -hex 32)

# Verify nucleus blob size
NUCLEUS_SIZE=$(stat -f%z $BUNDLES_DIR/nucleus.blob 2>/dev/null || stat -c%s $BUNDLES_DIR/nucleus.blob)
if [ $NUCLEUS_SIZE -ne 8192 ]; then
    echo "❌ Nucleus blob size incorrect: $NUCLEUS_SIZE bytes (expected 8192)"
    exit 1
fi
echo "✅ Nucleus blob: $NUCLEUS_SIZE bytes"

echo "🔨 Step 3: Building pre-nucleus loader..."
cd $MUSCLES_DIR/preloader
cargo build --target x86_64-unknown-uefi --release

# Verify pre-loader size
PRELOADER_SIZE=$(stat -f%z ../target/x86_64-unknown-uefi/release/preloader.efi 2>/dev/null || stat -c%s ../target/x86_64-unknown-uefi/release/preloader.efi)
if [ $PRELOADER_SIZE -gt 2048 ]; then
    echo "❌ Pre-loader size exceeded: $PRELOADER_SIZE bytes (max 2048)"
    exit 1
fi
echo "✅ Pre-loader: $PRELOADER_SIZE bytes"

cd ../..

echo "🔨 Step 4: Creating boot bundle..."
cp $MUSCLES_DIR/preloader/target/x86_64-unknown-uefi/release/preloader.efi $BUNDLES_DIR/
cp $BUNDLES_DIR/nucleus.blob $BUNDLES_DIR/

echo "🎉 Nucleus build complete!"
echo "📦 Boot bundle created in $BUNDLES_DIR/:"
echo "   - preloader.efi (2KiB pre-nucleus loader)"
echo "   - nucleus.blob (8KiB Nucleus Muscle)"
