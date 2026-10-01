use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckCommand {
    pub id: String,
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
}

impl CheckCommand {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_empty()
            || self.id.len() > 40
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            || self.label.trim().is_empty()
            || self.label.len() > 400
        {
            return Err("检查方式需要有效的标识与名称");
        }
        if !matches!(
            self.program.as_str(),
            "node" | "python3" | "cargo" | "rustc" | "npm"
        ) {
            return Err("当前检查环境支持 Node、Python 与 Rust 的项目命令");
        }
        if self.args.is_empty()
            || self.args.len() > 32
            || self
                .args
                .iter()
                .any(|arg| arg.len() > 4000 || arg.contains('\0'))
        {
            return Err("检查参数不正确");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckRequest {
    pub request_id: Uuid,
    pub workspace_key: Uuid,
    pub command: CheckCommand,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub request_id: Uuid,
    pub command: CheckCommand,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub duration_ms: u64,
    pub runtime_image: String,
    pub error: Option<String>,
}

impl CheckResult {
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0) && self.error.is_none()
    }
}
