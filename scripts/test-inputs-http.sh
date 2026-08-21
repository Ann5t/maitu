#!/usr/bin/env bash
set -euo pipefail

input_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
input_suffix="$$"
input_network="fudian-input-test-$input_suffix"
input_db="fudian-input-db-$input_suffix"
input_app="fudian-input-app-$input_suffix"
input_artifact_root="/tmp/fudian-input-artifacts-$input_suffix"
input_tmp="$(mktemp -d)"

cleanup_input_stack() {
  local exit_status="$?"
  if (( exit_status != 0 )) && docker inspect "$input_app" >/dev/null 2>&1; then
    docker logs "$input_app" >&2 || true
  fi
  [[ "$input_app" == fudian-input-app-* ]] \
    && docker rm -f "$input_app" >/dev/null 2>&1 || true
  [[ "$input_db" == fudian-input-db-* ]] \
    && docker rm -f "$input_db" >/dev/null 2>&1 || true
  [[ "$input_network" == fudian-input-test-* ]] \
    && docker network rm "$input_network" >/dev/null 2>&1 || true
  [[ "$input_tmp" == /tmp/tmp.* ]] && rm -rf "$input_tmp"
  return "$exit_status"
}
trap cleanup_input_stack EXIT

new_uuid() {
  python3 -c 'import uuid; print(uuid.uuid4())'
}

json_field() {
  python3 -c 'import json,sys; value=json.loads(sys.argv[1]);
for key in sys.argv[2].split("."):
    value=value[key]
print(str(value).lower() if isinstance(value,bool) else value)' "$1" "$2"
}

post_goal_for() {
  local project_id="$1"
  local action="$2"
  local payload="$3"
  local request_id="$(new_uuid)"
  local body
  body="$(python3 -c 'import json,sys; print(json.dumps({
    "clientRequestId":sys.argv[1],"action":sys.argv[2],"payload":json.loads(sys.argv[3])
  },ensure_ascii=False))' "$request_id" "$action" "$payload")"
  curl -fsS -H 'content-type: application/json' -d "$body" \
    "$input_base/api/v1/projects/$project_id/goal-commands"
}

begin_file() {
  local request_id="$1"
  local filename="$2"
  local media_type="$3"
  local source_path="$4"
  local size
  local payload
  size="$(wc -c < "$source_path" | tr -d ' ')"
  payload="$(python3 -c 'import json,sys; print(json.dumps({
    "clientRequestId":sys.argv[1],"filename":sys.argv[2],
    "declaredMediaType":sys.argv[3],"declaredSize":int(sys.argv[4])
  },ensure_ascii=False))' "$request_id" "$filename" "$media_type" "$size")"
  curl -fsS -H 'content-type: application/json' -d "$payload" \
    "$input_session_url/inputs"
}

upload_sequential() {
  local input_id="$1"
  local source_path="$2"
  local size
  local offset=0
  local chunk_path="$input_tmp/chunk"
  local chunk_size
  local chunk_hash
  local chunk_request_id
  size="$(wc -c < "$source_path" | tr -d ' ')"
  while (( offset < size )); do
    dd if="$source_path" of="$chunk_path" bs=1 skip="$offset" count=8 status=none
    chunk_size="$(wc -c < "$chunk_path" | tr -d ' ')"
    chunk_hash="$(sha256sum "$chunk_path" | cut -d' ' -f1)"
    chunk_request_id="$(new_uuid)"
    curl -fsS -X PUT --data-binary "@$chunk_path" \
      "$input_session_url/inputs/$input_id/chunks?clientRequestId=$chunk_request_id&offset=$offset&sha256=$chunk_hash" \
      >/dev/null
    offset=$((offset + chunk_size))
  done
}

finish_file() {
  local input_id="$1"
  local source_path="$2"
  local finish_request_id="$3"
  local full_hash
  full_hash="$(sha256sum "$source_path" | cut -d' ' -f1)"
  curl -fsS -H 'content-type: application/json' \
    -d "{\"clientRequestId\":\"$finish_request_id\",\"expectedSha256\":\"$full_hash\"}" \
    "$input_session_url/inputs/$input_id/finish"
}

docker network create "$input_network" >/dev/null
docker run -d --name "$input_db" --network "$input_network" \
  --network-alias input-db \
  -e POSTGRES_USER=fudian_test \
  -e POSTGRES_PASSWORD=fudian_test_only \
  -e POSTGRES_DB=fudian_test \
  postgres:17-alpine >/dev/null

