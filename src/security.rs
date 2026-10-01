use std::{net::SocketAddr, sync::Arc};

use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use axum::{
    Form, Json,
    body::{Body, to_bytes},
    extract::{ConnectInfo, Path, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{Html, IntoResponse, Redirect, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use maud::{DOCTYPE, Markup, html};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    config::keyed_digest,
    error::{AppError, AppResult},
    web::AppState,
};

pub const SESSION_COOKIE: &str = "__Host-fudian_session";
pub const CSRF_COOKIE: &str = "__Host-fudian_csrf";

#[derive(Clone, Debug)]
pub struct AuthenticatedSession {
    pub session_id: Uuid,
    pub user_id: i16,
    pub username: String,
    pub csrf_token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct RequestIdentity {
    pub client_ip_digest: String,
    pub user_agent_digest: String,
    pub request_id: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct SessionRecord {
    id: Uuid,
    user_id: i16,
    username: String,
    token_digest: String,
    csrf_digest: String,
    rotated_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(Debug)]
struct NewSession {
    authenticated: AuthenticatedSession,
    session_token: String,
}

#[derive(Deserialize)]
pub struct SetupForm {
    username: String,
    password: String,
    password_confirm: String,
    setup_token: String,
}

#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
}

#[derive(Deserialize)]
pub struct RecoveryForm {
    username: String,
    recovery_code: String,
    new_password: String,
    password_confirm: String,
}

#[derive(Deserialize)]
pub struct PasswordChangeForm {
    current_password: String,
    new_password: String,
    password_confirm: String,
}

pub async fn setup_page(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> AppResult<Response> {
    require_secure_boundary(&state, peer, &headers, false)?;
    if !state.config.security.required() {
        return Ok(Redirect::to("/").into_response());
    }
    if user_exists(&state).await? {
        return Ok(Redirect::to("/auth/login").into_response());
    }
    Ok(no_store_html(auth_page(
        "初始化 Fudian",
        "只有空库可进行一次初始化。初始化 token 来自服务器 secret 文件。",
        html! {
            form method="post" action="/auth/setup" class="auth-form" {
                label { "用户名" input name="username" autocomplete="username" required minlength="3" maxlength="64"; }
                label { "口令（至少 12 位）" input type="password" name="password" autocomplete="new-password" required minlength="12" maxlength="128"; }
                label { "再输入一次" input type="password" name="password_confirm" autocomplete="new-password" required minlength="12" maxlength="128"; }
                label { "初始化 token" input type="password" name="setup_token" autocomplete="off" required minlength="32" maxlength="256"; }
                button class="button button--primary" type="submit" { "初始化单用户" }
            }
        },
    )))
}

pub async fn setup_submit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<SetupForm>,
) -> AppResult<Response> {
    let identity = require_secure_boundary(&state, peer, &headers, true)?;
    require_security_enabled(&state)?;
    rate_limit_auth(&state, "login_ip", &identity.client_ip_digest).await?;
    if form.password != form.password_confirm {
        return Err(AppError::bad_request(
            "password_confirmation_mismatch",
            "两次口令不一致",
        ));
    }
    validate_username(&form.username)?;
    validate_password(&form.password)?;
    let supplied_setup = keyed_digest(
        &state.config.security.auth_pepper,
        "setup-token",
        form.setup_token.as_bytes(),
    );
    if Some(&supplied_setup) != state.config.security.setup_token_digest.as_ref() {
        consume_rate_limit(&state, "login_ip", &identity.client_ip_digest).await?;
        audit_event(
            &state,
            "auth.setup",
            "denied",
            None,
            None,
            &identity,
            json!({"reason":"invalid_setup_token"}),
        )
        .await?;
        return Err(AppError::forbidden("invalid_setup_token", "初始化凭据无效"));
    }

    let password_hash = hash_password(&state, &form.password)?;
    let mut transaction = state.pool.begin().await?;
    let inserted = sqlx::query(
        "INSERT INTO app_users (id, username, password_hash) VALUES (1, $1, $2) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(form.username.trim())
    .bind(password_hash)
    .execute(&mut *transaction)
    .await?;
    if inserted.rows_affected() != 1 {
        transaction.rollback().await?;
        return Err(AppError::conflict(
            "already_initialized",
            "Fudian 已经完成单用户初始化",
        ));
    }
    let recovery_codes = rotate_recovery_codes(&state, &mut transaction, 1, None).await?;
    let session = create_session_in_transaction(
        &state,
        &mut transaction,
        1,
        form.username.trim(),
        1,
        &identity,
        None,
    )
    .await?;
    insert_audit_event(
        &mut transaction,
        "auth.setup",
        "allowed",
        Some(1),
        Some(session.authenticated.session_id),
        &identity,
        json!({"recoveryCodeCount":recovery_codes.len()}),
    )
    .await?;
    transaction.commit().await?;

    let mut response = no_store_html(auth_page(
        "保存恢复码",
        "这些恢复码只显示这一次。请存到 Fudian 服务器之外的可靠位置。",
        html! {
            ol class="recovery-code-list" data-recovery-codes {
                @for code in &recovery_codes { li { code { (code) } } }
            }
            a class="button button--primary" href="/" { "进入 Fudian" }
        },
    ));
    set_session_cookies(&mut response, &session);
    Ok(response)
}

pub async fn login_page(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> AppResult<Response> {
    require_secure_boundary(&state, peer, &headers, false)?;
    if !state.config.security.required() {
        return Ok(Redirect::to("/").into_response());
    }
    if !user_exists(&state).await? {
        return Ok(Redirect::to("/auth/setup").into_response());
    }
    Ok(no_store_html(auth_page(
        "登录 Fudian",
        "单用户私有工作台",
        html! {
            form method="post" action="/auth/login" class="auth-form" {
                label { "用户名" input name="username" autocomplete="username" required maxlength="64"; }
                label { "口令" input type="password" name="password" autocomplete="current-password" required maxlength="128"; }
                button class="button button--primary" type="submit" { "登录" }
            }
            a class="auth-secondary-link" href="/auth/recover" { "使用一次性恢复码" }
        },
    )))
}

pub async fn login_submit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> AppResult<Response> {
    let identity = require_secure_boundary(&state, peer, &headers, true)?;
    require_security_enabled(&state)?;
    let account_digest = keyed_digest(
        &state.config.security.auth_pepper,
        "rate-account",
        form.username.trim().to_ascii_lowercase().as_bytes(),
    );
    ensure_not_rate_limited(&state, "login_ip", &identity.client_ip_digest).await?;
    ensure_not_rate_limited(&state, "login_account", &account_digest).await?;

    let user: Option<(i16, String, String, i64)> = sqlx::query_as(
        "SELECT id, username, password_hash, password_version FROM app_users WHERE lower(username) = lower($1)",
    )
    .bind(form.username.trim())
    .fetch_optional(&state.pool)
    .await?;
    let valid = match &user {
        Some((_, _, password_hash, _)) => verify_password(&state, &form.password, password_hash),
        None => {
            burn_dummy_password_check(&state, &form.password);
            false
        }
    };
    if !valid {
        consume_rate_limit(&state, "login_ip", &identity.client_ip_digest).await?;
        consume_rate_limit(&state, "login_account", &account_digest).await?;
        audit_event(
            &state,
            "auth.login",
            "denied",
            user.as_ref().map(|value| value.0),
            None,
            &identity,
            json!({"reason":"invalid_credentials"}),
        )
        .await?;
        return Err(AppError::forbidden(
            "invalid_credentials",
            "用户名或口令无效",
        ));
    }
    let (user_id, username, _, password_version) = user.expect("valid login has a user");
    let mut transaction = state.pool.begin().await?;
    let session = create_session_in_transaction(
        &state,
        &mut transaction,
        user_id,
        &username,
        password_version,
        &identity,
        None,
    )
    .await?;
    insert_audit_event(
        &mut transaction,
        "auth.login",
        "allowed",
        Some(user_id),
        Some(session.authenticated.session_id),
        &identity,
        json!({}),
    )
    .await?;
    transaction.commit().await?;
    let mut response = Redirect::to("/").into_response();
    set_session_cookies(&mut response, &session);
    Ok(response)
}

pub async fn logout(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth): axum::extract::Extension<AuthenticatedSession>,
    axum::extract::Extension(identity): axum::extract::Extension<RequestIdentity>,
) -> AppResult<Response> {
    sqlx::query(
        "UPDATE auth_sessions SET revoked_at = now(), revocation_reason = 'logout' \
         WHERE id = $1 AND revoked_at IS NULL",
    )
    .bind(auth.session_id)
    .execute(&state.pool)
    .await?;
    audit_event(
        &state,
        "auth.logout",
        "allowed",
        Some(auth.user_id),
        Some(auth.session_id),
        &identity,
        json!({}),
    )
    .await?;
    let mut response = Redirect::to("/auth/login").into_response();
    clear_session_cookies(&mut response);
    Ok(response)
}

pub async fn recovery_page(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> AppResult<Response> {
    require_secure_boundary(&state, peer, &headers, false)?;
    require_security_enabled(&state)?;
    Ok(no_store_html(auth_page(
        "恢复访问",
        "使用一枚未使用的离线恢复码。成功后旧会话和全部旧恢复码都会失效。",
        html! {
            form method="post" action="/auth/recover" class="auth-form" {
                label { "用户名" input name="username" autocomplete="username" required maxlength="64"; }
                label { "恢复码" input name="recovery_code" autocomplete="off" required maxlength="64"; }
                label { "新口令" input type="password" name="new_password" autocomplete="new-password" required minlength="12" maxlength="128"; }
                label { "再输入一次" input type="password" name="password_confirm" autocomplete="new-password" required minlength="12" maxlength="128"; }
                button class="button button--primary" type="submit" { "使用恢复码" }
            }
            a class="auth-secondary-link" href="/auth/login" { "返回登录" }
        },
    )))
}

pub async fn recovery_submit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<RecoveryForm>,
) -> AppResult<Response> {
    let identity = require_secure_boundary(&state, peer, &headers, true)?;
    require_security_enabled(&state)?;
    if form.new_password != form.password_confirm {
        return Err(AppError::bad_request(
            "password_confirmation_mismatch",
            "两次口令不一致",
        ));
    }
    validate_password(&form.new_password)?;
    let account_digest = keyed_digest(
        &state.config.security.auth_pepper,
        "rate-account",
        form.username.trim().to_ascii_lowercase().as_bytes(),
    );
    ensure_not_rate_limited(&state, "recovery_ip", &identity.client_ip_digest).await?;
    ensure_not_rate_limited(&state, "recovery_account", &account_digest).await?;
    let recovery_digest = keyed_digest(
        &state.config.security.auth_pepper,
        "recovery-code",
        normalize_recovery_code(&form.recovery_code).as_bytes(),
    );
    let password_hash = hash_password(&state, &form.new_password)?;
    let request_id = Uuid::new_v4();
    let mut transaction = state.pool.begin().await?;
    let user: Option<(i16, String, i64)> = sqlx::query_as(
        "SELECT id, username, password_version FROM app_users WHERE lower(username) = lower($1) FOR UPDATE",
    )
    .bind(form.username.trim())
    .fetch_optional(&mut *transaction)
    .await?;
    let recovery_id: Option<Uuid> = match &user {
        Some((user_id, _, _)) => sqlx::query_scalar(
            "SELECT id FROM auth_recovery_codes WHERE user_id = $1 AND code_digest = $2 AND used_at IS NULL FOR UPDATE",
        )
        .bind(user_id)
        .bind(&recovery_digest)
        .fetch_optional(&mut *transaction)
        .await?,
        None => None,
    };
    if user.is_none() || recovery_id.is_none() {
        transaction.rollback().await?;
        consume_rate_limit(&state, "recovery_ip", &identity.client_ip_digest).await?;
        consume_rate_limit(&state, "recovery_account", &account_digest).await?;
        audit_event(
            &state,
            "auth.recovery",
            "denied",
            user.as_ref().map(|value| value.0),
            None,
            &identity,
            json!({"reason":"invalid_recovery_credentials"}),
        )
        .await?;
        return Err(AppError::forbidden(
            "invalid_recovery_credentials",
            "用户名或恢复码无效",
        ));
    }
    let (user_id, username, password_version) = user.expect("recovery has a user");
    let next_password_version = password_version + 1;
    sqlx::query(
        "UPDATE app_users SET password_hash = $1, password_version = $2, \
         password_changed_at = now(), updated_at = now() WHERE id = $3",
    )
    .bind(password_hash)
    .bind(next_password_version)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    revoke_all_sessions(&mut transaction, user_id, "password_recovery").await?;
    consume_all_recovery_codes(&mut transaction, user_id, request_id).await?;
    let recovery_codes =
        rotate_recovery_codes(&state, &mut transaction, user_id, Some(request_id)).await?;
    let session = create_session_in_transaction(
        &state,
        &mut transaction,
        user_id,
        &username,
        next_password_version,
        &identity,
        None,
    )
    .await?;
    insert_audit_event(
        &mut transaction,
        "auth.recovery",
        "allowed",
        Some(user_id),
        Some(session.authenticated.session_id),
        &identity,
        json!({"recoveryCodeCount":recovery_codes.len()}),
    )
    .await?;
    transaction.commit().await?;

    let mut response = no_store_html(auth_page(
        "恢复完成",
        "旧会话与旧恢复码已失效。请保存新的一次性恢复码。",
        html! {
            ol class="recovery-code-list" data-recovery-codes {
                @for code in &recovery_codes { li { code { (code) } } }
            }
            a class="button button--primary" href="/" { "进入 Fudian" }
        },
    ));
    set_session_cookies(&mut response, &session);
    Ok(response)
}

pub async fn password_page(
    axum::extract::Extension(auth): axum::extract::Extension<AuthenticatedSession>,
) -> Response {
    no_store_html(auth_page(
        "更改口令",
        &format!("当前用户：{}", auth.username),
        html! {
            form method="post" action="/account/password" class="auth-form" {
                label { "当前口令" input type="password" name="current_password" autocomplete="current-password" required maxlength="128"; }
                label { "新口令" input type="password" name="new_password" autocomplete="new-password" required minlength="12" maxlength="128"; }
                label { "再输入一次" input type="password" name="password_confirm" autocomplete="new-password" required minlength="12" maxlength="128"; }
                button class="button button--primary" type="submit" { "更换口令并撤销旧会话" }
            }
            a class="auth-secondary-link" href="/" { "返回工作台" }
        },
    ))
}

pub async fn password_submit(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth): axum::extract::Extension<AuthenticatedSession>,
    axum::extract::Extension(identity): axum::extract::Extension<RequestIdentity>,
    Form(form): Form<PasswordChangeForm>,
) -> AppResult<Response> {
    if form.new_password != form.password_confirm {
        return Err(AppError::bad_request(
            "password_confirmation_mismatch",
            "两次口令不一致",
        ));
    }
    validate_password(&form.new_password)?;
    let mut transaction = state.pool.begin().await?;
    let (current_hash, password_version): (String, i64) = sqlx::query_as(
        "SELECT password_hash, password_version FROM app_users WHERE id = $1 FOR UPDATE",
    )
    .bind(auth.user_id)
    .fetch_one(&mut *transaction)
    .await?;
    if !verify_password(&state, &form.current_password, &current_hash) {
        transaction.rollback().await?;
        return Err(AppError::forbidden("invalid_credentials", "当前口令无效"));
    }
    let next_password_version = password_version + 1;
    sqlx::query(
        "UPDATE app_users SET password_hash = $1, password_version = $2, \
         password_changed_at = now(), updated_at = now() WHERE id = $3",
    )
    .bind(hash_password(&state, &form.new_password)?)
    .bind(next_password_version)
    .bind(auth.user_id)
    .execute(&mut *transaction)
    .await?;
    revoke_all_sessions(&mut transaction, auth.user_id, "password_changed").await?;
    let request_id = Uuid::new_v4();
    consume_all_recovery_codes(&mut transaction, auth.user_id, request_id).await?;
    let recovery_codes =
        rotate_recovery_codes(&state, &mut transaction, auth.user_id, Some(request_id)).await?;
    let session = create_session_in_transaction(
        &state,
        &mut transaction,
        auth.user_id,
        &auth.username,
        next_password_version,
        &identity,
        None,
    )
    .await?;
    insert_audit_event(
        &mut transaction,
        "auth.password_changed",
        "allowed",
        Some(auth.user_id),
        Some(session.authenticated.session_id),
        &identity,
        json!({"recoveryCodeCount":recovery_codes.len()}),
    )
    .await?;
    transaction.commit().await?;
    let mut response = no_store_html(auth_page(
        "口令已更换",
        "其他会话和旧恢复码均已失效。请保存新恢复码。",
        html! {
            ol class="recovery-code-list" data-recovery-codes {
                @for code in &recovery_codes { li { code { (code) } } }
            }
            a class="button button--primary" href="/" { "返回 Fudian" }
        },
    ));
    set_session_cookies(&mut response, &session);
    Ok(response)
}

pub async fn auth_status(
    State(state): State<Arc<AppState>>,
    auth: Option<axum::extract::Extension<AuthenticatedSession>>,
) -> Json<Value> {
    let Some(axum::extract::Extension(auth)) = auth else {
        return Json(
            json!({"authenticated": false, "securityRequired": state.config.security.required(), "csrfToken": null}),
        );
    };
    Json(json!({
        "authenticated": true,
        "username": auth.username,
        "sessionId": auth.session_id,
        "csrfToken": auth.csrf_token,
        "expiresAt": auth.expires_at,
    }))
}

pub async fn tool_proxy_root(
    State(state): State<Arc<AppState>>,
    Path((tool_lease_id, endpoint_index)): Path<(Uuid, usize)>,
    request: Request,
) -> AppResult<Response> {
    proxy_tool_request(state, tool_lease_id, endpoint_index, String::new(), request).await
}

pub async fn tool_proxy_path(
    State(state): State<Arc<AppState>>,
    Path((tool_lease_id, endpoint_index, path)): Path<(Uuid, usize, String)>,
    request: Request,
) -> AppResult<Response> {
    proxy_tool_request(state, tool_lease_id, endpoint_index, path, request).await
}

async fn proxy_tool_request(
    state: Arc<AppState>,
    tool_lease_id: Uuid,
    endpoint_index: usize,
    path: String,
    request: Request,
) -> AppResult<Response> {
    require_security_enabled(&state)?;
    let auth = request
        .extensions()
        .get::<AuthenticatedSession>()
        .cloned()
        .ok_or_else(|| AppError::forbidden("authentication_required", "需要登录"))?;
    let identity = request
        .extensions()
        .get::<RequestIdentity>()
        .cloned()
        .ok_or_else(|| AppError::forbidden("authentication_required", "需要登录"))?;
    let endpoint_refs: Option<sqlx::types::Json<Vec<String>>> = sqlx::query_scalar(
        "SELECT endpoint_refs FROM tool_leases WHERE id = $1 AND status = 'active' \
         AND soft_expires_at > now() AND hard_expires_at > now()",
    )
    .bind(tool_lease_id)
    .fetch_optional(&state.pool)
    .await?;
    let endpoint_refs = endpoint_refs.ok_or_else(|| {
        AppError::conflict(
            "tool_lease_not_proxyable",
            "ToolLease 不存在、未激活或已经到期",
        )
    })?;
    let endpoint = endpoint_refs
        .0
        .get(endpoint_index)
        .ok_or_else(|| AppError::not_found("ToolLease endpoint 序号不存在"))?;
    let mut target = validated_tool_endpoint(&state, endpoint)?;
    validate_proxy_path(&path)?;
    let target_path = if path.is_empty() {
        "/".to_owned()
    } else {
        format!("/{path}")
    };
    target.set_path(&target_path);
    target.set_query(request.uri().query());

    let method = request.method().clone();
    let request_headers = request.headers().clone();
    let request_body = to_bytes(
        request.into_body(),
        state.config.security.tool_proxy_body_max_bytes,
    )
    .await
    .map_err(|_| {
        AppError::bad_request(
            "tool_proxy_body_too_large",
            "ToolLease 代理请求体超过服务器限制",
        )
    })?;
    let mut upstream_request = state.tool_proxy_client.request(method.clone(), target);
    for name in [
        header::ACCEPT,
        header::ACCEPT_LANGUAGE,
        header::CONTENT_TYPE,
        header::IF_MATCH,
        header::IF_NONE_MATCH,
        header::IF_MODIFIED_SINCE,
        header::IF_UNMODIFIED_SINCE,
        header::RANGE,
    ] {
        if let Some(value) = request_headers.get(&name) {
            upstream_request = upstream_request.header(name, value);
        }
    }
    let mut upstream = match upstream_request.body(request_body).send().await {
        Ok(response) => response,
        Err(_) => {
            audit_event(
                &state,
                "tool_lease.proxy",
                "failed",
                Some(auth.user_id),
                Some(auth.session_id),
                &identity,
                json!({"toolLeaseId":tool_lease_id,"endpointIndex":endpoint_index,"reason":"upstream_unavailable"}),
            )
            .await?;
            return Err(tool_proxy_gateway_error(
                "tool_proxy_upstream_unavailable",
                "ToolLease 内部服务暂时不可用",
            ));
        }
    };
    if upstream.status().is_redirection() {
        audit_event(
            &state,
            "tool_lease.proxy",
            "denied",
            Some(auth.user_id),
            Some(auth.session_id),
            &identity,
            json!({"toolLeaseId":tool_lease_id,"endpointIndex":endpoint_index,"reason":"redirect_denied"}),
        )
        .await?;
        return Err(tool_proxy_gateway_error(
            "tool_proxy_redirect_denied",
            "ToolLease 内部服务返回了不允许的重定向",
        ));
    }
    let response_limit = state.config.security.tool_proxy_response_max_bytes;
    if upstream
        .content_length()
        .is_some_and(|length| length > response_limit as u64)
    {
        return Err(tool_proxy_gateway_error(
            "tool_proxy_response_too_large",
            "ToolLease 内部服务响应超过服务器限制",
        ));
    }
    let status = upstream.status();
    let upstream_headers = upstream.headers().clone();
    let mut body = Vec::new();
    while let Some(chunk) = upstream.chunk().await.map_err(|_| {
        tool_proxy_gateway_error(
            "tool_proxy_response_failed",
            "读取 ToolLease 内部服务响应失败",
        )
    })? {
        if body.len().saturating_add(chunk.len()) > response_limit {
            return Err(tool_proxy_gateway_error(
                "tool_proxy_response_too_large",
                "ToolLease 内部服务响应超过服务器限制",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    audit_event(
        &state,
        "tool_lease.proxy",
        "allowed",
        Some(auth.user_id),
        Some(auth.session_id),
        &identity,
        json!({"toolLeaseId":tool_lease_id,"endpointIndex":endpoint_index,"method":method.as_str(),"status":status.as_u16(),"responseBytes":body.len()}),
    )
    .await?;
    let mut response = Response::builder().status(status);
    for name in [
        header::CONTENT_TYPE,
        header::CONTENT_LANGUAGE,
        header::CONTENT_DISPOSITION,
        header::ETAG,
        header::LAST_MODIFIED,
        header::ACCEPT_RANGES,
        header::CONTENT_RANGE,
    ] {
        if let Some(value) = upstream_headers.get(&name) {
            response = response.header(name, value);
        }
    }
    response
        .header(header::CACHE_CONTROL, "private, no-store")
        .body(Body::from(body))
        .map_err(|_| AppError::internal("无法构造 ToolLease 代理响应"))
}

fn validated_tool_endpoint(state: &AppState, endpoint: &str) -> AppResult<url::Url> {
    let parsed = url::Url::parse(endpoint).map_err(|_| {
        AppError::forbidden("tool_proxy_endpoint_denied", "ToolLease endpoint 格式无效")
    })?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
        || parsed.port_or_known_default().is_none()
    {
        return Err(AppError::forbidden(
            "tool_proxy_endpoint_denied",
            "ToolLease endpoint 必须是无凭据、无路径的 HTTP(S) IP origin",
        ));
    }
    let address = match parsed.host() {
        Some(url::Host::Ipv4(address)) => std::net::IpAddr::V4(address),
        Some(url::Host::Ipv6(address)) => std::net::IpAddr::V6(address),
        _ => {
            return Err(AppError::forbidden(
                "tool_proxy_endpoint_denied",
                "ToolLease endpoint 必须使用明确 IP，不能使用会重新解析的主机名",
            ));
        }
    };
    if !state.config.security.tool_endpoint_ip_allowed(address) {
        return Err(AppError::forbidden(
            "tool_proxy_endpoint_denied",
            "ToolLease endpoint 不在允许的隔离网段",
        ));
    }
    Ok(parsed)
}

fn validate_proxy_path(path: &str) -> AppResult<()> {
    if path.contains('\\')
        || path.chars().any(char::is_control)
        || path.split('/').any(|segment| matches!(segment, "." | ".."))
    {
        return Err(AppError::bad_request(
            "invalid_tool_proxy_path",
            "ToolLease 代理路径无效",
        ));
    }
    Ok(())
}

fn tool_proxy_gateway_error(code: &'static str, message: &'static str) -> AppError {
    AppError::Operation {
        status: StatusCode::BAD_GATEWAY,
        code,
        message: message.to_owned(),
    }
}

pub async fn require_authenticated_session(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    mut request: Request,
    next: Next,
) -> Response {
    if !state.config.security.required()
        || is_public_path(request.uri().path())
        || is_machine_endpoint(request.uri().path())
    {
        return next.run(request).await;
    }
    let identity = match require_secure_boundary(&state, peer, request.headers(), false) {
        Ok(identity) => identity,
        Err(error) => return error.into_response(),
    };
    let session_token = match cookie_value(request.headers(), SESSION_COOKIE) {
        Some(value) => value.to_owned(),
        None => return unauthorized_response(&request),
    };
    let csrf_token = match cookie_value(request.headers(), CSRF_COOKIE) {
        Some(value) => value.to_owned(),
        None => return unauthorized_response(&request),
    };
    let record = match load_session(&state, &session_token).await {
        Ok(Some(record)) => record,
        Ok(None) => return unauthorized_response(&request),
        Err(error) => return error.into_response(),
    };
    let expected_csrf = keyed_digest(
        &state.config.security.auth_pepper,
        "csrf",
        csrf_token.as_bytes(),
    );
    if expected_csrf != record.csrf_digest {
        return unauthorized_response(&request);
    }
    let now = Utc::now();
    if record.expires_at <= now
        || record.last_seen_at + Duration::seconds(state.config.security.session_idle_seconds)
            <= now
    {
        let _ = revoke_session(&state, record.id, "expired").await;
        return unauthorized_response(&request);
    }
    if is_mutating(request.method()) {
        if let Err(error) = verify_write_origin(&state, request.headers()) {
            return error.into_response();
        }
        let header_csrf = request
            .headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok());
        let is_form = request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("application/x-www-form-urlencoded"));
        if !is_form && header_csrf != Some(csrf_token.as_str()) {
            return AppError::forbidden("csrf_failed", "CSRF token 缺失或无效").into_response();
        }
        let high_cost = is_high_cost_path(request.uri().path());
        let scope = if high_cost {
            "high_cost_session"
        } else {
            "mutation_session"
        };
        if let Err(error) = consume_session_rate_limit(&state, scope, &record.token_digest).await {
            return error.into_response();
        }
    }

    let mut rotated = None;
    let authenticated = if record.rotated_at
        + Duration::seconds(state.config.security.session_rotation_seconds)
        <= now
    {
        match rotate_session(&state, &record, &identity).await {
            Ok(new_session) => {
                let authenticated = new_session.authenticated.clone();
                rotated = Some(new_session);
                authenticated
            }
            Err(error) => return error.into_response(),
        }
    } else {
        if let Err(error) = sqlx::query(
            "UPDATE auth_sessions SET last_seen_at = now() WHERE id = $1 AND revoked_at IS NULL",
        )
        .bind(record.id)
        .execute(&state.pool)
        .await
        {
            return AppError::Database(error).into_response();
        }
        AuthenticatedSession {
            session_id: record.id,
            user_id: record.user_id,
            username: record.username,
            csrf_token,
            expires_at: record.expires_at,
        }
    };
    request.extensions_mut().insert(authenticated);
    request.extensions_mut().insert(identity);
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    if let Some(session) = rotated {
        set_session_cookies(&mut response, &session);
    }
    response
}

pub async fn security_response_headers(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    if state.config.security.required() {
        response.headers_mut().insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }
    response
}

fn require_secure_boundary(
    state: &AppState,
    peer: SocketAddr,
    headers: &HeaderMap,
    write: bool,
) -> AppResult<RequestIdentity> {
    if !state.config.security.required() {
        return Ok(RequestIdentity {
            client_ip_digest: keyed_digest(
                b"disabled",
                "client-ip",
                peer.ip().to_string().as_bytes(),
            ),
            user_agent_digest: keyed_digest(b"disabled", "user-agent", b"disabled"),
            request_id: request_id(headers),
        });
    }
    if !state.config.security.peer_is_trusted_proxy(peer.ip()) {
        return Err(AppError::forbidden(
            "untrusted_proxy",
            "安全模式只接受来自明确可信反向代理的请求",
        ));
    }
    let forwarded_proto = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next_back())
        .map(str::trim);
    if forwarded_proto != Some("https") {
        return Err(AppError::forbidden(
            "https_required",
            "安全模式必须通过 HTTPS 访问",
        ));
    }
    let expected_origin = state
        .config
        .security
        .public_origin
        .as_deref()
        .expect("required security has a public origin");
    let expected_authority = url::Url::parse(expected_origin)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?;
            Some(match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_owned(),
            })
        })
        .expect("validated public origin has an authority");
    let forwarded_host = headers
        .get("x-forwarded-host")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next_back())
        .map(str::trim);
    if forwarded_host != Some(expected_authority.as_str()) {
        return Err(AppError::forbidden(
            "target_origin_mismatch",
            "请求目标与配置的公开 origin 不一致",
        ));
    }
    if write {
        verify_write_origin(state, headers)?;
    }
    let client_ip = forwarded_client_ip(state, peer, headers);
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    Ok(RequestIdentity {
        client_ip_digest: keyed_digest(
            &state.config.security.auth_pepper,
            "client-ip",
            client_ip.as_bytes(),
        ),
        user_agent_digest: keyed_digest(
            &state.config.security.auth_pepper,
            "user-agent",
            user_agent.as_bytes(),
        ),
        request_id: request_id(headers),
    })
}

