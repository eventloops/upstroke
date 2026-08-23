#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || -z "$1" ]]; then
  echo "usage: validate-release-record.sh <expected-tag>" >&2
  exit 2
fi

expected_tag="$1"
expected_assets='["upstroke-aarch64-apple-darwin.tar.gz","upstroke-x86_64-pc-windows-msvc.zip","upstroke-x86_64-unknown-linux-gnu.tar.gz"]'

if ! jq -e --arg tag "$expected_tag" --argjson expected "$expected_assets" '
  (.tag_name == $tag) and
  (.immutable == true) and
  (.draft == false) and
  (([.assets[].name] | sort) == ($expected | sort)) and
  all(.assets[]; (.state == "uploaded") and ((.digest // "") | test("^sha256:[0-9a-f]{64}$")))
' >/dev/null; then
  echo "release must be published, immutable, and contain exactly the three digested assets" >&2
  exit 1
fi
