use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use sha2::{Digest, Sha256};
use tokio::fs;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Clone, Debug)]
pub struct ArtifactStore {
    root: Arc<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct StoredArtifact {
    pub storage_path: String,
    pub sha256: String,
}

impl ArtifactStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: Arc::new(root),
        }
    }

    pub async fn write_text(
        &self,
        project_id: Uuid,
        artifact_id: Uuid,
        filename: &str,
        content: &str,
    ) -> AppResult<StoredArtifact> {
        if filename.is_empty()
            || Path::new(filename)
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(AppError::bad_request(
                "invalid_artifact_name",
                "产物文件名不合法",
            ));
        }

        let storage_path = format!("{project_id}/{artifact_id}/{filename}");
        let target = self.resolve(&storage_path)?;
        let parent = target
            .parent()
            .ok_or_else(|| AppError::internal("无法确定产物目录"))?;
        fs::create_dir_all(parent).await?;

        let temporary = target.with_extension(format!("{}.tmp", Uuid::new_v4()));
        fs::write(&temporary, content.as_bytes()).await?;
        if let Err(error) = fs::rename(&temporary, &target).await {
            let _ = fs::remove_file(&temporary).await;
            return Err(error.into());
        }

        Ok(StoredArtifact {
            storage_path,
            sha256: hex::encode(Sha256::digest(content.as_bytes())),
        })
    }

    pub async fn read(&self, storage_path: &str) -> AppResult<Vec<u8>> {
        Ok(fs::read(self.resolve(storage_path)?).await?)
    }

    fn resolve(&self, storage_path: &str) -> AppResult<PathBuf> {
        let relative = Path::new(storage_path);
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(AppError::bad_request(
                "invalid_artifact_path",
                "产物路径不合法",
            ));
        }
        Ok(self.root.join(relative))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn rejects_paths_that_escape_the_artifact_root() {
        let store = ArtifactStore::new(PathBuf::from("/tmp/fudian-artifacts"));
        assert!(store.resolve("../secret").is_err());
        assert!(store.resolve("/etc/passwd").is_err());
        assert!(store.resolve("project/artifact/file.md").is_ok());
    }
}