fn verify_write_origin(state: &AppState, headers: &HeaderMap) -> AppResult<()> {
    let expected = state
        .config
        .security
        .public_origin
        .as_deref()
        .expect("required security has a public origin");
    let source = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or_else(|| {
            headers
                .get(header::REFERER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| url::Url::parse(value).ok())
                .map(|url| url.origin().ascii_serialization())
        });
    if source.as_deref() != Some(expected) {
        return Err(AppError::forbidden(
            "csrf_origin_mismatch",
            "请求来源与 Fudian 公开 origin 不一致",
        ));
    }
    if headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value != "same-origin" && value != "none")
    {
        return Err(AppError::forbidden(
            "csrf_fetch_site_mismatch",
            "跨站写请求被拒绝",
        ));
    }
    Ok(())
}

fn forwarded_client_ip(state: &AppState, peer: SocketAddr, headers: &HeaderMap) -> String {
    let forwarded = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .into_iter()
        .flat_map(|value| value.split(','))
        .filter_map(|value| value.trim().parse().ok())
        .collect::<Vec<std::net::IpAddr>>();
    forwarded
        .iter()
        .rev()
        .find(|address| !state.config.security.peer_is_trusted_proxy(**address))
        .copied()
        .or_else(|| forwarded.last().copied())
        .unwrap_or_else(|| peer.ip())
        .to_string()
}