for input_attempt in $(seq 1 30); do
  if docker exec "$input_db" \
    pg_isready -h 127.0.0.1 -U fudian_test -d fudian_test >/dev/null 2>&1; then
    break
  fi
  [[ "$input_attempt" == 30 ]] && docker logs "$input_db" && exit 1
  sleep 1
done

docker run -d --name "$input_app" --network "$input_network" \
  -p 127.0.0.1::3000 \
  -e DATABASE_URL=postgres://fudian_test:fudian_test_only@input-db:5432/fudian_test \
  -e FUDIAN_BIND=0.0.0.0:3000 \
  -e ARTIFACT_ROOT="$input_artifact_root" \
  -e INPUT_MAX_BYTES=1024 \
  -e INPUT_CHUNK_MAX_BYTES=8 \
  -e INPUT_INBOX_COPY_MAX_BYTES=64 \
  -e RUST_LOG=fudian=info \
  --mount "type=bind,src=$input_repo_root,dst=/app" \
  --mount type=volume,src=fudian_rust_cargo_registry,dst=/usr/local/cargo/registry \
  --mount type=volume,src=fudian_rust_cargo_git,dst=/usr/local/cargo/git \
  --mount type=volume,src=fudian_rust_target,dst=/app/target \
  fudian-nextgen-app:latest cargo run >/dev/null

input_port="$(docker port "$input_app" 3000/tcp | sed -n 's/.*://p')"
input_base="http://127.0.0.1:$input_port"
for input_attempt in $(seq 1 60); do
  if curl -fsS "$input_base/api/health" >/dev/null 2>&1; then
    break
  fi
  [[ "$input_attempt" == 60 ]] && docker logs "$input_app" && exit 1
  sleep 1
done

input_project_response="$(curl -fsS -H 'content-type: application/json' \
  -d '{"intent":"用隔离 HTTP 验证安全文件输入"}' "$input_base/api/projects")"
input_project_id="$(json_field "$input_project_response" id)"
input_goal_revision='{
  "whyNeeded":"验证 Session 输入不会破坏目标枝干现场",
  "contract":{
    "desiredOutcome":"文本与二进制文件经过受限、可审计的导入流程",
    "hardConstraints":["不信任客户端路径、扩展名或摘要"],
    "subjectivePreferences":[],"unknowns":[],
    "nonGoals":["不自动解压归档"],
    "validationPlan":["隔离 HTTP 与数据库审计"],
    "judgmentTriggers":["拟合并后冻结输入"],
    "stopConditions":["安全拒绝路径全部通过"],
    "expectedContributions":["输入验证证据"]
  },
  "expectedContributions":[],"explorationPlan":[],"contextInheritance":{},
  "toolRequirements":[],"inferences":[],"revisionReason":null
}'
input_proposal_response="$(post_goal_for "$input_project_id" proposal.create \
  "{\"revision\":$input_goal_revision}")"
input_proposal_id="$(json_field "$input_proposal_response" result.proposalId)"
post_goal_for "$input_project_id" proposal.submit \
  "{\"proposalId\":\"$input_proposal_id\",\"expectedRevision\":1}" >/dev/null
input_approval_response="$(post_goal_for "$input_project_id" proposal.approve \
  "{\"proposalId\":\"$input_proposal_id\",\"expectedRevision\":1,\"branchName\":\"安全文件输入\",\"assignment\":\"验证输入边界\",\"agentIdentity\":\"input-worker\"}")"
input_branch_id="$(json_field "$input_approval_response" result.goalBranchId)"
input_session_id="$(json_field "$input_approval_response" result.sessionId)"
input_contract_id="$(json_field "$input_approval_response" result.contractVersionId)"
input_session_url="$input_base/api/v1/projects/$input_project_id/sessions/$input_session_id"

# 客户端文件名只作显示；两个乱序分段仍按 offset 重组并校验完整摘要。
printf 'hello rust\n' > "$input_tmp/report.txt"
input_text_begin_request="$(new_uuid)"
input_text_begin="$(begin_file "$input_text_begin_request" '../../Windows\report.txt' \
  'application/octet-stream' "$input_tmp/report.txt")"
input_text_id="$(json_field "$input_text_begin" input.id)"
python3 -c 'import json,sys
r=json.loads(sys.argv[1]); assert not r["replayed"]
assert r["input"]["originalFilename"]=="../../Windows\\report.txt"
assert r["input"]["displayName"]=="report.txt"' "$input_text_begin"
input_text_begin_replay="$(begin_file "$input_text_begin_request" '../../Windows\report.txt' \
  'application/octet-stream' "$input_tmp/report.txt")"
[[ "$(json_field "$input_text_begin_replay" replayed)" == true ]]

