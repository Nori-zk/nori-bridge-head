#!/bin/bash

# Parse --no-sudo argument
SKIP_SUDO=false
for arg in "$@"; do
    if [ "$arg" = "--no-sudo" ]; then
        SKIP_SUDO=true
        shift
    fi
done

# Step 1: Get the directory where the script is located (absolute path)
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Step 2: Navigate to the script directory
cd "$SCRIPT_DIR"

# Step 3: Navigate to root directory
cd ..

# Step 4: Clear cache forcefully (due to files created with root permissions in target, because docker is used to compute the zk elf [impacts CI])
if [ "$SKIP_SUDO" = true ]; then
    rm -rf target
else
    sudo rm -rf target
fi

# Step 5: Build zk
cd script
cargo run --release --bin make
cd ..

# Step 6: Create public outputs
cd nori
cargo run --release --bin extract_zeroth_public_input