fn request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() <= 128)
        .map(str::to_owned)
}

async fn load_session(state: &AppState, token: &str) -> AppResult<Option<SessionRecord>> {
    if token.len() < 32 || token.len() > 256 {
        return Ok(None);
    }
    let digest = keyed_digest(
        &state.config.security.auth_pepper,
        "session",
        token.as_bytes(),
    );
    let row = sqlx::query_as::<_, SessionRecord>(
        "SELECT s.id, s.user_id, u.username, s.token_digest, s.csrf_digest, \
         s.rotated_at, s.last_seen_at, s.expires_at FROM auth_sessions s \
         JOIN app_users u ON u.id = s.user_id AND u.password_version = s.password_version \
         WHERE s.token_digest = $1 AND s.revoked_at IS NULL",
    )
    .bind(digest)
    .fetch_optional(&state.pool)
    .await?;
    Ok(row)
}

async fn rotate_session(
    state: &AppState,
    old: &SessionRecord,
    identity: &RequestIdentity,
) -> AppResult<NewSession> {
    let mut transaction = state.pool.begin().await?;
    let active: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM auth_sessions WHERE id = $1 AND revoked_at IS NULL FOR UPDATE",
    )
    .bind(old.id)
    .fetch_optional(&mut *transaction)
    .await?;
    if active.is_none() {
        transaction.rollback().await?;
        return Err(AppError::forbidden(
            "session_rotated",
            "会话已被其他请求轮换，请重新登录",
        ));
    }
    let password_version: i64 =
        sqlx::query_scalar("SELECT password_version FROM app_users WHERE id = $1")
            .bind(old.user_id)
            .fetch_one(&mut *transaction)
            .await?;
    let session = create_session_in_transaction(
        state,
        &mut transaction,
        old.user_id,
        &old.username,
        password_version,
        identity,
        Some(old.expires_at),
    )
    .await?;
    sqlx::query(
        "UPDATE auth_sessions SET revoked_at = now(), revocation_reason = 'rotated', \
         replaced_by_session_id = $1 WHERE id = $2 AND revoked_at IS NULL",
    )
    .bind(session.authenticated.session_id)
    .bind(old.id)
    .execute(&mut *transaction)
    .await?;
    insert_audit_event(
        &mut transaction,
        "auth.session_rotated",
        "allowed",
        Some(old.user_id),
        Some(session.authenticated.session_id),
        identity,
        json!({"previousSessionId":old.id}),
    )
    .await?;
    transaction.commit().await?;
    Ok(session)
}

