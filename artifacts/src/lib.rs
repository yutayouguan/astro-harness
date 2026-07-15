//! 文件空间（Artifact）索引：`~/.astro/sessions/artifacts.db`。

pub mod db;

pub use db::{
    artifacts_db_path, category_from_name, is_junk_artifact_name, open_default, ArtifactDb,
    ArtifactRow, ArtifactSource, ReconcileReport,
};
