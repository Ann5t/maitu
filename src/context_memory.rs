use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    error::{AppError, AppResult},
    goal_domain::canonical_json_sha256,
};

pub const CONTEXT_GENERATOR: &str = "builtin-extractive-v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextBudgetPolicy {
    pub max_catalog_items: usize,
    pub max_summary_chars: usize,
    pub max_snippet_chars: usize,
    pub max_full_chars: usize,
    pub required_context_unabridged: bool,
}

impl Default for ContextBudgetPolicy {
    fn default() -> Self {
        Self {
            max_catalog_items: 12,
            max_summary_chars: 1_200,
            max_snippet_chars: 2_000,
            max_full_chars: 100_000,
            required_context_unabridged: true,
        }
    }
}

impl ContextBudgetPolicy {
    pub fn validate(self) -> AppResult<Self> {
        if !(1..=1_000).contains(&self.max_catalog_items)
            || !(128..=32_000).contains(&self.max_summary_chars)
            || !(128..=32_000).contains(&self.max_snippet_chars)
            || !(1_024..=1_000_000).contains(&self.max_full_chars)
            || !self.required_context_unabridged
        {
            return Err(AppError::bad_request(
                "invalid_context_budget",
                "上下文预算超出安全范围，且目标契约/硬约束/权限必须保持完整",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextReadLevel {
    Summary,
    Snippet,
    Full,
}

impl ContextReadLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Snippet => "snippet",
            Self::Full => "full",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedContext {
    pub summary: Value,
    pub fulltext_index: Value,
    pub retrieval_index: Value,
}

pub fn derive_context(text: &str, policy: &ContextBudgetPolicy) -> AppResult<DerivedContext> {
    let summary = truncate_chars(text.trim(), policy.max_summary_chars);
    let char_count = text.chars().count();
    let chunk_size = 1_000usize;
    let chunks = if char_count == 0 {
        Vec::new()
    } else {
        (0..char_count)
            .step_by(chunk_size)
            .map(|start| json!({ "startChar": start, "endChar": (start + chunk_size).min(char_count) }))
            .collect::<Vec<_>>()
    };
    let tokens = retrieval_tokens(text, 512);
    Ok(DerivedContext {
        summary: json!({
            "text": summary,
            "truncated": char_count > policy.max_summary_chars,
            "sourceChars": char_count,
        }),
        fulltext_index: json!({
            "sourceChars": char_count,
            "chunks": chunks,
            "rebuildable": true,
        }),
        retrieval_index: json!({
            "tokens": tokens,
            "rebuildable": true,
        }),
    })
}

pub fn context_snippet(text: &str, query: Option<&str>, max_chars: usize) -> String {
    let max_chars = max_chars.max(1);
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_owned();
    }
    let lowered = text.to_lowercase();
    let query = query.map(str::trim).filter(|value| !value.is_empty());
    let match_byte = query.and_then(|query| lowered.find(&query.to_lowercase()));
    let match_char = match_byte
        .map(|index| text[..index].chars().count())
        .unwrap_or(0);
    let half = max_chars / 2;
    let mut start = match_char.saturating_sub(half);
    if start + max_chars > total {
        start = total - max_chars;
    }
    let body = text.chars().skip(start).take(max_chars).collect::<String>();
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        body,
        if start + max_chars < total { "…" } else { "" }
    )
}

pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    text.chars().take(max_chars).collect()
}

pub fn derivation_hash(payload: &Value) -> AppResult<String> {
    canonical_json_sha256(payload)
}

fn retrieval_tokens(text: &str, limit: usize) -> Vec<String> {
    let normalized = text
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&character) {
                character.to_lowercase().collect::<String>()
            } else {
                " ".to_owned()
            }
        })
        .collect::<String>();
    let mut tokens = normalized
        .split_whitespace()
        .filter(|token| token.chars().count() >= 2)
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(limit)
        .collect::<Vec<_>>();
    tokens.sort();
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_envelope_cannot_be_budgeted_away() {
        let policy = ContextBudgetPolicy {
            required_context_unabridged: false,
            ..ContextBudgetPolicy::default()
        };
        assert!(policy.validate().is_err());
    }

    #[test]
    fn derivations_are_deterministic_and_rebuildable() {
        let policy = ContextBudgetPolicy::default();
        let first = derive_context("约束 alpha alpha；结论 beta", &policy).unwrap();
        let second = derive_context("约束 alpha alpha；结论 beta", &policy).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.retrieval_index["rebuildable"], true);
        assert_eq!(
            derivation_hash(&first.summary).unwrap(),
            derivation_hash(&second.summary).unwrap()
        );
    }

    #[test]
    fn snippet_is_unicode_safe_and_centers_a_match() {
        let value = context_snippet(
            "甲乙丙丁戊己庚辛关键约束壬癸子丑寅卯辰巳",
            Some("关键约束"),
            8,
        );
        assert!(value.contains("关键约束"));
        assert!(value.starts_with('…'));
        assert!(value.ends_with('…'));
    }

    #[test]
    fn invalid_budget_is_rejected() {
        let policy = ContextBudgetPolicy {
            max_catalog_items: 0,
            ..ContextBudgetPolicy::default()
        };
        assert!(policy.validate().is_err());
    }
}
