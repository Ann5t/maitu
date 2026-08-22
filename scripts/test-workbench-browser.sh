#!/usr/bin/env bash
set -euo pipefail

browser_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
browser_suffix="$$"
browser_network="fudian-browser-test-$browser_suffix"
browser_db="fudian-browser-db-$browser_suffix"
browser_app="fudian-browser-app-$browser_suffix"
browser_image="mcr.microsoft.com/playwright:v1.62.0-noble"
browser_screenshot_dir="${SCREENSHOT_DIR:-$browser_repo_root/docs/screenshots}"
browser_worker_bootstrap="browser_worker_bootstrap_0123456789abcdef"

cleanup_browser_stack() {
  local exit_status="$?"
  if (( exit_status != 0 )) && docker inspect "$browser_app" >/dev/null 2>&1; then
    docker logs "$browser_app" >&2 || true
  fi
  [[ "$browser_app" == fudian-browser-app-* ]] \
    && docker rm -f "$browser_app" >/dev/null 2>&1 || true
  [[ "$browser_db" == fudian-browser-db-* ]] \
    && docker rm -f "$browser_db" >/dev/null 2>&1 || true
  [[ "$browser_network" == fudian-browser-test-* ]] \
    && docker network rm "$browser_network" >/dev/null 2>&1 || true
  return "$exit_status"
}
trap cleanup_browser_stack EXIT

docker image inspect "$browser_image" >/dev/null
mkdir -p "$browser_screenshot_dir"
browser_screenshot_dir="$(cd "$browser_screenshot_dir" && pwd)"
docker network create "$browser_network" >/dev/null
docker run -d --name "$browser_db" --network "$browser_network" \
  --network-alias browser-db \
  -e POSTGRES_USER=fudian_test -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test postgres:17-alpine >/dev/null
for browser_attempt in $(seq 1 30); do
  if docker exec "$browser_db" pg_isready -U fudian_test -d fudian_test >/dev/null 2>&1; then break; fi
  [[ "$browser_attempt" == 30 ]] && docker logs "$browser_db" && exit 1
  sleep 1
done

docker run -d --name "$browser_app" --network "$browser_network" \
  --network-alias browser-app \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@browser-db:5432/fudian_test \
  -e FUDIAN_BIND=0.0.0.0:3000 -e ARTIFACT_ROOT=/tmp/fudian-browser-artifacts \
  -e FUDIAN_WORKER_BOOTSTRAP_TOKEN="$browser_worker_bootstrap" \
  -e RUST_LOG=fudian=info \
  --mount "type=bind,src=$browser_repo_root,dst=/app" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  --mount type=volume,src=fudian_rust_target,dst=/app/target \
  fudian-nextgen-app:latest cargo run >/dev/null
for browser_attempt in $(seq 1 60); do
  if docker exec "$browser_app" bash -c 'exec 3<>/dev/tcp/127.0.0.1/3000' >/dev/null 2>&1; then break; fi
  [[ "$browser_attempt" == 60 ]] && docker logs "$browser_app" && exit 1
  sleep 1
done

docker run --rm --init --ipc=host --network "$browser_network" \
  -e BASE_URL=http://browser-app:3000 \
  -e SCREENSHOT_DIR=/screenshots \
  -e WORKER_BOOTSTRAP="$browser_worker_bootstrap" \
  --mount "type=bind,src=$browser_repo_root,dst=/work" \
  --mount "type=bind,src=$browser_screenshot_dir,dst=/screenshots" \
  --mount type=volume,src=fudian_playwright_npm_cache,dst=/root/.npm \
  "$browser_image" sh -c '
    npm install --prefix /tmp/pw --no-audit --no-fund @playwright/test@1.62.0 >/dev/null &&
    cp /work/tests/browser/workbench.spec.js /tmp/pw/workbench.spec.js &&
    cp /work/tests/browser/ideas.spec.js /tmp/pw/ideas.spec.js &&
    cd /tmp/pw &&
    ./node_modules/.bin/playwright test workbench.spec.js ideas.spec.js --reporter=line --workers=1
  '

test -s "$browser_screenshot_dir/goal-workbench-desktop.png"
test -s "$browser_screenshot_dir/goal-workbench-mobile.png"
test -s "$browser_screenshot_dir/ideas-map-desktop.png"
test -s "$browser_screenshot_dir/ideas-map-tablet.png"
test -s "$browser_screenshot_dir/idea-detail-mobile.png"
echo "Chromium passed: Goal workbench plus Idea/ProjectProposal flow at desktop, tablet and mobile sizes"
