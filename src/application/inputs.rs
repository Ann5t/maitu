use std::{io::ErrorKind, path::Path};

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction, types::Json};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncReadExt, AsyncWriteExt},
};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    goal_domain::{CommandReceiptIdentity, SessionStatus, canonical_json_sha256},
    input_artifacts::{
        BeginInputArtifact, ChunkQuery, FinishInputArtifact, ImportInputArtifact,
        InputArtifactRecord, InputChunkRecord, can_copy_to_inbox, is_archive_media_type,
        normalize_bare_sha256, normalize_declared_media_type, normalize_inbox_path,
        normalize_upload_name, safe_storage_path, sniff_media_type,
    },
};

type InputTransaction<'a> = Transaction<'a, Postgres>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputArtifactResponse {
    pub replayed: bool,
    pub input: InputArtifactRecord,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputChunkResponse {
    pub replayed: bool,
    pub input_artifact_id: Uuid,
    pub offset: u64,
    pub size: u64,
    pub sha256: String,
    pub received_bytes: u64,
    pub declared_size: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct InputLocation {
    pub project_id: Uuid,
    pub session_id: Uuid,
    pub input_id: Uuid,
}

pub async fn list_inputs(
    pool: &PgPool,
    project_id: Uuid,
    session_id: Uuid,
) -> AppResult<Vec<InputArtifactRecord>> {
    let session_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM goal_sessions WHERE id = $1 AND project_id = $2)",
    )
    .bind(session_id)
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    if !session_exists {
        return Err(AppError::not_found("Agent Session 不存在"));
    }
    Ok(sqlx::query_as::<_, InputArtifactRecord>(
        "SELECT * FROM input_artifacts WHERE project_id = $1 AND session_id = $2 \
         ORDER BY created_at, id",
    )
    .bind(project_id)
    .bind(session_id)
    .fetch_all(pool)
    .await?)
}

