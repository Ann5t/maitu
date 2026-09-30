#!/usr/bin/env bash
set -euo pipefail

secure_compose_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
secure_compose_suffix="$$"
secure_compose_project="fudian-secure-test-$secure_compose_suffix"
secure_compose_file="$secure_compose_repo_root/compose.secure.yaml"
secure_compose_tmp="$(mktemp -d)"
secure_compose_env="$secure_compose_tmp/compose.env"
secure_compose_host="secure-compose.test"
secure_compose_port="$(python3 -c 'import socket
s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
secure_compose_network_octet="$((secure_compose_suffix % 240 + 10))"
secure_compose_origin="https://$secure_compose_host:$secure_compose_port"
secure_compose_image="${FUDIAN_SECURE_COMPOSE_IMAGE:-fudian-nextgen-runtime:bp09-test}"
secure_compose_postgres_image="postgres:17-alpine@sha256:18cfe3ef5e6815560c98237d6216d1e5119702fb0f3894c8785dd58b8bbe5d73"
secure_compose_password="Secure-compose-passphrase-2026"
secure_compose_setup_token="secure_compose_setup_token_${secure_compose_suffix}_abcdef"
secure_compose_db_password="secure_compose_db_${secure_compose_suffix}_abcdef"
secure_compose_pepper="secure_compose_pepper_${secure_compose_suffix}_abcdef"
secure_compose_worker="secure_compose_worker_${secure_compose_suffix}_abcdef"

secure_compose() {
  docker compose -p "$secure_compose_project" -f "$secure_compose_file" \
    --env-file "$secure_compose_env" "$@"
}

cleanup_secure_compose() {
  local exit_status="$?"
  if (( exit_status != 0 )) && [[ "$secure_compose_project" == fudian-secure-test-* ]]; then
    secure_compose ps -a >&2 || true
    secure_compose logs --no-color >&2 || true
  fi
  if [[ "$secure_compose_project" == fudian-secure-test-* ]]; then
    secure_compose down --volumes --remove-orphans --timeout 10 >/dev/null 2>&1 || true
  fi
  if [[ "$secure_compose_tmp" == /tmp/tmp.* && -d "$secure_compose_tmp" ]]; then
    rm -rf -- "$secure_compose_tmp"
  fi
  return "$exit_status"
}
trap cleanup_secure_compose EXIT

secure_curl() {
  curl --silent --show-error --insecure --noproxy '*' \
    --resolve "$secure_compose_host:$secure_compose_port:127.0.0.1" "$@"
}

mkdir -p "$secure_compose_tmp/secrets"
chmod 0700 "$secure_compose_tmp/secrets"
printf '%s' "$secure_compose_db_password" \
  >"$secure_compose_tmp/secrets/postgres-password"
printf 'postgres://fudian:%s@postgres:5432/fudian' "$secure_compose_db_password" \
  >"$secure_compose_tmp/secrets/database-url"
printf '%s' "$secure_compose_pepper" >"$secure_compose_tmp/secrets/auth-pepper"
printf '%s' "$secure_compose_setup_token" >"$secure_compose_tmp/secrets/setup-token"
printf '%s' "$secure_compose_worker" >"$secure_compose_tmp/secrets/worker-bootstrap-token"
chmod 0400 "$secure_compose_tmp/secrets/postgres-password"
docker run --rm --network none --user 0:0 \
  --mount "type=bind,src=$secure_compose_tmp/secrets,dst=/secrets" \
  "$secure_compose_postgres_image" \
  sh -ec 'chown 1000:1000 /secrets/database-url /secrets/auth-pepper \
    /secrets/setup-token /secrets/worker-bootstrap-token
    chmod 0400 /secrets/database-url /secrets/auth-pepper \
      /secrets/setup-token /secrets/worker-bootstrap-token'

printf '%s\n' \
  "FUDIAN_COMPOSE_PROJECT=$secure_compose_project" \
  "FUDIAN_SITE_HOST=$secure_compose_host" \
  "FUDIAN_PUBLIC_ORIGIN=$secure_compose_origin" \
  'FUDIAN_HTTPS_BIND=127.0.0.1' \
  "FUDIAN_HTTPS_PORT=$secure_compose_port" \
  "FUDIAN_CADDYFILE=$secure_compose_repo_root/deploy/Caddyfile.private" \
  "FUDIAN_EDGE_SUBNET=10.201.$secure_compose_network_octet.0/24" \
  "FUDIAN_CADDY_EDGE_IP=10.201.$secure_compose_network_octet.10" \
  "FUDIAN_APP_EDGE_IP=10.201.$secure_compose_network_octet.20" \
  "FUDIAN_DATA_SUBNET=10.202.$secure_compose_network_octet.0/24" \
  "FUDIAN_POSTGRES_DATA_IP=10.202.$secure_compose_network_octet.10" \
  "FUDIAN_APP_DATA_IP=10.202.$secure_compose_network_octet.20" \
  "FUDIAN_TOOLS_SUBNET=10.203.$secure_compose_network_octet.0/24" \
  "FUDIAN_APP_TOOLS_IP=10.203.$secure_compose_network_octet.10" \
  "FUDIAN_APP_IMAGE=$secure_compose_image" \
  'RUNNER_RUNTIME_DIGEST=sha256:0000000000000000000000000000000000000000000000000000000000000000' \
  "FUDIAN_POSTGRES_PASSWORD_FILE=$secure_compose_tmp/secrets/postgres-password" \
  "FUDIAN_DATABASE_URL_FILE=$secure_compose_tmp/secrets/database-url" \
  "FUDIAN_AUTH_PEPPER_FILE=$secure_compose_tmp/secrets/auth-pepper" \
  "FUDIAN_SETUP_TOKEN_FILE=$secure_compose_tmp/secrets/setup-token" \
  "FUDIAN_WORKER_BOOTSTRAP_TOKEN_FILE=$secure_compose_tmp/secrets/worker-bootstrap-token" \
  'FUDIAN_APP_MEMORY=768m' 'FUDIAN_APP_CPUS=1.0' \
  'FUDIAN_POSTGRES_MEMORY=512m' 'FUDIAN_POSTGRES_CPUS=1.0' \
  'FUDIAN_CADDY_MEMORY=128m' 'FUDIAN_CADDY_CPUS=0.5' \
  >"$secure_compose_env"
