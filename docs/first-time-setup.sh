#!/usr/bin/env bash

# This script attempts to install all necessary dependencies
# for a Linux / macOS development environment. It's provided
# as a convenience, automating the manual installation steps.
# When in doubt, trust the manual installation guide.

set -euf -o pipefail

# Rustup
if ! command -v rustup &> /dev/null
then
    echo "[ ] Installing Rust..."
    curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf | sh
    echo "[✓] Installed Rust"
else
    echo "[✓] Found Rust installation"
fi

# thumbv7em-none-eabihf
echo "[ ] Installing thumbv7em-none-eabihf target..."
rustup target add thumbv7em-none-eabihf
echo "[✓] Installed (or found existing) thumbv7em-none-eabihf target"

# llvm-tools-preview
echo "[ ] Installing llvm-tools-preview"
rustup component add llvm-tools-preview
echo "[✓] Installed (or found existing) llvm-tools-preview"

# cargo-binutils
if ! cargo objcopy --version &> /dev/null
then
    echo "[ ] Installing cargo-binutils..."
    cargo install cargo-binutils
    echo "[✓] Installed cargo binutils"
else
    echo "[✓] Found cargo binutils"
fi

# Sanity-check llvm-tools-preview and cargo-binutils
if ! rust-objcopy --version &> /dev/null
then
    echo "[!!] Something wrong with llvm-tools-preview and cargo-binutils installation!"
    exit 1
else
    echo "[✓] OK llvm-tools-preview and cargo-binutils installation"
fi

echo "[ ] Attempting build of all embedded and host-side Rust code..."
pushd . &> /dev/null
cd ..
cargo build --target thumbv7em-none-eabihf --workspace
cd tools/pymotion-sensor
cargo build
popd &> /dev/null
echo "[✓] OK build of all embedded and host-side Rust code"

# Check for Python installation with minimum version
# for versions. TODO check version.
if ! python -c "import sys; assert sys.version_info[0] == 3 and sys.version_info[1] > 6" &> /dev/null
then
    echo "[!] Cannot find Python interpreter at 'python', or installation is not the 3.7 min version"
    echo "[!] Make sure you have at least a Python 3.7 installation available"
else
    echo "[✓] Found Python installation with interpreter 'python' and min version 3.7"
fi

echo "[✓] DONE. Make sure that you address any warnings [!] generated by this script."