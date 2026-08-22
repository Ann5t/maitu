use std::{io::ErrorKind, path::Path};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, Postgres, Transaction, types::Json};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
};
use uuid::Uuid;

use crate::{
    application::projects,
    domain::ProjectIntake,
    error::{AppError, AppResult},
    idea_domain::{
        AttachIdeaSourceQuery, IdeaCommandRequest, IdeaRevisionDraft, ProjectProposalRevisionDraft,
        ProjectProposalStatus, clean_required, validate_idea_relation,
    },
    idea_models::{
        IdeaEventRecord, IdeaLinkView, IdeaRecord, IdeaRevisionRecord, IdeaRevisionSourceRecord,
        IdeaSnapshot, IdeaSourceRecord, IdeaSummary, ProjectProposalIdeaRecord,
        ProjectProposalRecord, ProjectProposalRevisionRecord,
    },
    input_artifacts::{
        normalize_bare_sha256, normalize_declared_media_type, normalize_upload_name,
        safe_storage_path, sniff_media_type,
    },
};

type IdeaTransaction<'a> = Transaction<'a, Postgres>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaCommandResponse {
    pub replayed: bool,
    pub result: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaSourceResponse {
    pub replayed: bool,
    pub idea_revision: i32,
    pub source: IdeaSourceRecord,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateIdeaInput {
    revision: IdeaRevisionDraft,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviseIdeaInput {
    expected_revision: i32,
    revision: IdeaRevisionDraft,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinkIdeaInput {
    expected_source_revision: i32,
    target_idea_id: Uuid,
    expected_target_revision: i32,
    relation: String,
    rationale: String,
}

#[derive(Debug, Deserialize)]
struct ArchiveIdeaInput {
    reason: String,
}

#[derive(Debug, Deserialize)]
struct CreateProjectProposalInput {
    revision: ProjectProposalRevisionDraft,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviseProjectProposalInput {
    expected_revision: i32,
    revision: ProjectProposalRevisionDraft,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProposalVersionInput {
    expected_revision: i32,
}

#[derive(Debug, Deserialize)]
struct ProposalDecisionInput {
    rationale: String,
}

#[derive(Clone, Debug, FromRow)]
struct IdeaStateRow {
    id: Uuid,
    state: String,
    current_revision: i32,
}

#[derive(Clone, Debug, FromRow)]
struct ProposalStateRow {
    status: String,
    current_revision: i32,
}

#[derive(Clone, Debug, FromRow)]
struct ProposalDraftRow {
    title: String,
    project_intent: String,
    why_now: String,
    root_goal: Json<Value>,
    retained_notes: Json<Vec<String>>,
    omitted_notes: Json<Vec<String>>,
    revision_reason: Option<String>,
}

pub async fn list_ideas(pool: &PgPool) -> AppResult<Vec<IdeaSummary>> {
    Ok(sqlx::query_as::<_, IdeaSummary>(
        "SELECT i.id, i.state, i.current_revision, r.title, r.body, r.source_kind, \
         r.source_ref, i.updated_at, \
         (SELECT count(*) FROM idea_links l \
          WHERE l.source_idea_id = i.id OR l.target_idea_id = i.id) AS link_count, \
         (SELECT count(DISTINCT pri.proposal_id) FROM project_proposal_revision_ideas pri \
          WHERE pri.idea_id = i.id) AS proposal_count, \
         (SELECT poi.project_id FROM project_origin_ideas poi \
          WHERE poi.idea_id = i.id ORDER BY poi.created_at DESC LIMIT 1) AS promoted_project_id \
         FROM ideas i \
         JOIN idea_revisions r ON r.idea_id = i.id AND r.revision = i.current_revision \
         ORDER BY i.updated_at DESC, i.id",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn list_links(pool: &PgPool) -> AppResult<Vec<IdeaLinkView>> {
    Ok(sqlx::query_as::<_, IdeaLinkView>(
        "SELECT l.*, sr.title AS source_title, tr.title AS target_title \
         FROM idea_links l \
         JOIN idea_revisions sr ON sr.idea_id = l.source_idea_id \
          AND sr.revision = l.source_revision \
         JOIN idea_revisions tr ON tr.idea_id = l.target_idea_id \
          AND tr.revision = l.target_revision \
         ORDER BY l.created_at DESC, l.id",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn get_snapshot(pool: &PgPool, idea_id: Uuid) -> AppResult<IdeaSnapshot> {
    let idea = sqlx::query_as::<_, IdeaRecord>("SELECT * FROM ideas WHERE id = $1")
        .bind(idea_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("想法不存在"))?;
    let revisions = sqlx::query_as::<_, IdeaRevisionRecord>(
        "SELECT * FROM idea_revisions WHERE idea_id = $1 ORDER BY revision DESC",
    )
    .bind(idea_id)
    .fetch_all(pool)
    .await?;
    let sources = sqlx::query_as::<_, IdeaSourceRecord>(
        "SELECT * FROM idea_source_objects WHERE idea_id = $1 ORDER BY created_at DESC, id",
    )
    .bind(idea_id)
    .fetch_all(pool)
    .await?;
    let revision_sources = sqlx::query_as::<_, IdeaRevisionSourceRecord>(
        "SELECT * FROM idea_revision_sources WHERE idea_id = $1 \
         ORDER BY idea_revision DESC, created_at, source_id",
    )
    .bind(idea_id)
    .fetch_all(pool)
    .await?;
    let links = sqlx::query_as::<_, IdeaLinkView>(
        "SELECT l.*, sr.title AS source_title, tr.title AS target_title \
         FROM idea_links l \
         JOIN idea_revisions sr ON sr.idea_id = l.source_idea_id \
          AND sr.revision = l.source_revision \
         JOIN idea_revisions tr ON tr.idea_id = l.target_idea_id \
          AND tr.revision = l.target_revision \
         WHERE l.source_idea_id = $1 OR l.target_idea_id = $1 \
         ORDER BY l.created_at DESC, l.id",
    )
    .bind(idea_id)
    .fetch_all(pool)
    .await?;
    let proposals = sqlx::query_as::<_, ProjectProposalRecord>(
        "SELECT p.* FROM project_proposals p WHERE EXISTS ( \
           SELECT 1 FROM project_proposal_revision_ideas pri \
           WHERE pri.proposal_id = p.id AND pri.idea_id = $1 \
         ) ORDER BY p.updated_at DESC, p.id",
    )
    .bind(idea_id)
    .fetch_all(pool)
    .await?;
    let proposal_ids = proposals.iter().map(|item| item.id).collect::<Vec<_>>();
    let proposal_revisions = if proposal_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, ProjectProposalRevisionRecord>(
            "SELECT * FROM project_proposal_revisions \
             WHERE proposal_id = ANY($1) ORDER BY created_at DESC, revision DESC",
        )
        .bind(&proposal_ids)
        .fetch_all(pool)
        .await?
    };
    let proposal_sources = if proposal_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, ProjectProposalIdeaRecord>(
            "SELECT * FROM project_proposal_revision_ideas \
             WHERE proposal_id = ANY($1) ORDER BY proposal_revision DESC, created_at, idea_id",
        )
        .bind(&proposal_ids)
        .fetch_all(pool)
        .await?
    };
    let events = sqlx::query_as::<_, IdeaEventRecord>(
        "SELECT * FROM idea_events WHERE \
         (aggregate_type = 'idea' AND aggregate_id = $1) OR \
         (aggregate_type = 'project_proposal' AND aggregate_id = ANY($2)) \
         ORDER BY sequence DESC LIMIT 100",
    )
    .bind(idea_id)
    .bind(&proposal_ids)
    .fetch_all(pool)
    .await?;

    Ok(IdeaSnapshot {
        model_version: "idea-project/v1",
        idea,
        revisions,
        sources,
        revision_sources,
        links,
        proposals,
        proposal_revisions,
        proposal_sources,
        events,
    })
}

pub async fn attach_source(
    pool: &PgPool,
    artifact_root: &Path,
    input_max_bytes: u64,
    idea_id: Uuid,
    query: AttachIdeaSourceQuery,
    bytes: Vec<u8>,
) -> AppResult<IdeaSourceResponse> {
    if bytes.is_empty() {
        return Err(AppError::bad_request(
            "empty_idea_source",
            "想法来源文件不能为空",
        ));
    }
    if bytes.len() as u64 > input_max_bytes || bytes.len() > i64::MAX as usize {
        return Err(AppError::bad_request(
            "upload_too_large",
            format!("单个想法来源不能超过 {input_max_bytes} 字节"),
        ));
    }
    let (original_filename, display_name) = normalize_upload_name(query.filename)?;
    let declared_media_type = normalize_declared_media_type(query.declared_media_type)?;
    let sha256 = hex::encode(Sha256::digest(&bytes));
    if let Some(expected) = query
        .expected_sha256
        .map(normalize_bare_sha256)
        .transpose()?
        && expected != sha256
    {
        return Err(AppError::bad_request(
            "artifact_hash_mismatch",
            "想法来源的 SHA-256 与请求声明不一致",
        ));
    }
    let trusted_media_type = sniff_media_type(&bytes[..bytes.len().min(8_192)]).to_owned();
    let kind = if trusted_media_type.starts_with("image/") {
        "image"
    } else if trusted_media_type.starts_with("audio/") {
        "audio"
    } else {
        "file"
    };
    let note = query.note.unwrap_or_default().trim().to_owned();
    if note.chars().count() > 4_000 {
        return Err(AppError::bad_request(
            "text_too_long",
            "来源说明不能超过 4000 个字符",
        ));
    }
    let request_hash = crate::goal_domain::canonical_json_sha256(&json!({
        "ideaId": idea_id,
        "expectedRevision": query.expected_revision,
        "filename": original_filename,
        "declaredMediaType": declared_media_type,
        "sha256": sha256,
        "note": note,
    }))?;
    let storage_key = format!("objects/sha256/{}/{}", &sha256[..2], sha256);
    write_content_object(artifact_root, &storage_key, &sha256, &bytes).await?;

    let mut transaction = pool.begin().await?;
    let idea = lock_idea(&mut transaction, idea_id).await?;
    if let Some(existing) = sqlx::query_as::<_, IdeaSourceRecord>(
        "SELECT * FROM idea_source_objects WHERE idea_id = $1 AND client_request_id = $2",
    )
    .bind(idea_id)
    .bind(query.client_request_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        crate::goal_domain::CommandReceiptIdentity {
            command_kind: "idea.source.attach".into(),
            input_hash: existing.request_hash.clone(),
        }
        .ensure_replay_matches(&crate::goal_domain::CommandReceiptIdentity {
            command_kind: "idea.source.attach".into(),
            input_hash: request_hash,
        })?;
        let attached_revision: i32 = sqlx::query_scalar(
            "SELECT min(idea_revision) FROM idea_revision_sources \
             WHERE idea_id = $1 AND source_id = $2",
        )
        .bind(idea_id)
        .bind(existing.id)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        return Ok(IdeaSourceResponse {
            replayed: true,
            idea_revision: attached_revision,
            source: existing,
        });
    }
    if idea.state == "archived" {
        return Err(AppError::conflict(
            "idea_archived",
            "已归档想法不能再附加来源",
        ));
    }
    ensure_revision(idea.current_revision, query.expected_revision)?;
    let current = sqlx::query_as::<_, IdeaRevisionRecord>(
        "SELECT * FROM idea_revisions WHERE idea_id = $1 AND revision = $2",
    )
    .bind(idea_id)
    .bind(idea.current_revision)
    .fetch_one(&mut *transaction)
    .await?;
    let source_id = Uuid::new_v4();
    let next = idea.current_revision + 1;
    let source = sqlx::query_as::<_, IdeaSourceRecord>(
        "INSERT INTO idea_source_objects \
         (id, idea_id, client_request_id, request_hash, kind, original_filename, \
          display_name, declared_media_type, trusted_media_type, size_bytes, sha256, \
          storage_key, note, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 'human') \
         RETURNING *",
    )
    .bind(source_id)
    .bind(idea_id)
    .bind(query.client_request_id)
    .bind(&request_hash)
    .bind(kind)
    .bind(&original_filename)
    .bind(&display_name)
    .bind(&declared_media_type)
    .bind(&trusted_media_type)
    .bind(bytes.len() as i64)
    .bind(&sha256)
    .bind(&storage_key)
    .bind(&note)
    .fetch_one(&mut *transaction)
    .await?;
    let revision = IdeaRevisionDraft {
        title: current.title,
        body: current.body,
        source_kind: kind.into(),
        source_ref: Some(format!("idea-source:{source_id}")),
        revision_reason: Some(format!("附加来源：{display_name}")),
    }
    .validate(true)?;
    insert_idea_revision(&mut transaction, idea_id, next, &revision).await?;
    copy_revision_sources(&mut transaction, idea_id, idea.current_revision, next).await?;
    sqlx::query(
        "INSERT INTO idea_revision_sources \
         (idea_id, idea_revision, source_id, role) VALUES ($1, $2, $3, 'material')",
    )
    .bind(idea_id)
    .bind(next)
    .bind(source_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE ideas SET current_revision = $1, \
         state = CASE WHEN state = 'promoted' THEN state ELSE 'developing' END, \
         updated_at = now() WHERE id = $2",
    )
    .bind(next)
    .bind(idea_id)
    .execute(&mut *transaction)
    .await?;
    insert_event(
        &mut transaction,
        "idea",
        idea_id,
        "idea.source_attached",
        query.client_request_id,
        json!({
            "sourceId": source_id,
            "revision": next,
            "kind": kind,
            "sha256": sha256,
            "trustedMediaType": trusted_media_type,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(IdeaSourceResponse {
        replayed: false,
        idea_revision: next,
        source,
    })
}

pub async fn read_source(
    pool: &PgPool,
    artifact_root: &Path,
    idea_id: Uuid,
    source_id: Uuid,
) -> AppResult<(IdeaSourceRecord, Vec<u8>)> {
    let source = sqlx::query_as::<_, IdeaSourceRecord>(
        "SELECT * FROM idea_source_objects WHERE id = $1 AND idea_id = $2",
    )
    .bind(source_id)
    .bind(idea_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("想法来源不存在"))?;
    let path = safe_storage_path(artifact_root, &source.storage_key)?;
    let bytes = fs::read(path).await?;
    if bytes.len() as i64 != source.size_bytes
        || hex::encode(Sha256::digest(&bytes)) != source.sha256
    {
        return Err(AppError::internal("想法来源对象完整性校验失败"));
    }
    Ok((source, bytes))
}

pub async fn run_command(
    pool: &PgPool,
    subject_id: Option<Uuid>,
    request: IdeaCommandRequest,
) -> AppResult<IdeaCommandResponse> {
    let action = clean_required("命令类型", request.action.clone(), 120)?;
    let identity = request.identity(subject_id)?;
    let mut transaction = pool.begin().await?;

    let existing: Option<(Option<Uuid>, String, String, Json<Value>)> = sqlx::query_as(
        "SELECT subject_id, command_kind, input_hash, result FROM idea_command_receipts \
         WHERE client_request_id = $1",
    )
    .bind(request.client_request_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if let Some((stored_subject, command_kind, input_hash, result)) = existing {
        if stored_subject != subject_id {
            return Err(AppError::conflict(
                "idempotency_conflict",
                "同一 clientRequestId 已用于另一个想法或提案",
            ));
        }
        crate::goal_domain::CommandReceiptIdentity {
            command_kind,
            input_hash,
        }
        .ensure_replay_matches(&identity)?;
        transaction.commit().await?;
        return Ok(IdeaCommandResponse {
            replayed: true,
            result: result.0,
        });
    }

    let result = match action.as_str() {
        "idea.create" => {
            require_no_subject(subject_id)?;
            create_idea(
                &mut transaction,
                request.client_request_id,
                decode_payload::<CreateIdeaInput>(request.payload)?,
            )
            .await?
        }
        "idea.revise" => {
            revise_idea(
                &mut transaction,
                require_subject(subject_id, "想法")?,
                request.client_request_id,
                decode_payload::<ReviseIdeaInput>(request.payload)?,
            )
            .await?
        }
        "idea.link" => {
            link_idea(
                &mut transaction,
                require_subject(subject_id, "想法")?,
                request.client_request_id,
                decode_payload::<LinkIdeaInput>(request.payload)?,
            )
            .await?
        }
        "idea.archive" => {
            archive_idea(
                &mut transaction,
                require_subject(subject_id, "想法")?,
                request.client_request_id,
                decode_payload::<ArchiveIdeaInput>(request.payload)?,
            )
            .await?
        }
        "project_proposal.create" => {
            create_project_proposal(
                &mut transaction,
                require_subject(subject_id, "想法")?,
                request.client_request_id,
                decode_payload::<CreateProjectProposalInput>(request.payload)?,
            )
            .await?
        }
        "project_proposal.revise" => {
            revise_project_proposal(
                &mut transaction,
                require_subject(subject_id, "ProjectProposal")?,
                request.client_request_id,
                decode_payload::<ReviseProjectProposalInput>(request.payload)?,
            )
            .await?
        }
        "project_proposal.submit" => {
            submit_project_proposal(
                &mut transaction,
                require_subject(subject_id, "ProjectProposal")?,
                request.client_request_id,
                decode_payload::<ProposalVersionInput>(request.payload)?,
            )
            .await?
        }
        "project_proposal.approve" => {
            approve_project_proposal(
                &mut transaction,
                require_subject(subject_id, "ProjectProposal")?,
                request.client_request_id,
                decode_payload::<ProposalVersionInput>(request.payload)?,
            )
            .await?
        }
        "project_proposal.reject" => {
            decide_project_proposal(
                &mut transaction,
                require_subject(subject_id, "ProjectProposal")?,
                request.client_request_id,
                decode_payload::<ProposalDecisionInput>(request.payload)?,
                true,
            )
            .await?
        }
        "project_proposal.cancel" => {
            decide_project_proposal(
                &mut transaction,
                require_subject(subject_id, "ProjectProposal")?,
                request.client_request_id,
                decode_payload::<ProposalDecisionInput>(request.payload)?,
                false,
            )
            .await?
        }
        _ => {
            return Err(AppError::bad_request(
                "unsupported_idea_action",
                "不支持的想法或 ProjectProposal 动作",
            ));
        }
    };

    sqlx::query(
        "INSERT INTO idea_command_receipts \
         (client_request_id, subject_id, command_kind, input_hash, result) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(request.client_request_id)
    .bind(subject_id)
    .bind(&identity.command_kind)
    .bind(&identity.input_hash)
    .bind(Json(result.clone()))
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(IdeaCommandResponse {
        replayed: false,
        result,
    })
}

async fn create_idea(
    transaction: &mut IdeaTransaction<'_>,
    request_id: Uuid,
    input: CreateIdeaInput,
) -> AppResult<Value> {
    let revision = input.revision.validate(false)?;
    let idea_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO ideas (id, state, current_revision, created_by) \
         VALUES ($1, 'captured', 1, 'human')",
    )
    .bind(idea_id)
    .execute(&mut **transaction)
    .await?;
    insert_idea_revision(transaction, idea_id, 1, &revision).await?;
    insert_event(
        transaction,
        "idea",
        idea_id,
        "idea.created",
        request_id,
        json!({ "revision": 1, "sourceKind": revision.source_kind }),
    )
    .await?;
    Ok(json!({ "ideaId": idea_id, "revision": 1 }))
}

async fn revise_idea(
    transaction: &mut IdeaTransaction<'_>,
    idea_id: Uuid,
    request_id: Uuid,
    input: ReviseIdeaInput,
) -> AppResult<Value> {
    let idea = lock_idea(transaction, idea_id).await?;
    if idea.state == "archived" {
        return Err(AppError::conflict(
            "idea_archived",
            "已归档想法不能继续修订",
        ));
    }
    ensure_revision(idea.current_revision, input.expected_revision)?;
    let revision = input.revision.validate(true)?;
    let next = idea.current_revision + 1;
    insert_idea_revision(transaction, idea_id, next, &revision).await?;
    copy_revision_sources(transaction, idea_id, idea.current_revision, next).await?;
    sqlx::query(
        "UPDATE ideas SET current_revision = $1, \
         state = CASE WHEN state = 'promoted' THEN state ELSE 'developing' END, \
         updated_at = now() \
         WHERE id = $2",
    )
    .bind(next)
    .bind(idea_id)
    .execute(&mut **transaction)
    .await?;
    insert_event(
        transaction,
        "idea",
        idea_id,
        "idea.revised",
        request_id,
        json!({ "fromRevision": idea.current_revision, "revision": next }),
    )
    .await?;
    Ok(json!({ "ideaId": idea_id, "revision": next }))
}

async fn link_idea(
    transaction: &mut IdeaTransaction<'_>,
    source_id: Uuid,
    request_id: Uuid,
    input: LinkIdeaInput,
) -> AppResult<Value> {
    if source_id == input.target_idea_id {
        return Err(AppError::bad_request("self_idea_link", "想法不能关联自身"));
    }
    let source = lock_idea(transaction, source_id).await?;
    let target = lock_idea(transaction, input.target_idea_id).await?;
    ensure_revision(source.current_revision, input.expected_source_revision)?;
    ensure_revision(target.current_revision, input.expected_target_revision)?;
    let relation = validate_idea_relation(input.relation)?;
    let rationale = clean_required("关联原因", input.rationale, 4_000)?;
    let link_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO idea_links (id, source_idea_id, source_revision, target_idea_id, \
         target_revision, relation, rationale, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'human')",
    )
    .bind(link_id)
    .bind(source_id)
    .bind(source.current_revision)
    .bind(target.id)
    .bind(target.current_revision)
    .bind(&relation)
    .bind(&rationale)
    .execute(&mut **transaction)
    .await?;
    insert_event(
        transaction,
        "idea",
        source_id,
        "idea.linked",
        request_id,
        json!({ "linkId": link_id, "targetIdeaId": target.id, "relation": relation }),
    )
    .await?;
    Ok(json!({ "linkId": link_id }))
}

async fn archive_idea(
    transaction: &mut IdeaTransaction<'_>,
    idea_id: Uuid,
    request_id: Uuid,
    input: ArchiveIdeaInput,
) -> AppResult<Value> {
    let idea = lock_idea(transaction, idea_id).await?;
    if idea.state == "archived" {
        return Ok(json!({ "ideaId": idea_id, "state": "archived" }));
    }
    let reason = clean_required("归档原因", input.reason, 4_000)?;
    sqlx::query(
        "UPDATE ideas SET state = 'archived', archived_at = now(), updated_at = now() \
         WHERE id = $1",
    )
    .bind(idea_id)
    .execute(&mut **transaction)
    .await?;
    insert_event(
        transaction,
        "idea",
        idea_id,
        "idea.archived",
        request_id,
        json!({ "reason": reason }),
    )
    .await?;
    Ok(json!({ "ideaId": idea_id, "state": "archived" }))
}

async fn create_project_proposal(
    transaction: &mut IdeaTransaction<'_>,
    idea_id: Uuid,
    request_id: Uuid,
    input: CreateProjectProposalInput,
) -> AppResult<Value> {
    let idea = lock_idea(transaction, idea_id).await?;
    if idea.state == "archived" {
        return Err(AppError::conflict(
            "idea_archived",
            "已归档想法不能创建 ProjectProposal",
        ));
    }
    let revision = input.revision.validate(false, false)?;
    if !revision
        .sources
        .iter()
        .any(|source| source.idea_id == idea_id && source.role == "source")
    {
        return Err(AppError::bad_request(
            "subject_idea_not_primary",
            "当前想法必须作为 source 出现在 ProjectProposal 中",
        ));
    }
    verify_sources(transaction, &revision).await?;
    let proposal_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO project_proposals \
         (id, status, current_revision, created_by) VALUES ($1, 'draft', 1, 'human')",
    )
    .bind(proposal_id)
    .execute(&mut **transaction)
    .await?;
    insert_proposal_revision(transaction, proposal_id, 1, &revision).await?;
    sqlx::query(
        "UPDATE ideas SET state = CASE WHEN state = 'promoted' THEN state ELSE 'proposed' END, \
         updated_at = now() WHERE id = ANY($1)",
    )
    .bind(
        revision
            .sources
            .iter()
            .map(|item| item.idea_id)
            .collect::<Vec<_>>(),
    )
    .execute(&mut **transaction)
    .await?;
    insert_event(
        transaction,
        "project_proposal",
        proposal_id,
        "project_proposal.created",
        request_id,
        json!({ "revision": 1, "sourceIdeaId": idea_id }),
    )
    .await?;
    Ok(json!({ "projectProposalId": proposal_id, "revision": 1 }))
}

async fn revise_project_proposal(
    transaction: &mut IdeaTransaction<'_>,
    proposal_id: Uuid,
    request_id: Uuid,
    input: ReviseProjectProposalInput,
) -> AppResult<Value> {
    let proposal = lock_proposal(transaction, proposal_id).await?;
    let next_status = ProjectProposalStatus::try_from(proposal.status.as_str())?.revise()?;
    ensure_revision(proposal.current_revision, input.expected_revision)?;
    let revision = input.revision.validate(false, true)?;
    verify_sources(transaction, &revision).await?;
    let next = proposal.current_revision + 1;
    insert_proposal_revision(transaction, proposal_id, next, &revision).await?;
    sqlx::query(
        "UPDATE project_proposals SET status = $1, current_revision = $2, updated_at = now() \
         WHERE id = $3",
    )
    .bind(next_status.as_str())
    .bind(next)
    .bind(proposal_id)
    .execute(&mut **transaction)
    .await?;
    insert_event(
        transaction,
        "project_proposal",
        proposal_id,
        "project_proposal.revised",
        request_id,
        json!({ "fromRevision": proposal.current_revision, "revision": next }),
    )
    .await?;
    Ok(
        json!({ "projectProposalId": proposal_id, "revision": next, "status": next_status.as_str() }),
    )
}

async fn submit_project_proposal(
    transaction: &mut IdeaTransaction<'_>,
    proposal_id: Uuid,
    request_id: Uuid,
    input: ProposalVersionInput,
) -> AppResult<Value> {
    let proposal = lock_proposal(transaction, proposal_id).await?;
    ensure_revision(proposal.current_revision, input.expected_revision)?;
    let status = ProjectProposalStatus::try_from(proposal.status.as_str())?.submit()?;
    load_proposal_draft(transaction, proposal_id, proposal.current_revision)
        .await?
        .validate(true, false)?;
    sqlx::query("UPDATE project_proposals SET status = $1, updated_at = now() WHERE id = $2")
        .bind(status.as_str())
        .bind(proposal_id)
        .execute(&mut **transaction)
        .await?;
    insert_event(
        transaction,
        "project_proposal",
        proposal_id,
        "project_proposal.submitted",
        request_id,
        json!({ "revision": proposal.current_revision }),
    )
    .await?;
    Ok(json!({ "projectProposalId": proposal_id, "status": status.as_str() }))
}

async fn approve_project_proposal(
    transaction: &mut IdeaTransaction<'_>,
    proposal_id: Uuid,
    request_id: Uuid,
    input: ProposalVersionInput,
) -> AppResult<Value> {
    let proposal = lock_proposal(transaction, proposal_id).await?;
    ensure_revision(proposal.current_revision, input.expected_revision)?;
    let status = ProjectProposalStatus::try_from(proposal.status.as_str())?.approve()?;
    let revision = load_proposal_draft(transaction, proposal_id, proposal.current_revision)
        .await?
        .validate(true, false)?;
    verify_sources(transaction, &revision).await?;

    let project_id = projects::create_project_in_transaction(
        transaction,
        ProjectIntake {
            intent: revision.project_intent.clone(),
        },
        "human",
    )
    .await?;
    sqlx::query("UPDATE projects SET title = $1, updated_at = now() WHERE id = $2")
        .bind(&revision.title)
        .bind(project_id)
        .execute(&mut **transaction)
        .await?;

    let root_proposal_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO goal_branch_proposals \
         (id, project_id, status, current_revision, created_by) \
         VALUES ($1, $2, 'draft', 1, 'human')",
    )
    .bind(root_proposal_id)
    .bind(project_id)
    .execute(&mut **transaction)
    .await?;
    let root = &revision.root_goal;
    sqlx::query(
        "INSERT INTO goal_branch_proposal_revisions \
         (id, proposal_id, revision, why_needed, contract, expected_contributions, \
          exploration_plan, context_inheritance, tool_requirements, capability_policy, inferences, \
          revision_reason, created_by) \
         VALUES ($1, $2, 1, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'human')",
    )
    .bind(Uuid::new_v4())
    .bind(root_proposal_id)
    .bind(&root.why_needed)
    .bind(Json(&root.contract))
    .bind(Json(&root.expected_contributions))
    .bind(Json(&root.exploration_plan))
    .bind(Json(&root.context_inheritance))
    .bind(Json(&root.tool_requirements))
    .bind(Json(&root.capability_policy))
    .bind(Json(&root.inferences))
    .bind(&root.revision_reason)
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        "UPDATE project_proposals SET status = $1, approved_revision = $2, \
         approved_project_id = $3, approved_root_goal_proposal_id = $4, \
         decision_rationale = '用户批准立项', decided_at = now(), updated_at = now() \
         WHERE id = $5",
    )
    .bind(status.as_str())
    .bind(proposal.current_revision)
    .bind(project_id)
    .bind(root_proposal_id)
    .bind(proposal_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO project_origins \
         (project_id, project_proposal_id, proposal_revision, root_goal_proposal_id) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(project_id)
    .bind(proposal_id)
    .bind(proposal.current_revision)
    .bind(root_proposal_id)
    .execute(&mut **transaction)
    .await?;
    for source in &revision.sources {
        sqlx::query(
            "INSERT INTO project_origin_ideas \
             (project_id, idea_id, idea_revision, role, rationale) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(project_id)
        .bind(source.idea_id)
        .bind(source.idea_revision)
        .bind(&source.role)
        .bind(&source.rationale)
        .execute(&mut **transaction)
        .await?;
        if source.role != "omitted" {
            sqlx::query("UPDATE ideas SET state = 'promoted', updated_at = now() WHERE id = $1")
                .bind(source.idea_id)
                .execute(&mut **transaction)
                .await?;
        }
    }
    insert_event(
        transaction,
        "project_proposal",
        proposal_id,
        "project_proposal.approved",
        request_id,
        json!({
            "revision": proposal.current_revision,
            "projectId": project_id,
            "rootGoalProposalId": root_proposal_id,
        }),
    )
    .await?;
    insert_event(
        transaction,
        "project",
        project_id,
        "project.created_from_ideas",
        request_id,
        json!({ "projectProposalId": proposal_id }),
    )
    .await?;
    sqlx::query(
        "INSERT INTO goal_events \
         (id, project_id, aggregate_type, aggregate_id, event_type, actor_type, \
          client_request_id, payload) \
         VALUES ($1, $2, 'project', $2, 'project.created_from_ideas', 'human', $3, $4)",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(request_id)
    .bind(Json(json!({
        "projectProposalId": proposal_id,
        "rootGoalProposalId": root_proposal_id,
    })))
    .execute(&mut **transaction)
    .await?;

    Ok(json!({
        "projectProposalId": proposal_id,
        "status": status.as_str(),
        "projectId": project_id,
        "rootGoalProposalId": root_proposal_id,
    }))
}

async fn decide_project_proposal(
    transaction: &mut IdeaTransaction<'_>,
    proposal_id: Uuid,
    request_id: Uuid,
    input: ProposalDecisionInput,
    reject: bool,
) -> AppResult<Value> {
    let proposal = lock_proposal(transaction, proposal_id).await?;
    let current = ProjectProposalStatus::try_from(proposal.status.as_str())?;
    let status = if reject {
        current.reject()?
    } else {
        current.cancel()?
    };
    let rationale = clean_required("决定理由", input.rationale, 4_000)?;
    sqlx::query(
        "UPDATE project_proposals SET status = $1, decision_rationale = $2, \
         decided_at = now(), updated_at = now() WHERE id = $3",
    )
    .bind(status.as_str())
    .bind(&rationale)
    .bind(proposal_id)
    .execute(&mut **transaction)
    .await?;
    let event_type = if reject {
        "project_proposal.rejected"
    } else {
        "project_proposal.cancelled"
    };
    insert_event(
        transaction,
        "project_proposal",
        proposal_id,
        event_type,
        request_id,
        json!({ "rationale": rationale }),
    )
    .await?;
    Ok(json!({ "projectProposalId": proposal_id, "status": status.as_str() }))
}

async fn insert_idea_revision(
    transaction: &mut IdeaTransaction<'_>,
    idea_id: Uuid,
    revision_number: i32,
    revision: &IdeaRevisionDraft,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO idea_revisions \
         (id, idea_id, revision, title, body, source_kind, source_ref, revision_reason, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'human')",
    )
    .bind(Uuid::new_v4())
    .bind(idea_id)
    .bind(revision_number)
    .bind(&revision.title)
    .bind(&revision.body)
    .bind(&revision.source_kind)
    .bind(&revision.source_ref)
    .bind(&revision.revision_reason)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_proposal_revision(
    transaction: &mut IdeaTransaction<'_>,
    proposal_id: Uuid,
    revision_number: i32,
    revision: &ProjectProposalRevisionDraft,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO project_proposal_revisions \
         (id, proposal_id, revision, title, project_intent, why_now, root_goal, \
          retained_notes, omitted_notes, revision_reason, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'human')",
    )
    .bind(Uuid::new_v4())
    .bind(proposal_id)
    .bind(revision_number)
    .bind(&revision.title)
    .bind(&revision.project_intent)
    .bind(&revision.why_now)
    .bind(Json(&revision.root_goal))
    .bind(Json(&revision.retained_notes))
    .bind(Json(&revision.omitted_notes))
    .bind(&revision.revision_reason)
    .execute(&mut **transaction)
    .await?;
    for source in &revision.sources {
        sqlx::query(
            "INSERT INTO project_proposal_revision_ideas \
             (proposal_id, proposal_revision, idea_id, idea_revision, role, rationale) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(proposal_id)
        .bind(revision_number)
        .bind(source.idea_id)
        .bind(source.idea_revision)
        .bind(&source.role)
        .bind(&source.rationale)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn verify_sources(
    transaction: &mut IdeaTransaction<'_>,
    revision: &ProjectProposalRevisionDraft,
) -> AppResult<()> {
    for source in &revision.sources {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM idea_revisions \
             WHERE idea_id = $1 AND revision = $2)",
        )
        .bind(source.idea_id)
        .bind(source.idea_revision)
        .fetch_one(&mut **transaction)
        .await?;
        if !exists {
            return Err(AppError::bad_request(
                "unknown_idea_revision",
                format!(
                    "想法 {} 的 v{} 不存在",
                    source.idea_id, source.idea_revision
                ),
            ));
        }
    }
    Ok(())
}

async fn load_proposal_draft(
    transaction: &mut IdeaTransaction<'_>,
    proposal_id: Uuid,
    revision: i32,
) -> AppResult<ProjectProposalRevisionDraft> {
    let row = sqlx::query_as::<_, ProposalDraftRow>(
        "SELECT title, project_intent, why_now, root_goal, retained_notes, omitted_notes, \
         revision_reason FROM project_proposal_revisions \
         WHERE proposal_id = $1 AND revision = $2",
    )
    .bind(proposal_id)
    .bind(revision)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::not_found("ProjectProposal 版本不存在"))?;
    let sources = sqlx::query_as::<_, ProjectProposalIdeaRecord>(
        "SELECT * FROM project_proposal_revision_ideas \
         WHERE proposal_id = $1 AND proposal_revision = $2 ORDER BY created_at, idea_id",
    )
    .bind(proposal_id)
    .bind(revision)
    .fetch_all(&mut **transaction)
    .await?;
    Ok(ProjectProposalRevisionDraft {
        title: row.title,
        project_intent: row.project_intent,
        why_now: row.why_now,
        root_goal: serde_json::from_value(row.root_goal.0)?,
        retained_notes: row.retained_notes.0,
        omitted_notes: row.omitted_notes.0,
        sources: sources
            .into_iter()
            .map(|source| crate::idea_domain::ProjectProposalIdeaSource {
                idea_id: source.idea_id,
                idea_revision: source.idea_revision,
                role: source.role,
                rationale: source.rationale,
            })
            .collect(),
        revision_reason: row.revision_reason,
    })
}

async fn copy_revision_sources(
    transaction: &mut IdeaTransaction<'_>,
    idea_id: Uuid,
    from_revision: i32,
    to_revision: i32,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO idea_revision_sources \
         (idea_id, idea_revision, source_id, role) \
         SELECT idea_id, $1, source_id, role FROM idea_revision_sources \
         WHERE idea_id = $2 AND idea_revision = $3",
    )
    .bind(to_revision)
    .bind(idea_id)
    .bind(from_revision)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn write_content_object(
    artifact_root: &Path,
    storage_key: &str,
    expected_sha256: &str,
    bytes: &[u8],
) -> AppResult<()> {
    let object_path = safe_storage_path(artifact_root, storage_key)?;
    let parent = object_path
        .parent()
        .ok_or_else(|| AppError::internal("想法来源对象路径无父目录"))?;
    fs::create_dir_all(parent).await?;
    if fs::try_exists(&object_path).await? {
        let existing = fs::read(&object_path).await?;
        if hex::encode(Sha256::digest(&existing)) != expected_sha256 {
            return Err(AppError::conflict(
                "artifact_hash_mismatch",
                "同摘要内容对象与实际内容不一致",
            ));
        }
        return Ok(());
    }
    let temporary = parent.join(format!(".{expected_sha256}.{}.tmp", Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .await?;
    file.write_all(bytes).await?;
    file.flush().await?;
    drop(file);
    match fs::hard_link(&temporary, &object_path).await {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let existing = fs::read(&object_path).await?;
            if hex::encode(Sha256::digest(&existing)) != expected_sha256 {
                let _ = fs::remove_file(&temporary).await;
                return Err(AppError::conflict(
                    "artifact_hash_mismatch",
                    "并发写入的内容对象与摘要不一致",
                ));
            }
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary).await;
            return Err(error.into());
        }
    }
    fs::remove_file(&temporary).await?;
    Ok(())
}

async fn lock_idea(
    transaction: &mut IdeaTransaction<'_>,
    idea_id: Uuid,
) -> AppResult<IdeaStateRow> {
    sqlx::query_as::<_, IdeaStateRow>(
        "SELECT id, state, current_revision FROM ideas WHERE id = $1 FOR UPDATE",
    )
    .bind(idea_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::not_found("想法不存在"))
}

async fn lock_proposal(
    transaction: &mut IdeaTransaction<'_>,
    proposal_id: Uuid,
) -> AppResult<ProposalStateRow> {
    sqlx::query_as::<_, ProposalStateRow>(
        "SELECT status, current_revision FROM project_proposals \
         WHERE id = $1 FOR UPDATE",
    )
    .bind(proposal_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::not_found("ProjectProposal 不存在"))
}

async fn insert_event(
    transaction: &mut IdeaTransaction<'_>,
    aggregate_type: &str,
    aggregate_id: Uuid,
    event_type: &str,
    request_id: Uuid,
    payload: Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO idea_events \
         (id, aggregate_type, aggregate_id, event_type, actor_type, client_request_id, payload) \
         VALUES ($1, $2, $3, $4, 'human', $5, $6)",
    )
    .bind(Uuid::new_v4())
    .bind(aggregate_type)
    .bind(aggregate_id)
    .bind(event_type)
    .bind(request_id)
    .bind(Json(payload))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn ensure_revision(current: i32, expected: i32) -> AppResult<()> {
    if current == expected {
        Ok(())
    } else {
        Err(AppError::conflict(
            "stale_revision",
            format!("内容已经更新到 v{current}，请刷新后再操作"),
        ))
    }
}

fn decode_payload<T: DeserializeOwned>(payload: Value) -> AppResult<T> {
    serde_json::from_value(payload).map_err(|_| {
        AppError::bad_request("invalid_idea_command_payload", "命令参数缺失或格式不正确")
    })
}

fn require_subject(subject_id: Option<Uuid>, label: &str) -> AppResult<Uuid> {
    subject_id.ok_or_else(|| {
        AppError::bad_request(
            "missing_command_subject",
            format!("{label}命令缺少操作对象"),
        )
    })
}

fn require_no_subject(subject_id: Option<Uuid>) -> AppResult<()> {
    if subject_id.is_none() {
        Ok(())
    } else {
        Err(AppError::bad_request(
            "unexpected_command_subject",
            "创建想法时不能预先指定对象 ID",
        ))
    }
}
