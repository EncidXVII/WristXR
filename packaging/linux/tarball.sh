#!/usr/bin/env sh
# Copyright (C) 2026 EncidXVII
# SPDX-License-Identifier: GPL-3.0-or-later

# Builds the Linux tarball (x64 or arm64; arm64 doubles as the Steam Frame beta) into dist/release/.
# Usage: packaging/linux/tarball.sh <x64|arm64>
set -eu
arch="$1"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
cargo build --release -p wxr-app
name="WristXR-$version-linux-$arch"
stage="dist/release/$name"
rm -rf "$stage"
mkdir -p "$stage"
cp target/release/wxr-app "$stage/wristxr"
cp README.md LICENSE NOTICE THIRD_PARTY_NOTICES.md packaging/linux/install.sh packaging/linux/uninstall.sh "$stage/"
chmod +x "$stage/install.sh" "$stage/uninstall.sh"
tar -C dist/release -czf "dist/release/$name.tar.gz" "$name"
(cd dist/release && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
echo "dist/release/$name.tar.gz"
