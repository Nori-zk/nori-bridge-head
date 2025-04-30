#!/bin/bash

# Step 1: Get the directory where the script is located (absolute path)
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Step 2: Navigate to the script directory
cd "$SCRIPT_DIR"

# Step 3: Navigate to root directory
cd ..

# Step 4: Clear cache forcefully (due to files created with root permissions in target, because docker is used to compute the zk elf [impacts CI])
sudo rm -rf target

# Step 5: Build zk
cd script
cargo run --bin make
cd ..

# Step 6: Create public outputs
cd nori
cargo run --bin extract_zeroth_public_input