async fn create_session_in_transaction(
    state: &AppState,
    transaction: &mut Transaction<'_, Postgres>,
    user_id: i16,
    username: &str,
    password_version: i64,
    identity: &RequestIdentity,
    absolute_expiry: Option<DateTime<Utc>>,
) -> AppResult<NewSession> {
    let session_token = random_token(32)?;
    let csrf_token = random_token(32)?;
    let token_digest = keyed_digest(
        &state.config.security.auth_pepper,
        "session",
        session_token.as_bytes(),
    );
    let csrf_digest = keyed_digest(
        &state.config.security.auth_pepper,
        "csrf",
        csrf_token.as_bytes(),
    );
    let now = Utc::now();
    let configured_expiry = now + Duration::seconds(state.config.security.session_ttl_seconds);
    let expires_at = absolute_expiry
        .map(|expiry| expiry.min(configured_expiry))
        .unwrap_or(configured_expiry);
    let session_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO auth_sessions (id, user_id, token_digest, csrf_digest, password_version, \
         client_ip_digest, user_agent_digest, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(&token_digest)
    .bind(&csrf_digest)
    .bind(password_version)
    .bind(&identity.client_ip_digest)
    .bind(&identity.user_agent_digest)
    .bind(expires_at)
    .execute(&mut **transaction)
    .await?;
    Ok(NewSession {
        authenticated: AuthenticatedSession {
            session_id,
            user_id,
            username: username.to_owned(),
            csrf_token,
            expires_at,
        },
        session_token,
    })
}

fn hash_password(state: &AppState, password: &str) -> AppResult<String> {
    validate_password(password)?;
    let mut salt_bytes = [0_u8; 16];
    getrandom::fill(&mut salt_bytes)
        .map_err(|_| AppError::internal("无法从操作系统获取安全随机数"))?;
    let salt =
        SaltString::encode_b64(&salt_bytes).map_err(|_| AppError::internal("无法构造口令 salt"))?;
    let params =
        Params::new(19_456, 2, 1, Some(32)).map_err(|_| AppError::internal("Argon2id 参数无效"))?;
    let argon2 = Argon2::new_with_secret(
        &state.config.security.auth_pepper,
        Algorithm::Argon2id,
        Version::V0x13,
        params,
    )
    .map_err(|_| AppError::internal("无法初始化 Argon2id"))?;
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AppError::internal("口令哈希失败"))
}