pub async fn begin_input(
    pool: &PgPool,
    artifact_root: &Path,
    input_max_bytes: u64,
    project_id: Uuid,
    session_id: Uuid,
    input: BeginInputArtifact,
) -> AppResult<InputArtifactResponse> {
    if input.declared_size > input_max_bytes || input.declared_size > i64::MAX as u64 {
        return Err(AppError::bad_request(
            "upload_too_large",
            format!("单个输入文件不能超过 {input_max_bytes} 字节"),
        ));
    }
    let (original_filename, display_name) = normalize_upload_name(input.filename)?;
    let declared_media_type = normalize_declared_media_type(input.declared_media_type)?;
    let request_hash = canonical_json_sha256(&json!({
        "sessionId": session_id,
        "filename": original_filename,
        "declaredMediaType": declared_media_type,
        "declaredSize": input.declared_size,
    }))?;
    let mut transaction = pool.begin().await?;
    lock_project(&mut transaction, project_id).await?;
    if let Some(existing) = sqlx::query_as::<_, InputArtifactRecord>(
        "SELECT * FROM input_artifacts WHERE project_id = $1 AND client_request_id = $2",
    )
    .bind(project_id)
    .bind(input.client_request_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        let existing_hash = canonical_json_sha256(&json!({
            "sessionId": existing.session_id,
            "filename": existing.original_filename,
            "declaredMediaType": existing.declared_media_type,
            "declaredSize": existing.declared_size,
        }))?;
        CommandReceiptIdentity {
            command_kind: "input.begin".into(),
            input_hash: existing_hash,
        }
        .ensure_replay_matches(&CommandReceiptIdentity {
            command_kind: "input.begin".into(),
            input_hash: request_hash,
        })?;
        transaction.commit().await?;
        return Ok(InputArtifactResponse {
            replayed: true,
            input: existing,
        });
    }
    let (goal_branch_id, status) =
        load_session_scope(&mut transaction, project_id, session_id).await?;
    if SessionStatus::try_from(status.as_str())? != SessionStatus::Running {
        return Err(AppError::conflict(
            "candidate_frozen",
            "只有 running Session 可以接收新输入；请把文件交给后续 Session",
        ));
    }

    let input_id = Uuid::new_v4();
    let storage_key = format!("inputs/staging/{input_id}/complete");
    let storage_path = safe_storage_path(artifact_root, &storage_key)?;
    if let Some(parent) = storage_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let record = sqlx::query_as::<_, InputArtifactRecord>(
        "INSERT INTO input_artifacts \
         (id, project_id, goal_branch_id, session_id, client_request_id, status, \
          original_filename, display_name, declared_media_type, declared_size, storage_key) \
         VALUES ($1, $2, $3, $4, $5, 'staging', $6, $7, $8, $9, $10) \
         RETURNING *",
    )
    .bind(input_id)
    .bind(project_id)
    .bind(goal_branch_id)
    .bind(session_id)
    .bind(input.client_request_id)
    .bind(original_filename)
    .bind(display_name)
    .bind(declared_media_type)
    .bind(input.declared_size as i64)
    .bind(storage_key)
    .fetch_one(&mut *transaction)
    .await?;
    insert_input_event(
        &mut transaction,
        project_id,
        input_id,
        "input.upload_started",
        input.client_request_id,
        json!({
            "goalBranchId": goal_branch_id,
            "sessionId": session_id,
            "declaredSize": input.declared_size,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(InputArtifactResponse {
        replayed: false,
        input: record,
    })
}

pub async fn append_chunk(
    pool: &PgPool,
    artifact_root: &Path,
    chunk_max_bytes: usize,
    location: InputLocation,
    query: ChunkQuery,
    bytes: Vec<u8>,
) -> AppResult<InputChunkResponse> {
    let InputLocation {
        project_id,
        session_id,
        input_id,
    } = location;
    if bytes.len() > chunk_max_bytes {
        return Err(AppError::bad_request(
            "upload_chunk_too_large",
            format!("单个分段不能超过 {chunk_max_bytes} 字节"),
        ));
    }
    let chunk_sha256 = hex::encode(Sha256::digest(&bytes));
    if let Some(expected) = query.sha256 {
        let expected = normalize_bare_sha256(expected)?;
        if expected != chunk_sha256 {
            return Err(AppError::bad_request(
                "artifact_hash_mismatch",
                "分段 SHA-256 与请求声明不一致",
            ));
        }
    }
    if query.offset > i64::MAX as u64 || bytes.len() > i64::MAX as usize {
        return Err(AppError::bad_request(
            "invalid_upload_offset",
            "分段偏移超出支持范围",
        ));
    }
    let mut transaction = pool.begin().await?;
    lock_project(&mut transaction, project_id).await?;
    let input = load_input_for_update(&mut transaction, project_id, session_id, input_id).await?;
    if let Some(existing) = sqlx::query_as::<_, InputChunkRecord>(
        "SELECT * FROM input_artifact_chunks \
         WHERE input_artifact_id = $1 AND client_request_id = $2",
    )
    .bind(input_id)
    .bind(query.client_request_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        if existing.offset_bytes != query.offset as i64
            || existing.size_bytes != bytes.len() as i64
            || existing.sha256 != chunk_sha256
        {
            return Err(AppError::conflict(
                "idempotency_conflict",
                "同一分段请求 ID 已用于不同内容、大小或偏移",
            ));
        }
        transaction.commit().await?;
        return Ok(InputChunkResponse {
            replayed: true,
            input_artifact_id: input_id,
            offset: query.offset,
            size: bytes.len() as u64,
            sha256: chunk_sha256,
            received_bytes: input.actual_size as u64,
            declared_size: input.declared_size as u64,
        });
    }
    ensure_running_session(
        &mut transaction,
        project_id,
        session_id,
        input.goal_branch_id,
    )
    .await?;
    if input.status != "staging" {
        return Err(AppError::conflict(
            "invalid_state_transition",
            "只有 staging InputArtifact 可以接收分段",
        ));
    }
    let end = query
        .offset
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| AppError::bad_request("invalid_upload_offset", "分段范围溢出"))?;
    if end > input.declared_size as u64 {
        return Err(AppError::bad_request(
            "upload_too_large",
            "分段超出上传开始时声明的总大小",
        ));
    }
    let overlaps: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM input_artifact_chunks \
         WHERE input_artifact_id = $1 \
         AND offset_bytes < $2 AND offset_bytes + size_bytes > $3)",
    )
    .bind(input_id)
    .bind(end as i64)
    .bind(query.offset as i64)
    .fetch_one(&mut *transaction)
    .await?;
    if overlaps {
        return Err(AppError::conflict(
            "invalid_upload_offset",
            "分段范围与已经接收的内容重叠",
        ));
    }

    let storage_key = format!(
        "inputs/chunks/{input_id}/{:020}-{chunk_sha256}",
        query.offset
    );
    let storage_path = safe_storage_path(artifact_root, &storage_key)?;
    if let Some(parent) = storage_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&storage_path)
        .await
    {
        Ok(mut file) => {
            file.write_all(&bytes).await?;
            file.flush().await?;
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let existing = fs::read(&storage_path).await?;
            if hex::encode(Sha256::digest(existing)) != chunk_sha256 {
                return Err(AppError::conflict(
                    "artifact_hash_mismatch",
                    "内容寻址的分段文件与摘要不一致",
                ));
            }
        }
        Err(error) => return Err(error.into()),
    }
    sqlx::query(
        "INSERT INTO input_artifact_chunks \
         (input_artifact_id, offset_bytes, size_bytes, sha256, storage_key, client_request_id) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(input_id)
    .bind(query.offset as i64)
    .bind(bytes.len() as i64)
    .bind(&chunk_sha256)
    .bind(storage_key)
    .bind(query.client_request_id)
    .execute(&mut *transaction)
    .await?;
    let received_bytes: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(size_bytes), 0)::bigint FROM input_artifact_chunks \
         WHERE input_artifact_id = $1",
    )
    .bind(input_id)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query("UPDATE input_artifacts SET actual_size = $1 WHERE id = $2")
        .bind(received_bytes)
        .bind(input_id)
        .execute(&mut *transaction)
        .await?;
    insert_input_event(
        &mut transaction,
        project_id,
        input_id,
        "input.chunk_received",
        query.client_request_id,
        json!({
            "offset": query.offset,
            "size": bytes.len(),
            "sha256": chunk_sha256,
            "receivedBytes": received_bytes,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(InputChunkResponse {
        replayed: false,
        input_artifact_id: input_id,
        offset: query.offset,
        size: bytes.len() as u64,
        sha256: chunk_sha256,
        received_bytes: received_bytes as u64,
        declared_size: input.declared_size as u64,
    })
}

pub async fn finish_input(
    pool: &PgPool,
    artifact_root: &Path,
    project_id: Uuid,
    session_id: Uuid,
    input_id: Uuid,
    input: FinishInputArtifact,
) -> AppResult<InputArtifactResponse> {
    let expected_sha256 = input
        .expected_sha256
        .map(normalize_bare_sha256)
        .transpose()?;
    let request_hash = canonical_json_sha256(&json!({
        "inputArtifactId": input_id,
        "expectedSha256": expected_sha256,
    }))?;
    let mut transaction = pool.begin().await?;
    lock_project(&mut transaction, project_id).await?;
    let current = load_input_for_update(&mut transaction, project_id, session_id, input_id).await?;
    if let Some(existing_request_id) = current.finish_client_request_id {
        CommandReceiptIdentity {
            command_kind: "input.finish".into(),
            input_hash: current
                .finish_request_hash
                .clone()
                .ok_or_else(|| AppError::internal("InputArtifact 完成请求缺少输入摘要"))?,
        }
        .ensure_replay_matches(&CommandReceiptIdentity {
            command_kind: "input.finish".into(),
            input_hash: request_hash,
        })?;
        if existing_request_id != input.client_request_id {
            return Err(AppError::conflict(
                "idempotency_conflict",
                "InputArtifact 已由另一个完成请求冻结",
            ));
        }
        transaction.commit().await?;
        return Ok(InputArtifactResponse {
            replayed: true,
            input: current,
        });
    }
    ensure_running_session(
        &mut transaction,
        project_id,
        session_id,
        current.goal_branch_id,
    )
    .await?;
    if current.status != "staging" {
        return Err(AppError::conflict(
            "invalid_state_transition",
            "只有 staging InputArtifact 可以完成上传",
        ));
    }
    let chunks = sqlx::query_as::<_, InputChunkRecord>(
        "SELECT * FROM input_artifact_chunks WHERE input_artifact_id = $1 \
         ORDER BY offset_bytes",
    )
    .bind(input_id)
    .fetch_all(&mut *transaction)
    .await?;
    let mut expected_offset = 0_i64;
    for chunk in &chunks {
        if chunk.offset_bytes != expected_offset {
            return Err(AppError::conflict(
                "incomplete_upload",
                "上传分段不连续，仍有缺口",
            ));
        }
        expected_offset = expected_offset
            .checked_add(chunk.size_bytes)
            .ok_or_else(|| AppError::bad_request("upload_too_large", "上传分段总大小溢出"))?;
    }
    if expected_offset != current.declared_size {
        return Err(AppError::conflict(
            "incomplete_upload",
            "已经接收的分段总大小与声明大小不一致",
        ));
    }

    let staging_path = safe_storage_path(artifact_root, &current.storage_key)?;
    if let Some(parent) = staging_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut output = File::create(&staging_path).await?;
    let mut full_hasher = Sha256::new();
    let mut prefix = Vec::with_capacity(8_192);
    let mut copied = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    for chunk in &chunks {
        let chunk_path = safe_storage_path(artifact_root, &chunk.storage_key)?;
        let mut chunk_file = File::open(chunk_path).await?;
        let mut chunk_hasher = Sha256::new();
        let mut chunk_size = 0_u64;
        loop {
            let read = chunk_file.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            let data = &buffer[..read];
            chunk_hasher.update(data);
            full_hasher.update(data);
            output.write_all(data).await?;
            if prefix.len() < 8_192 {
                let take = (8_192 - prefix.len()).min(read);
                prefix.extend_from_slice(&data[..take]);
            }
            chunk_size += read as u64;
            copied += read as u64;
        }
        if chunk_size != chunk.size_bytes as u64
            || hex::encode(chunk_hasher.finalize()) != chunk.sha256
        {
            return Err(AppError::conflict(
                "artifact_hash_mismatch",
                "持久化分段内容与登记摘要不一致",
            ));
        }
    }
    output.flush().await?;
    if copied != current.declared_size as u64 {
        return Err(AppError::conflict(
            "artifact_hash_mismatch",
            "合并后的实际大小与声明大小不一致",
        ));
    }
    let sha256 = hex::encode(full_hasher.finalize());
    if let Some(expected) = &expected_sha256
        && expected != &sha256
    {
        let verification = json!({
            "result": "rejected",
            "reason": "expected_sha256_mismatch",
            "observedSha256": sha256,
        });
        sqlx::query(
            "UPDATE input_artifacts SET status = 'rejected', actual_size = $1, \
             verification = $2, finish_client_request_id = $3, finish_request_hash = $4, \
             verified_at = now() WHERE id = $5",
        )
        .bind(copied as i64)
        .bind(Json(verification))
        .bind(input.client_request_id)
        .bind(&request_hash)
        .bind(input_id)
        .execute(&mut *transaction)
        .await?;
        insert_input_event(
            &mut transaction,
            project_id,
            input_id,
            "input.rejected",
            input.client_request_id,
            json!({ "reason": "expected_sha256_mismatch" }),
        )
        .await?;
        transaction.commit().await?;
        return Err(AppError::bad_request(
            "artifact_hash_mismatch",
            "完整文件 SHA-256 与请求声明不一致",
        ));
    }
    let trusted_media_type = sniff_media_type(&prefix).to_owned();
    let object_key = format!("inputs/objects/{}/{}", &sha256[..2], sha256);
    let object_path = safe_storage_path(artifact_root, &object_key)?;
    if let Some(parent) = object_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    match fs::metadata(&object_path).await {
        Ok(metadata) => {
            if metadata.len() != copied || hash_file(&object_path).await? != sha256 {
                return Err(AppError::conflict(
                    "artifact_hash_mismatch",
                    "内容寻址仓库已有对象与摘要不一致",
                ));
            }
            fs::remove_file(&staging_path).await?;
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            fs::rename(&staging_path, &object_path).await?;
        }
        Err(error) => return Err(error.into()),
    }
    let verification = json!({
        "result": "verified",
        "declaredMediaType": current.declared_media_type,
        "trustedMediaType": trusted_media_type,
        "archiveStoredOnly": is_archive_media_type(&trusted_media_type),
        "archiveExtracted": false,
    });
    sqlx::query(
        "UPDATE input_artifacts SET status = 'available', actual_size = $1, sha256 = $2, \
         trusted_media_type = $3, storage_key = $4, verification = $5, \
         finish_client_request_id = $6, finish_request_hash = $7, verified_at = now(), \
         available_at = now() WHERE id = $8",
    )
    .bind(copied as i64)
    .bind(&sha256)
    .bind(&trusted_media_type)
    .bind(&object_key)
    .bind(Json(verification))
    .bind(input.client_request_id)
    .bind(request_hash)
    .bind(input_id)
    .execute(&mut *transaction)
    .await?;
    insert_input_event(
        &mut transaction,
        project_id,
        input_id,
        "input.verified",
        input.client_request_id,
        json!({
            "sha256": sha256,
            "actualSize": copied,
            "trustedMediaType": trusted_media_type,
        }),
    )
    .await?;
    insert_input_event(
        &mut transaction,
        project_id,
        input_id,
        "input.available",
        input.client_request_id,
        json!({ "storageKey": object_key }),
    )
    .await?;
    let record = load_input_for_update(&mut transaction, project_id, session_id, input_id).await?;
    transaction.commit().await?;
    Ok(InputArtifactResponse {
        replayed: false,
        input: record,
    })
}

pub async fn import_input(
    pool: &PgPool,
    artifact_root: &Path,
    inbox_copy_max_bytes: u64,
    project_id: Uuid,
    session_id: Uuid,
    input_id: Uuid,
    input: ImportInputArtifact,
) -> AppResult<InputArtifactResponse> {
    let normalized_requested_path = input
        .inbox_relative_path
        .map(normalize_inbox_path)
        .transpose()?;
    let request_hash = canonical_json_sha256(&json!({
        "inputArtifactId": input_id,
        "inboxRelativePath": normalized_requested_path,
    }))?;
    let mut transaction = pool.begin().await?;
    lock_project(&mut transaction, project_id).await?;
    let current = load_input_for_update(&mut transaction, project_id, session_id, input_id).await?;
    if let Some(existing_request_id) = current.import_client_request_id {
        CommandReceiptIdentity {
            command_kind: "input.import".into(),
            input_hash: current
                .import_request_hash
                .clone()
                .ok_or_else(|| AppError::internal("InputArtifact 导入请求缺少输入摘要"))?,
        }
        .ensure_replay_matches(&CommandReceiptIdentity {
            command_kind: "input.import".into(),
            input_hash: request_hash,
        })?;
        if existing_request_id != input.client_request_id {
            return Err(AppError::conflict(
                "idempotency_conflict",
                "InputArtifact 已由另一个导入请求处理",
            ));
        }
        transaction.commit().await?;
        return Ok(InputArtifactResponse {
            replayed: true,
            input: current,
        });
    }
    if current.status != "available" {
        return Err(AppError::conflict(
            "invalid_state_transition",
            "只有 available InputArtifact 可以导入",
        ));
    }
    let (goal_branch_id, session_status) =
        load_session_scope(&mut transaction, project_id, session_id).await?;
    if goal_branch_id != current.goal_branch_id {
        return Err(AppError::bad_request(
            "cross_project_reference",
            "InputArtifact 不属于该目标枝干",
        ));
    }
    if SessionStatus::try_from(session_status.as_str())? != SessionStatus::Running {
        return Err(AppError::conflict(
            "candidate_frozen",
            "冻结或结束的 Session 不能导入新文件；请创建后续 Session",
        ));
    }
    let sha256 = current
        .sha256
        .clone()
        .ok_or_else(|| AppError::internal("可用 InputArtifact 缺少 SHA-256"))?;
    let trusted_media_type = current
        .trusted_media_type
        .clone()
        .ok_or_else(|| AppError::internal("可用 InputArtifact 缺少可信 media type"))?;
    let object_path = safe_storage_path(artifact_root, &current.storage_key)?;
    if hash_file(&object_path).await? != sha256 {
        return Err(AppError::conflict(
            "artifact_hash_mismatch",
            "内容仓库对象与 InputArtifact SHA-256 不一致",
        ));
    }

    let artifact_id = if let Some(existing) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM artifacts WHERE project_id = $1 AND storage_path = $2 AND version = 1",
    )
    .bind(project_id)
    .bind(&current.storage_key)
    .fetch_optional(&mut *transaction)
    .await?
    {
        existing
    } else {
        let artifact_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO artifacts \
             (id, project_id, title, kind, storage_path, media_type, sha256, version, status, \
              approved_at) \
             VALUES ($1, $2, $3, 'input', $4, $5, $6, 1, 'approved', now())",
        )
        .bind(artifact_id)
        .bind(project_id)
        .bind(&current.display_name)
        .bind(&current.storage_key)
        .bind(&trusted_media_type)
        .bind(&sha256)
        .execute(&mut *transaction)
        .await?;
        artifact_id
    };

    let copy_to_inbox = can_copy_to_inbox(
        &trusted_media_type,
        current.actual_size as u64,
        inbox_copy_max_bytes,
    ) && file_is_safe_utf8(&object_path).await?;
    let (import_mode, inbox_relative_path) = if copy_to_inbox {
        let default_path = format!(
            "inputs/{input_id}/{}",
            current.display_name.replace(['/', '\\'], "_")
        );
        let relative_path = normalized_requested_path.unwrap_or_else(|| {
            normalize_inbox_path(default_path)
                .unwrap_or_else(|_| format!("inputs/{input_id}/input.txt"))
        });
        let inbox_key =
            format!("inputs/inboxes/{project_id}/{goal_branch_id}/{session_id}/{relative_path}");
        let inbox_path = safe_storage_path(artifact_root, &inbox_key)?;
        if let Some(parent) = inbox_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let mut destination = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&inbox_path)
            .await
            .map_err(|error| {
                if error.kind() == ErrorKind::AlreadyExists {
                    AppError::conflict(
                        "filename_conflict",
                        "Session inbox 已存在同名文件，不会静默覆盖",
                    )
                } else {
                    error.into()
                }
            })?;
        let mut source = File::open(&object_path).await?;
        tokio::io::copy(&mut source, &mut destination).await?;
        destination.flush().await?;
        ("worktree_copy", Some(relative_path))
    } else {
        ("artifact_reference", None)
    };
    sqlx::query(
        "UPDATE input_artifacts SET status = 'imported', import_mode = $1, \
         inbox_relative_path = $2, artifact_id = $3, import_client_request_id = $4, \
         import_request_hash = $5, imported_at = now() WHERE id = $6",
    )
    .bind(import_mode)
    .bind(&inbox_relative_path)
    .bind(artifact_id)
    .bind(input.client_request_id)
    .bind(request_hash)
    .bind(input_id)
    .execute(&mut *transaction)
    .await?;
    insert_input_event(
        &mut transaction,
        project_id,
        input_id,
        "input.imported",
        input.client_request_id,
        json!({
            "sessionId": session_id,
            "goalBranchId": goal_branch_id,
            "artifactId": artifact_id,
            "importMode": import_mode,
            "inboxRelativePath": inbox_relative_path,
        }),
    )
    .await?;
    let record = load_input_for_update(&mut transaction, project_id, session_id, input_id).await?;
    transaction.commit().await?;
    Ok(InputArtifactResponse {
        replayed: false,
        input: record,
    })
}

