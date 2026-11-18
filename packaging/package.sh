#!/usr/bin/env bash
# 配布する zip を作る。リリース（release.yml）と CI（ci.yml）の両方から、Windows の Git Bash で呼ぶ。
#
# 使い方: bash packaging/package.sh <出力先のフォルダ>
#   先に `cargo build --release --locked -p xml-viewer` で exe を作っておく。
#   出力先には zip（xml-viewer-v<バージョン>.zip）と、その SHA-256（.sha256）だけを置く。
#
# zip の中身:
#   xml-viewer.exe
#   README.txt                 README.md をそのまま入れる
#   LICENSE.txt
#   THIRD-PARTY-LICENSES.txt   exe に含まれるクレートのライセンス（cargo-about）
set -euo pipefail

out=${1:?出力先のフォルダを指定してください}
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [ -z "$version" ]; then
  echo "Cargo.toml からバージョンを読めません" >&2
  exit 1
fi
name="xml-viewer-v$version.zip"

stage=target/package
rm -rf "$stage"
mkdir -p "$stage" "$out"
cp target/release/xml-viewer.exe "$stage/"
cp README.md "$stage/README.txt"
cp LICENSE "$stage/LICENSE.txt"
cargo about generate --workspace --locked --fail about.hbs -o "$stage/THIRD-PARTY-LICENSES.txt"

# Git Bash には zip コマンドが無いので、PowerShell で固める。
rm -f "$out/$name"
pwsh -NoLogo -NoProfile -NonInteractive -Command "Compress-Archive -Path '$stage/*' -DestinationPath '$out/$name'"
(cd "$out" && sha256sum "$name" > "$name.sha256")

echo "$out/$name"