fn verify_password(state: &AppState, password: &str, encoded: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(encoded) else {
        return false;
    };
    let params = match Params::try_from(&parsed) {
        Ok(params) => params,
        Err(_) => return false,
    };
    let Ok(argon2) = Argon2::new_with_secret(
        &state.config.security.auth_pepper,
        Algorithm::Argon2id,
        Version::V0x13,
        params,
    ) else {
        return false;
    };
    argon2.verify_password(password.as_bytes(), &parsed).is_ok()
}

fn burn_dummy_password_check(state: &AppState, password: &str) {
    const DUMMY: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$E5jQYHdqgYTA0BsQ5jW2zrdMH+IDgUhF3tadGJL3I3A";
    let _ = verify_password(state, password, DUMMY);
}

fn validate_username(username: &str) -> AppResult<()> {
    let username = username.trim();
    if !(3..=64).contains(&username.len())
        || !username.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
    {
        return Err(AppError::bad_request(
            "invalid_username",
            "用户名必须是 3–64 位字母数字或 ._-，且以字母数字开头",
        ));
    }
    Ok(())
}

fn validate_password(password: &str) -> AppResult<()> {
    if !(12..=128).contains(&password.chars().count()) || password.chars().any(char::is_control) {
        return Err(AppError::bad_request(
            "weak_password",
            "口令必须是 12–128 个非控制字符",
        ));
    }
    Ok(())
}