pub async fn read_input(
    pool: &PgPool,
    artifact_root: &Path,
    project_id: Uuid,
    session_id: Uuid,
    input_id: Uuid,
) -> AppResult<(InputArtifactRecord, Vec<u8>)> {
    let record = sqlx::query_as::<_, InputArtifactRecord>(
        "SELECT * FROM input_artifacts \
         WHERE id = $1 AND project_id = $2 AND session_id = $3",
    )
    .bind(input_id)
    .bind(project_id)
    .bind(session_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("InputArtifact 不存在"))?;
    if !matches!(record.status.as_str(), "available" | "imported") {
        return Err(AppError::conflict(
            "input_not_available",
            "InputArtifact 尚不可下载",
        ));
    }
    let path = safe_storage_path(artifact_root, &record.storage_key)?;
    let bytes = fs::read(path).await?;
    let observed = hex::encode(Sha256::digest(&bytes));
    if record.sha256.as_deref() != Some(observed.as_str()) {
        return Err(AppError::conflict(
            "artifact_hash_mismatch",
            "下载内容与 InputArtifact 摘要不一致",
        ));
    }
    Ok((record, bytes))
}

async fn lock_project(transaction: &mut InputTransaction<'_>, project_id: Uuid) -> AppResult<()> {
    let exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
            .bind(project_id)
            .fetch_optional(&mut **transaction)
            .await?;
    if exists.is_none() {
        return Err(AppError::not_found("项目不存在"));
    }
    Ok(())
}

