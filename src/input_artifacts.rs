use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, types::Json};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeginInputArtifact {
    pub client_request_id: Uuid,
    pub filename: String,
    pub declared_media_type: Option<String>,
    pub declared_size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishInputArtifact {
    pub client_request_id: Uuid,
    pub expected_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportInputArtifact {
    pub client_request_id: Uuid,
    pub inbox_relative_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkQuery {
    pub client_request_id: Uuid,
    pub offset: u64,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputArtifactRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub session_id: Uuid,
    pub client_request_id: Uuid,
    pub status: String,
    pub original_filename: String,
    pub display_name: String,
    pub declared_media_type: Option<String>,
    pub trusted_media_type: Option<String>,
    pub declared_size: i64,
    pub actual_size: i64,
    pub sha256: Option<String>,
    pub storage_key: String,
    pub verification: Json<Value>,
    pub import_mode: Option<String>,
    pub inbox_relative_path: Option<String>,
    pub artifact_id: Option<Uuid>,
    pub finish_client_request_id: Option<Uuid>,
    pub finish_request_hash: Option<String>,
    pub import_client_request_id: Option<Uuid>,
    pub import_request_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub available_at: Option<DateTime<Utc>>,
    pub imported_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputChunkRecord {
    pub input_artifact_id: Uuid,
    pub offset_bytes: i64,
    pub size_bytes: i64,
    pub sha256: String,
    pub storage_key: String,
    pub client_request_id: Uuid,
    pub created_at: DateTime<Utc>,
}

pub fn normalize_upload_name(value: String) -> AppResult<(String, String)> {
    let original = value.trim().to_owned();
    if original.is_empty() || original.chars().count() > 512 || original.contains('\0') {
        return Err(AppError::bad_request(
            "invalid_filename",
            "文件名为空、过长或含 NUL",
        ));
    }
    let display = original
        .split(['/', '\\'])
        .rfind(|segment| !segment.trim().is_empty())
        .unwrap_or("unnamed")
        .chars()
        .map(|character| {
            if character.is_control() {
                '�'
            } else {
                character
            }
        })
        .collect::<String>();
    let display = display.trim().trim_matches('.').trim().to_owned();
    let display = if display.is_empty() {
        "unnamed".to_owned()
    } else {
        display
    };
    Ok((original, display))
}

pub fn normalize_declared_media_type(value: Option<String>) -> AppResult<Option<String>> {
    value
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            if value.is_empty() {
                return Ok(None);
            }
            if value.len() > 200
                || !value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(byte, b'/' | b'+' | b'-' | b'.' | b';' | b'=' | b' ')
                })
            {
                return Err(AppError::bad_request(
                    "invalid_media_type",
                    "声明的 media type 不合法",
                ));
            }
            Ok(Some(value))
        })
        .unwrap_or(Ok(None))
}

pub fn normalize_inbox_path(value: String) -> AppResult<String> {
    let normalized = value.trim().replace('\\', "/");
    if normalized.is_empty()
        || normalized.len() > 1_000
        || normalized.starts_with('/')
        || normalized.contains('\0')
        || normalized.contains(':')
    {
        return Err(unsafe_path());
    }
    let path = Path::new(&normalized);
    for component in path.components() {
        match component {
            Component::Normal(segment) => {
                let segment = segment.to_string_lossy();
                if segment.is_empty()
                    || segment.ends_with(['.', ' '])
                    || is_windows_reserved_name(&segment)
                {
                    return Err(unsafe_path());
                }
            }
            _ => return Err(unsafe_path()),
        }
    }
    if normalized.split('/').any(|segment| segment.is_empty()) {
        return Err(unsafe_path());
    }
    Ok(normalized)
}

pub fn safe_storage_path(root: &Path, storage_key: &str) -> AppResult<PathBuf> {
    let storage_key = normalize_inbox_path(storage_key.to_owned())?;
    Ok(root.join(storage_key))
}