async fn rotate_recovery_codes(
    state: &AppState,
    transaction: &mut Transaction<'_, Postgres>,
    user_id: i16,
    _request_id: Option<Uuid>,
) -> AppResult<Vec<String>> {
    let mut codes = Vec::with_capacity(8);
    for _ in 0..8 {
        let raw = random_recovery_code()?;
        let digest = keyed_digest(
            &state.config.security.auth_pepper,
            "recovery-code",
            normalize_recovery_code(&raw).as_bytes(),
        );
        sqlx::query("INSERT INTO auth_recovery_codes (id, user_id, code_digest) VALUES ($1,$2,$3)")
            .bind(Uuid::new_v4())
            .bind(user_id)
            .bind(digest)
            .execute(&mut **transaction)
            .await?;
        codes.push(raw);
    }
    Ok(codes)
}

async fn consume_all_recovery_codes(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: i16,
    _request_id: Uuid,
) -> AppResult<()> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM auth_recovery_codes WHERE user_id = $1 AND used_at IS NULL FOR UPDATE",
    )
    .bind(user_id)
    .fetch_all(&mut **transaction)
    .await?;
    for id in ids {
        let derived_request = Uuid::new_v4();
        sqlx::query(
            "UPDATE auth_recovery_codes SET used_at = now(), used_request_id = $1 WHERE id = $2",
        )
        .bind(derived_request)
        .bind(id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn revoke_all_sessions(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: i16,
    reason: &str,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE auth_sessions SET revoked_at = now(), revocation_reason = $1 \
         WHERE user_id = $2 AND revoked_at IS NULL",
    )
    .bind(reason)
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn random_token(bytes: usize) -> AppResult<String> {
    let mut value = vec![0_u8; bytes];
    getrandom::fill(&mut value).map_err(|_| AppError::internal("无法从操作系统获取安全随机数"))?;
    Ok(URL_SAFE_NO_PAD.encode(value))
}

fn random_recovery_code() -> AppResult<String> {
    let mut value = [0_u8; 20];
    getrandom::fill(&mut value).map_err(|_| AppError::internal("无法从操作系统获取安全随机数"))?;
    let encoded = hex::encode_upper(value);
    Ok(encoded
        .as_bytes()
        .chunks(8)
        .map(|chunk| std::str::from_utf8(chunk).expect("hex is UTF-8"))
        .collect::<Vec<_>>()
        .join("-"))
}

fn normalize_recovery_code(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_hexdigit())
        .flat_map(char::to_uppercase)
        .collect()
}

async fn user_exists(state: &AppState) -> AppResult<bool> {
    Ok(
        sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM app_users WHERE id = 1)")
            .fetch_one(&state.pool)
            .await?,
    )
}

