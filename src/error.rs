use axum::{
    Json,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;
use tracing::error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{message}")]
    Operation {
        status: StatusCode,
        code: &'static str,
        message: String,
    },
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl AppError {
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::Operation {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code,
            message: message.into(),
        }
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::Operation {
            status: StatusCode::CONFLICT,
            code,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::Operation {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Operation {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: message.into(),
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::Operation { status, .. } => *status,
            Self::Database(_) | Self::Io(_) | Self::Json(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Operation { code, .. } => code,
            Self::Database(_) => "database_error",
            Self::Io(_) => "io_error",
            Self::Json(_) => "json_error",
        }
    }

    pub fn public_message(&self) -> String {
        match self {
            Self::Operation { message, .. } => message.clone(),
            _ => "服务暂时无法完成请求，请稍后重试".to_owned(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let code = self.code();
        if status.is_server_error() {
            error!(error = %self, %code, "请求处理失败");
        }
        let message = self.public_message();
        let mut response =
            (status, Json(json!({ "error": message, "code": code }))).into_response();
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
        response
    }
}

pub type AppResult<T> = Result<T, AppError>;