dd if="$input_tmp/report.txt" of="$input_tmp/text-tail" bs=1 skip=6 count=5 status=none
input_tail_hash="$(sha256sum "$input_tmp/text-tail" | cut -d' ' -f1)"
input_tail_request="$(new_uuid)"
input_tail_response="$(curl -fsS -X PUT --data-binary "@$input_tmp/text-tail" \
  "$input_session_url/inputs/$input_text_id/chunks?clientRequestId=$input_tail_request&offset=6&sha256=$input_tail_hash")"
[[ "$(json_field "$input_tail_response" receivedBytes)" == 5 ]]
input_tail_replay="$(curl -fsS -X PUT --data-binary "@$input_tmp/text-tail" \
  "$input_session_url/inputs/$input_text_id/chunks?clientRequestId=$input_tail_request&offset=6&sha256=$input_tail_hash")"
[[ "$(json_field "$input_tail_replay" replayed)" == true ]]
dd if="$input_tmp/report.txt" of="$input_tmp/text-head" bs=1 skip=0 count=6 status=none
input_head_hash="$(sha256sum "$input_tmp/text-head" | cut -d' ' -f1)"
input_head_request="$(new_uuid)"
curl -fsS -X PUT --data-binary "@$input_tmp/text-head" \
  "$input_session_url/inputs/$input_text_id/chunks?clientRequestId=$input_head_request&offset=0&sha256=$input_head_hash" \
  >/dev/null
input_text_finish_request="$(new_uuid)"
input_text_finish="$(finish_file "$input_text_id" "$input_tmp/report.txt" "$input_text_finish_request")"
input_text_hash="$(sha256sum "$input_tmp/report.txt" | cut -d' ' -f1)"
input_text_storage_key="$(json_field "$input_text_finish" input.storageKey)"
python3 -c 'import json,sys
r=json.loads(sys.argv[1]); i=r["input"]
assert i["status"]=="available" and i["sha256"]==sys.argv[2]
assert i["trustedMediaType"]=="text/plain; charset=utf-8"
assert i["verification"]["declaredMediaType"]=="application/octet-stream"' \
  "$input_text_finish" "$input_text_hash"
input_text_finish_replay="$(finish_file "$input_text_id" "$input_tmp/report.txt" "$input_text_finish_request")"
[[ "$(json_field "$input_text_finish_replay" replayed)" == true ]]

curl -fsS -D "$input_tmp/download.headers" -o "$input_tmp/download.txt" \
  "$input_session_url/inputs/$input_text_id/content"
cmp "$input_tmp/report.txt" "$input_tmp/download.txt"
grep -qi '^content-type: text/plain; charset=utf-8' "$input_tmp/download.headers"
grep -qi "^etag: \"$input_text_hash\"" "$input_tmp/download.headers"
grep -qi '^x-content-type-options: nosniff' "$input_tmp/download.headers"

input_traversal_status="$(curl -sS -o "$input_tmp/traversal.json" -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$(new_uuid)\",\"inboxRelativePath\":\"../escape.txt\"}" \
  "$input_session_url/inputs/$input_text_id/import")"
[[ "$input_traversal_status" == 422 ]]
grep -q 'unsafe_artifact_path' "$input_tmp/traversal.json"
input_text_import_request="$(new_uuid)"
input_text_import="$(curl -fsS -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$input_text_import_request\",\"inboxRelativePath\":\"notes/report.txt\"}" \
  "$input_session_url/inputs/$input_text_id/import")"
input_text_artifact_id="$(json_field "$input_text_import" input.artifactId)"
python3 -c 'import json,sys
i=json.loads(sys.argv[1])["input"]
assert i["status"]=="imported" and i["importMode"]=="worktree_copy"
assert i["inboxRelativePath"]=="notes/report.txt"' "$input_text_import"
input_text_import_replay="$(curl -fsS -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$input_text_import_request\",\"inboxRelativePath\":\"notes/report.txt\"}" \
  "$input_session_url/inputs/$input_text_id/import")"
[[ "$(json_field "$input_text_import_replay" replayed)" == true ]]
input_inbox_path="$input_artifact_root/inputs/inboxes/$input_project_id/$input_branch_id/$input_session_id/notes/report.txt"
[[ "$(docker exec "$input_app" sha256sum "$input_inbox_path" | cut -d' ' -f1)" == "$input_text_hash" ]]