fn require_security_enabled(state: &AppState) -> AppResult<()> {
    if !state.config.security.required() {
        return Err(AppError::forbidden(
            "security_disabled",
            "当前实例明确关闭了身份系统",
        ));
    }
    Ok(())
}

async fn revoke_session(state: &AppState, session_id: Uuid, reason: &str) -> AppResult<()> {
    sqlx::query(
        "UPDATE auth_sessions SET revoked_at = now(), revocation_reason = $1 \
         WHERE id = $2 AND revoked_at IS NULL",
    )
    .bind(reason)
    .bind(session_id)
    .execute(&state.pool)
    .await?;
    Ok(())
}

async fn ensure_not_rate_limited(state: &AppState, scope: &str, key: &str) -> AppResult<()> {
    let blocked: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM auth_rate_limit_buckets \
         WHERE scope = $1 AND key_digest = $2 AND blocked_until > now())",
    )
    .bind(scope)
    .bind(key)
    .fetch_one(&state.pool)
    .await?;
    if blocked {
        return Err(rate_limit_error());
    }
    Ok(())
}

async fn rate_limit_auth(state: &AppState, scope: &str, key: &str) -> AppResult<()> {
    ensure_not_rate_limited(state, scope, key).await
}

async fn consume_rate_limit(state: &AppState, scope: &str, key: &str) -> AppResult<()> {
    update_rate_limit(
        state,
        scope,
        key,
        state.config.security.login_window_seconds,
        state.config.security.login_attempts_per_window,
    )
    .await
}

async fn consume_session_rate_limit(state: &AppState, scope: &str, key: &str) -> AppResult<()> {
    let limit = if scope == "high_cost_session" {
        state.config.security.high_cost_attempts_per_window
    } else {
        state.config.security.mutation_attempts_per_window
    };
    update_rate_limit(
        state,
        scope,
        key,
        state.config.security.mutation_window_seconds,
        limit,
    )
    .await
}