async fn load_session_scope(
    transaction: &mut InputTransaction<'_>,
    project_id: Uuid,
    session_id: Uuid,
) -> AppResult<(Uuid, String)> {
    sqlx::query_as(
        "SELECT goal_branch_id, status FROM goal_sessions \
         WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(session_id)
    .bind(project_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::not_found("Agent Session 不存在"))
}

async fn ensure_running_session(
    transaction: &mut InputTransaction<'_>,
    project_id: Uuid,
    session_id: Uuid,
    expected_goal_branch_id: Uuid,
) -> AppResult<()> {
    let (goal_branch_id, status) = load_session_scope(transaction, project_id, session_id).await?;
    if goal_branch_id != expected_goal_branch_id {
        return Err(AppError::bad_request(
            "cross_project_reference",
            "InputArtifact 不属于该目标枝干",
        ));
    }
    if SessionStatus::try_from(status.as_str())? != SessionStatus::Running {
        return Err(AppError::conflict(
            "candidate_frozen",
            "冻结或结束的 Session 不能改变文件输入；请创建后续 Session",
        ));
    }
    Ok(())
}

async fn load_input_for_update(
    transaction: &mut InputTransaction<'_>,
    project_id: Uuid,
    session_id: Uuid,
    input_id: Uuid,
) -> AppResult<InputArtifactRecord> {
    sqlx::query_as::<_, InputArtifactRecord>(
        "SELECT * FROM input_artifacts \
         WHERE id = $1 AND project_id = $2 AND session_id = $3 FOR UPDATE",
    )
    .bind(input_id)
    .bind(project_id)
    .bind(session_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::not_found("InputArtifact 不存在"))
}

async fn insert_input_event(
    transaction: &mut InputTransaction<'_>,
    project_id: Uuid,
    input_id: Uuid,
    event_type: &str,
    client_request_id: Uuid,
    payload: Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO goal_events \
         (id, project_id, aggregate_type, aggregate_id, event_type, actor_type, \
          client_request_id, payload) \
         VALUES ($1, $2, 'input_artifact', $3, $4, 'system', $5, $6)",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(input_id)
    .bind(event_type)
    .bind(client_request_id)
    .bind(Json(payload))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn hash_file(path: &Path) -> AppResult<String> {
    let mut file = File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

async fn file_is_safe_utf8(path: &Path) -> AppResult<bool> {
    let bytes = fs::read(path).await?;
    Ok(std::str::from_utf8(&bytes).is_ok() && !bytes.contains(&0))
}