pub fn sniff_media_type(prefix: &[u8]) -> &'static str {
    if prefix.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if prefix.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if prefix.starts_with(b"GIF87a") || prefix.starts_with(b"GIF89a") {
        "image/gif"
    } else if prefix.len() >= 12 && &prefix[..4] == b"RIFF" && &prefix[8..12] == b"WEBP" {
        "image/webp"
    } else if prefix.len() >= 12 && &prefix[..4] == b"RIFF" && &prefix[8..12] == b"WAVE" {
        "audio/wav"
    } else if prefix.starts_with(b"ID3")
        || (prefix.len() >= 2 && prefix[0] == 0xff && prefix[1] & 0xe0 == 0xe0)
    {
        "audio/mpeg"
    } else if prefix.starts_with(b"OggS") {
        "audio/ogg"
    } else if prefix.len() >= 12 && &prefix[4..8] == b"ftyp" {
        "audio/mp4"
    } else if prefix.starts_with(b"%PDF-") {
        "application/pdf"
    } else if prefix.starts_with(b"PK\x03\x04")
        || prefix.starts_with(b"PK\x05\x06")
        || prefix.starts_with(b"PK\x07\x08")
    {
        "application/zip"
    } else if prefix.starts_with(&[0x1f, 0x8b]) {
        "application/gzip"
    } else if let Ok(text) = std::str::from_utf8(prefix) {
        let trimmed = text.trim_start_matches('\u{feff}').trim_start();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            "application/json"
        } else if !text.contains('\0') {
            "text/plain; charset=utf-8"
        } else {
            "application/octet-stream"
        }
    } else {
        "application/octet-stream"
    }
}

pub fn is_archive_media_type(media_type: &str) -> bool {
    matches!(media_type, "application/zip" | "application/gzip")
}

pub fn can_copy_to_inbox(media_type: &str, size: u64, threshold: u64) -> bool {
    size <= threshold
        && (media_type.starts_with("text/")
            || matches!(
                media_type,
                "application/json" | "application/xml" | "image/svg+xml"
            ))
}

pub fn normalize_bare_sha256(value: String) -> AppResult<String> {
    let value = value.trim().to_ascii_lowercase();
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::bad_request(
            "invalid_digest",
            "SHA-256 摘要不合法",
        ));
    }
    Ok(value)
}

fn is_windows_reserved_name(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or(segment)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

fn unsafe_path() -> AppError {
    AppError::bad_request(
        "unsafe_artifact_path",
        "inbox 路径必须是安全、规范的相对路径",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_filename_is_display_only() {
        let (original, display) =
            normalize_upload_name("../../Windows\\report.txt".into()).unwrap();
        assert_eq!(original, "../../Windows\\report.txt");
        assert_eq!(display, "report.txt");
    }

    #[test]
    fn inbox_path_rejects_traversal_absolute_and_reserved_names() {
        for unsafe_value in ["../x", "/etc/passwd", "C:\\x", "a//b", "NUL.txt"] {
            assert_eq!(
                normalize_inbox_path(unsafe_value.into())
                    .unwrap_err()
                    .code(),
                "unsafe_artifact_path"
            );
        }
        assert_eq!(
            normalize_inbox_path("inputs/report.txt".into()).unwrap(),
            "inputs/report.txt"
        );
    }

    #[test]
    fn media_sniffing_does_not_trust_the_extension() {
        assert_eq!(sniff_media_type(b"\x89PNG\r\n\x1a\nrest"), "image/png");
        assert_eq!(sniff_media_type(b"{\"safe\":true}"), "application/json");
        assert_eq!(sniff_media_type(b"ID3\x04\0\0voice"), "audio/mpeg");
        assert_eq!(sniff_media_type(b"\xff\xd8\xff\xe0photo"), "image/jpeg");
        assert_eq!(
            sniff_media_type(&[0xff, 0x00, 0x12]),
            "application/octet-stream"
        );
    }

    #[test]
    fn only_small_editable_content_is_copied_to_inbox() {
        assert!(can_copy_to_inbox("text/plain; charset=utf-8", 128, 1024));
        assert!(!can_copy_to_inbox("application/octet-stream", 128, 1024));
        assert!(!can_copy_to_inbox("application/json", 2048, 1024));
    }
}