async fn update_rate_limit(
    state: &AppState,
    scope: &str,
    key: &str,
    window_seconds: i64,
    limit: i32,
) -> AppResult<()> {
    let mut transaction = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO auth_rate_limit_buckets \
         (scope, key_digest, window_started_at, attempt_count) VALUES ($1,$2,now(),0) \
         ON CONFLICT (scope, key_digest) DO NOTHING",
    )
    .bind(scope)
    .bind(key)
    .execute(&mut *transaction)
    .await?;
    let (window_started_at, attempt_count, blocked_until): (
        DateTime<Utc>,
        i32,
        Option<DateTime<Utc>>,
    ) = sqlx::query_as(
        "SELECT window_started_at, attempt_count, blocked_until \
         FROM auth_rate_limit_buckets WHERE scope = $1 AND key_digest = $2 FOR UPDATE",
    )
    .bind(scope)
    .bind(key)
    .fetch_one(&mut *transaction)
    .await?;
    let now = Utc::now();
    if blocked_until.is_some_and(|until| until > now) {
        transaction.commit().await?;
        return Err(rate_limit_error());
    }
    let window_end = window_started_at + Duration::seconds(window_seconds);
    let (next_started, next_count) = if window_end <= now {
        (now, 1)
    } else {
        (window_started_at, attempt_count + 1)
    };
    let next_blocked =
        (next_count > limit).then_some(next_started + Duration::seconds(window_seconds));
    sqlx::query(
        "UPDATE auth_rate_limit_buckets SET window_started_at = $1, attempt_count = $2, \
         blocked_until = $3, updated_at = now() WHERE scope = $4 AND key_digest = $5",
    )
    .bind(next_started)
    .bind(next_count)
    .bind(next_blocked)
    .bind(scope)
    .bind(key)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    if next_blocked.is_some() {
        Err(rate_limit_error())
    } else {
        Ok(())
    }
}

fn rate_limit_error() -> AppError {
    AppError::Operation {
        status: StatusCode::TOO_MANY_REQUESTS,
        code: "rate_limited",
        message: "请求过于频繁，请等待当前窗口结束后再试".to_owned(),
    }
}

async fn audit_event(
    state: &AppState,
    event_type: &str,
    outcome: &str,
    user_id: Option<i16>,
    session_id: Option<Uuid>,
    identity: &RequestIdentity,
    detail: Value,
) -> AppResult<()> {
    let mut transaction = state.pool.begin().await?;
    insert_audit_event(
        &mut transaction,
        event_type,
        outcome,
        user_id,
        session_id,
        identity,
        detail,
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn insert_audit_event(
    transaction: &mut Transaction<'_, Postgres>,
    event_type: &str,
    outcome: &str,
    user_id: Option<i16>,
    session_id: Option<Uuid>,
    identity: &RequestIdentity,
    detail: Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO security_audit_events \
         (id, event_type, outcome, user_id, session_id, request_id, client_ip_digest, \
          user_agent_digest, detail) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(Uuid::new_v4())
    .bind(event_type)
    .bind(outcome)
    .bind(user_id)
    .bind(session_id)
    .bind(&identity.request_id)
    .bind(&identity.client_ip_digest)
    .bind(&identity.user_agent_digest)
    .bind(detail)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn set_session_cookies(response: &mut Response, session: &NewSession) {
    let max_age = (session.authenticated.expires_at - Utc::now())
        .num_seconds()
        .max(1);
    append_set_cookie(
        response,
        &format!(
            "{SESSION_COOKIE}={}; Path=/; Max-Age={max_age}; Secure; HttpOnly; SameSite=Strict",
            session.session_token
        ),
    );
    append_set_cookie(
        response,
        &format!(
            "{CSRF_COOKIE}={}; Path=/; Max-Age={max_age}; Secure; SameSite=Strict",
            session.authenticated.csrf_token
        ),
    );
}

fn clear_session_cookies(response: &mut Response) {
    append_set_cookie(
        response,
        &format!("{SESSION_COOKIE}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Strict"),
    );
    append_set_cookie(
        response,
        &format!("{CSRF_COOKIE}=; Path=/; Max-Age=0; Secure; SameSite=Strict"),
    );
}

fn append_set_cookie(response: &mut Response, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(candidate, value)| (candidate == name).then_some(value))
}

fn unauthorized_response(request: &Request) -> Response {
    let wants_html = !request.uri().path().starts_with("/api/")
        && request
            .headers()
            .get(header::ACCEPT)
            .and_then(|value| value.to_str().ok())
            .is_none_or(|value| value.contains("text/html") || value.contains("*/*"));
    if wants_html && request.method() == Method::GET {
        Redirect::temporary("/auth/login").into_response()
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"code":"authentication_required","error":"需要登录"})),
        )
            .into_response()
    }
}

fn is_mutating(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn is_high_cost_path(path: &str) -> bool {
    path.contains("/chunks")
        || path.contains("/tool-calls")
        || path.contains("/tool-executions")
        || path.contains("/tool-leases")
        || path.contains("/plugins/install")
        || path.contains("/runner-jobs")
        || (path.starts_with("/api/maitu/tasks/") && path.ends_with("/start"))
        || (path.starts_with("/api/maitu/projects/") && path.ends_with("/plans"))
        || (path.starts_with("/api/maitu/projects/") && path.ends_with("/code"))
        || (path.starts_with("/api/maitu/tasks/") && path.ends_with("/accept"))
}

fn is_public_path(path: &str) -> bool {
    path == "/api/health"
        || path == "/auth/setup"
        || path == "/auth/login"
        || path == "/auth/recover"
        || path.starts_with("/assets/")
}

fn is_machine_endpoint(path: &str) -> bool {
    path.starts_with("/api/v1/scheduler/")
        || (path.contains("/runner-jobs/")
            && (path.ends_with("/finalize") || path.ends_with("/fail")))
        || (path.contains("/tool-executions/") && path.ends_with("/finalize"))
}

fn no_store_html(markup: Markup) -> Response {
    let mut response = Html(markup.into_string()).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

fn auth_page(title: &str, introduction: &str, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="zh-CN" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="color-scheme" content="light dark";
                title { (title) " · 浮点" }
                link rel="stylesheet" href="/assets/app.css";
                script defer src="/assets/theme.js" {}
            }
            body class="auth-shell" {
                main class="auth-card" {
                    p class="eyebrow" { "FUDIAN · PRIVATE WORKSPACE" }
                    h1 { (title) }
                    p class="auth-introduction" { (introduction) }
                    (content)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_codes_normalize_without_ambiguity() {
        assert_eq!(normalize_recovery_code("ab12-cd34 EF56"), "AB12CD34EF56");
    }

    #[test]
    fn high_cost_routes_are_narrowly_classified() {
        assert!(is_high_cost_path("/api/v1/x/tool-calls"));
        assert!(is_high_cost_path("/api/v1/x/inputs/id/chunks"));
        assert!(!is_high_cost_path("/projects/id/goal-commands"));
    }
}
