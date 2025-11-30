#!/bin/bash

set -e

echo "🔬 Building Muscle Nucleus - The Biological Kernel"

# Build the nucleus
cd nucleus
cargo build --release

# Verify kernel size
KERNEL_SIZE=$(stat -f%z target/x86_64-unknown-none/release/libnucleus.a 2>/dev/null || stat -c%s target/x86_64-unknown-none/release/libnucleus.a)
MAX_SIZE=8192

if [ $KERNEL_SIZE -gt $MAX_SIZE ]; then
    echo "❌ Kernel size exceeded: ${KERNEL_SIZE} > ${MAX_SIZE}"
    exit 1
else
    echo "✅ Kernel size: ${KERNEL_SIZE} bytes (max: ${MAX_SIZE})"
fi

# Run tests
echo "🧪 Running tests..."
cargo test

echo "🎉 Muscle Nucleus build complete!"
