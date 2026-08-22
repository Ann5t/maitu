#!/usr/bin/env bash
set -euo pipefail

plugin_test_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
plugin_test_tmp="$(mktemp -d)"

cleanup_plugin_test() {
  local exit_status="$?"
  [[ "$plugin_test_tmp" == /tmp/tmp.* ]] && rm -rf "$plugin_test_tmp"
  return "$exit_status"
}
trap cleanup_plugin_test EXIT

chmod 755 "$plugin_test_tmp"
mkdir -p "$plugin_test_tmp/input" "$plugin_test_tmp/output" "$plugin_test_tmp/result"
chmod 755 "$plugin_test_tmp/input" "$plugin_test_tmp/result"
chmod 777 "$plugin_test_tmp/output"
cp "$plugin_test_root"/tests/fixtures/plugins/{valid.rs,library_probe.py,valid.c,valid.cpp,page.html} \
  "$plugin_test_tmp/input/"

run_plugin_tool() {
  local image="$1"
  local plugin_id="$2"
  local plugin_version="$3"
  local tool_name="$4"
  local input_json="$5"
  docker run --rm \
    --network none \
    --read-only \
    --user 1000:1000 \
    --cap-drop ALL \
    --security-opt no-new-privileges \
    --pids-limit 128 \
    --memory 1g \
    --cpus 1 \
    --tmpfs /tmp:rw,nosuid,nodev,size=268435456,mode=1777 \
    -e FUDIAN_INPUT=/workspace/input \
    -e FUDIAN_OUTPUT=/workspace/output \
    --mount "type=bind,src=$plugin_test_tmp/input,dst=/workspace/input,readonly" \
    --mount "type=bind,src=$plugin_test_tmp/output,dst=/workspace/output" \
    --mount "type=bind,src=$plugin_test_tmp/result,dst=/workspace/result" \
    --entrypoint /runtime/fudian-tool-runtime \
    "$image" execute "$plugin_id" "$plugin_version" "$tool_name" "$input_json"
}

run_plugin_tool fudian-plugin-rust:1.0.0 fudian.tools.rust 1.0.0 check \
  '{"sourcePath":"valid.rs","reportPath":"rust.json"}'
run_plugin_tool fudian-plugin-python:1.0.0 fudian.tools.python 1.0.0 run \
  '{"sourcePath":"library_probe.py","reportPath":"python-v1.json"}'
run_plugin_tool fudian-plugin-python:2.0.0 fudian.tools.python 2.0.0 run \
  '{"sourcePath":"library_probe.py","reportPath":"python-v2.json"}'
run_plugin_tool fudian-plugin-cxx:1.0.0 fudian.tools.cxx 1.0.0 check \
  '{"sourcePath":"valid.c","language":"c","reportPath":"c.json"}'
run_plugin_tool fudian-plugin-cxx:1.0.0 fudian.tools.cxx 1.0.0 check \
  '{"sourcePath":"valid.cpp","language":"cpp","reportPath":"cpp.json"}'
run_plugin_tool fudian-plugin-playwright:1.0.0 fudian.tools.playwright 1.0.0 inspect \
  '{"sourcePath":"page.html","screenshotPath":"page.png","reportPath":"playwright.json"}'

python3 - "$plugin_test_tmp/output" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
reports = [
    "rust.json",
    "python-v1.json",
    "python-v2.json",
    "c.json",
    "cpp.json",
    "playwright.json",
]
for name in reports:
    report = json.loads((root / name).read_text())
    assert report["succeeded"], (name, report)
assert "legacy:sample" in json.loads((root / "python-v1.json").read_text())["stdout"]
assert "modern:sample" in json.loads((root / "python-v2.json").read_text())["stdout"]
assert (root / "page.png").stat().st_size > 1_000
PY

echo "direct plugin image smoke passed: Rust, Python v1/v2, C, C++, Playwright/Chromium"