chmod 0600 "$secure_compose_env"

docker image inspect "$secure_compose_image" >/dev/null
secure_compose config --quiet
secure_compose up -d --no-build

for secure_compose_attempt in $(seq 1 90); do
  if secure_curl --fail "$secure_compose_origin/api/health" >/dev/null 2>&1; then
    break
  fi
  if [[ "$secure_compose_attempt" == 90 ]]; then
    echo "安全 Compose 未在期限内提供 HTTPS 健康端点" >&2
    exit 1
  fi
  sleep 1
done

secure_compose_app="$(secure_compose ps -q app)"
secure_compose_db="$(secure_compose ps -q postgres)"
secure_compose_caddy="$(secure_compose ps -q caddy)"
test -n "$secure_compose_app"
test -n "$secure_compose_db"
test -n "$secure_compose_caddy"

python3 -c 'import json,subprocess,sys
app,db,caddy,port=sys.argv[1:]
def inspect(container):
    return json.loads(subprocess.check_output(["docker","inspect",container]))[0]
a,d,c=map(inspect,(app,db,caddy))
assert a["Config"]["User"] == "1000:1000"
assert c["Config"]["User"] == "1000:1000"
for value in (a,d,c):
    host=value["HostConfig"]
    assert host["ReadonlyRootfs"] is True
    assert host["PidsLimit"] > 0
    assert host["Memory"] > 0 and host["NanoCpus"] > 0
    assert host["SecurityOpt"] == ["no-new-privileges:true"]
    assert host["LogConfig"]["Config"]["max-size"] == "10m"
    assert host["LogConfig"]["Config"]["max-file"] == "5"
assert "ALL" in a["HostConfig"]["CapDrop"]
assert "ALL" in c["HostConfig"]["CapDrop"]
assert not a["NetworkSettings"]["Ports"].get("3000/tcp")
assert not d["NetworkSettings"]["Ports"].get("5432/tcp")
bindings=c["NetworkSettings"]["Ports"]["443/tcp"]
assert bindings == [{"HostIp":"127.0.0.1","HostPort":port}], bindings
assert len(a["NetworkSettings"]["Networks"]) == 3
assert len(d["NetworkSettings"]["Networks"]) == 1
assert len(c["NetworkSettings"]["Networks"]) == 2
assert a["RestartCount"] == d["RestartCount"] == c["RestartCount"] == 0
' "$secure_compose_app" "$secure_compose_db" "$secure_compose_caddy" \
  "$secure_compose_port"

for secure_compose_network in edge data tools; do
  secure_compose_network_id="$(secure_compose config --format json \
    | python3 -c 'import json,sys
value=json.load(sys.stdin); print(value["networks"][sys.argv[1]]["name"])' \
      "$secure_compose_network")"
  [[ "$(docker network inspect -f '{{.Internal}}' "$secure_compose_network_id")" == true ]]
done
secure_compose_ingress_name="$(secure_compose config --format json \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["networks"]["ingress"]["name"])')"
[[ "$(docker network inspect -f '{{.Internal}}' "$secure_compose_ingress_name")" == false ]]
[[ "$(docker network inspect -f '{{len .Containers}}' "$secure_compose_ingress_name")" == 1 ]]

if docker inspect "$secure_compose_app" "$secure_compose_db" "$secure_compose_caddy" \
  | grep -F -e "$secure_compose_db_password" -e "$secure_compose_pepper" \
    -e "$secure_compose_setup_token" -e "$secure_compose_worker" >/dev/null; then
  echo "Docker 元数据泄漏了文件型 secret 的实际值" >&2
  exit 1
fi

secure_compose_setup_status="$(secure_curl \
  -D "$secure_compose_tmp/setup.headers" -c "$secure_compose_tmp/session.jar" \
  -o "$secure_compose_tmp/setup.body" -w '%{http_code}' \
  -H "Origin: $secure_compose_origin" \
  --data-urlencode 'username=owner' \
  --data-urlencode "password=$secure_compose_password" \
  --data-urlencode "password_confirm=$secure_compose_password" \
  --data-urlencode "setup_token=$secure_compose_setup_token" \
  "$secure_compose_origin/auth/setup")"
[[ "$secure_compose_setup_status" == 200 ]]
grep -qi 'strict-transport-security: max-age=31536000' "$secure_compose_tmp/setup.headers"
grep -qi '__Host-fudian_session=.*Secure; HttpOnly; SameSite=Strict' \
  "$secure_compose_tmp/setup.headers"
secure_curl --fail -b "$secure_compose_tmp/session.jar" "$secure_compose_origin/" \
  >"$secure_compose_tmp/dashboard.html"
grep -q '最近项目' "$secure_compose_tmp/dashboard.html"

secure_compose_migrations="$(secure_compose exec -T postgres \
  psql -U fudian -d fudian -Atc 'SELECT count(*) FROM schema_migrations;')"
[[ "$secure_compose_migrations" == 15 ]]

echo "secure compose passed: file secrets, internal networks, no app/database ports, HTTPS-only gateway, non-root read-only services and bounded resources"
