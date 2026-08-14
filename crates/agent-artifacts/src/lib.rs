//! 文件空间（Artifact）索引：`~/.astro/sessions/artifacts.db`。
//! Knowledge Content DB：`~/.astro/sessions/knowledge.db`（FTS，无向量）。

pub mod content_db;
pub mod db;

pub use content_db::{ContentRow, KnowledgeDb};
pub use db::{
    artifacts_db_path, category_from_name, is_junk_artifact_name, open_default, ArtifactDb,
    ArtifactRow, ArtifactSource, ReconcileReport,
};