# 同一内容形成独立逻辑输入，但复用内容对象与 Artifact；同名 inbox 不覆盖。
input_duplicate_begin="$(begin_file "$(new_uuid)" 'copy.txt' 'text/plain' "$input_tmp/report.txt")"
input_duplicate_id="$(json_field "$input_duplicate_begin" input.id)"
upload_sequential "$input_duplicate_id" "$input_tmp/report.txt"
input_duplicate_finish="$(finish_file "$input_duplicate_id" "$input_tmp/report.txt" "$(new_uuid)")"
[[ "$(json_field "$input_duplicate_finish" input.storageKey)" == "$input_text_storage_key" ]]
input_collision_status="$(curl -sS -o "$input_tmp/collision.json" -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$(new_uuid)\",\"inboxRelativePath\":\"notes/report.txt\"}" \
  "$input_session_url/inputs/$input_duplicate_id/import")"
[[ "$input_collision_status" == 409 ]]
grep -q 'filename_conflict' "$input_tmp/collision.json"
input_duplicate_import="$(curl -fsS -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$(new_uuid)\",\"inboxRelativePath\":\"notes/report-copy.txt\"}" \
  "$input_session_url/inputs/$input_duplicate_id/import")"
[[ "$(json_field "$input_duplicate_import" input.artifactId)" == "$input_text_artifact_id" ]]

# 归档只保存为不可执行的产物引用，绝不自动解压。
printf 'PK\003\004junk' > "$input_tmp/archive.zip"
input_archive_begin="$(begin_file "$(new_uuid)" 'evidence.zip' 'text/plain' "$input_tmp/archive.zip")"
input_archive_id="$(json_field "$input_archive_begin" input.id)"
upload_sequential "$input_archive_id" "$input_tmp/archive.zip"
input_archive_finish="$(finish_file "$input_archive_id" "$input_tmp/archive.zip" "$(new_uuid)")"
python3 -c 'import json,sys
i=json.loads(sys.argv[1])["input"]
assert i["trustedMediaType"]=="application/zip"
assert i["verification"]["archiveStoredOnly"] is True
assert i["verification"]["archiveExtracted"] is False' "$input_archive_finish"
input_archive_import="$(curl -fsS -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$(new_uuid)\",\"inboxRelativePath\":null}" \
  "$input_session_url/inputs/$input_archive_id/import")"
[[ "$(json_field "$input_archive_import" input.importMode)" == artifact_reference ]]

# 准备一个可用但未导入的输入，稍后验证候选冻结。
printf 'pending' > "$input_tmp/pending.txt"
input_pending_begin="$(begin_file "$(new_uuid)" 'pending.txt' 'text/plain' "$input_tmp/pending.txt")"
input_pending_id="$(json_field "$input_pending_begin" input.id)"
upload_sequential "$input_pending_id" "$input_tmp/pending.txt"
finish_file "$input_pending_id" "$input_tmp/pending.txt" "$(new_uuid)" >/dev/null

# 总大小、分段大小和无效 Session 都在写入前被拒绝。
input_oversize_status="$(curl -sS -o "$input_tmp/oversize.json" -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$(new_uuid)\",\"filename\":\"large.bin\",\"declaredMediaType\":null,\"declaredSize\":1025}" \
  "$input_session_url/inputs")"
[[ "$input_oversize_status" == 422 ]]
grep -q 'upload_too_large' "$input_tmp/oversize.json"
printf '123456789' > "$input_tmp/nine.bin"
input_chunk_limit_begin="$(begin_file "$(new_uuid)" 'nine.bin' 'application/octet-stream' "$input_tmp/nine.bin")"
input_chunk_limit_id="$(json_field "$input_chunk_limit_begin" input.id)"
input_chunk_limit_status="$(curl -sS -o "$input_tmp/chunk-limit.json" -w '%{http_code}' \
  -X PUT --data-binary "@$input_tmp/nine.bin" \
  "$input_session_url/inputs/$input_chunk_limit_id/chunks?clientRequestId=$(new_uuid)&offset=0")"
[[ "$input_chunk_limit_status" == 422 ]]
grep -q 'upload_chunk_too_large' "$input_tmp/chunk-limit.json"
input_missing_session_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  "$input_base/api/v1/projects/$input_project_id/sessions/$(new_uuid)/inputs")"
[[ "$input_missing_session_status" == 404 ]]

# 完整摘要不符会留下可审计的 rejected 记录，而不是可用对象。
printf 'x' > "$input_tmp/mismatch.bin"
input_mismatch_begin="$(begin_file "$(new_uuid)" 'mismatch.bin' 'application/octet-stream' "$input_tmp/mismatch.bin")"
input_mismatch_id="$(json_field "$input_mismatch_begin" input.id)"
upload_sequential "$input_mismatch_id" "$input_tmp/mismatch.bin"
input_mismatch_status="$(curl -sS -o "$input_tmp/mismatch.json" -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$(new_uuid)\",\"expectedSha256\":\"$(printf '0%.0s' {1..64})\"}" \
  "$input_session_url/inputs/$input_mismatch_id/finish")"
[[ "$input_mismatch_status" == 422 ]]
grep -q 'artifact_hash_mismatch' "$input_tmp/mismatch.json"

# 拟合并冻结后，开始、追加、完成和导入均不能再改变现场。
input_contribution_response="$(post_goal_for "$input_project_id" session.add_contribution \
  "{\"sessionId\":\"$input_session_id\",\"kind\":\"evidence\",\"title\":\"文件输入证据\",\"body\":\"隔离输入流程通过\",\"artifactId\":null,\"evidenceRefs\":[],\"supersedesId\":null}")"
input_contribution_id="$(json_field "$input_contribution_response" result.contributionId)"
post_goal_for "$input_project_id" merge.propose \
  "{\"sessionId\":\"$input_session_id\",\"candidate\":{\"contributionIds\":[\"$input_contribution_id\"],\"contractVersionId\":\"$input_contract_id\",\"gitBaseCommit\":null,\"gitHeadCommit\":null,\"gitDirty\":false,\"environmentFingerprint\":null,\"testEvidence\":[\"文件 HTTP 验收\"],\"risks\":[],\"selfCheck\":\"文件输入边界已逐条检查\"}}" \
  >/dev/null
input_frozen_begin_status="$(curl -sS -o "$input_tmp/frozen-begin.json" -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$(new_uuid)\",\"filename\":\"late.txt\",\"declaredMediaType\":\"text/plain\",\"declaredSize\":1}" \
  "$input_session_url/inputs")"
[[ "$input_frozen_begin_status" == 409 ]]
grep -q 'candidate_frozen' "$input_tmp/frozen-begin.json"
input_frozen_import_status="$(curl -sS -o "$input_tmp/frozen-import.json" -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$(new_uuid)\",\"inboxRelativePath\":\"late.txt\"}" \
  "$input_session_url/inputs/$input_pending_id/import")"
[[ "$input_frozen_import_status" == 409 ]]
grep -q 'candidate_frozen' "$input_tmp/frozen-import.json"
printf '1' > "$input_tmp/one.bin"
input_frozen_chunk_status="$(curl -sS -o "$input_tmp/frozen-chunk.json" -w '%{http_code}' \
  -X PUT --data-binary "@$input_tmp/one.bin" \
  "$input_session_url/inputs/$input_chunk_limit_id/chunks?clientRequestId=$(new_uuid)&offset=0")"
[[ "$input_frozen_chunk_status" == 409 ]]
grep -q 'candidate_frozen' "$input_tmp/frozen-chunk.json"
input_frozen_finish_status="$(curl -sS -o "$input_tmp/frozen-finish.json" -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d "{\"clientRequestId\":\"$(new_uuid)\",\"expectedSha256\":null}" \
  "$input_session_url/inputs/$input_chunk_limit_id/finish")"
[[ "$input_frozen_finish_status" == 409 ]]
grep -q 'candidate_frozen' "$input_tmp/frozen-finish.json"

input_list="$(curl -fsS "$input_session_url/inputs")"
python3 -c 'import json,sys
items={x["id"]:x for x in json.loads(sys.argv[1])["inputs"]}
assert len(items)==6
assert items[sys.argv[2]]["status"]=="rejected"
assert items[sys.argv[3]]["status"]=="available"
assert items[sys.argv[4]]["status"]=="staging"' \
  "$input_list" "$input_mismatch_id" "$input_pending_id" "$input_chunk_limit_id"

input_db_audit="$(docker exec "$input_db" psql -U fudian_test -d fudian_test -At -F '|' -c \
  "SELECT (SELECT count(*) FROM input_artifacts),
          (SELECT count(*) FROM input_artifact_chunks),
          (SELECT count(*) FROM artifacts),
          (SELECT count(*) FROM goal_events WHERE event_type = 'input.upload_started'),
          (SELECT count(*) FROM goal_events WHERE event_type = 'input.chunk_received'),
          (SELECT count(*) FROM goal_events WHERE event_type = 'input.imported'),
          (SELECT count(*) FROM goal_events WHERE event_type = 'input.rejected');")"
[[ "$input_db_audit" == '6|7|2|6|7|3|1' ]]
[[ "$(docker exec "$input_app" sh -c "find '$input_artifact_root/inputs/objects' -type f | wc -l")" == 3 ]]
docker exec "$input_app" test ! -e /tmp/escape.txt

echo "input HTTP test passed: 6 logical inputs, 7 immutable chunks, 3 content objects, frozen candidate rejected writes"
