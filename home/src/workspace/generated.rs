//! Agent 工作区 `generated/` 分类落盘约定。

use std::path::{Path, PathBuf};

/// 产物类型 → `generated/<dir_name>/`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedKind {
    Images,
    Videos,
    Audio,
    Code,
    Project,
    Docs,
    Html,
    Other,
}

impl GeneratedKind {
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Images => "images",
            Self::Videos => "videos",
            Self::Audio => "audio",
            Self::Code => "code",
            Self::Project => "project",
            Self::Docs => "docs",
            Self::Html => "html",
            Self::Other => "other",
        }
    }
}

/// 相对 Agent **工作区根** 的路径；`ensure_agent_space` 初始化创建。
pub const GENERATED_SUBDIRS: &[&str] = &[
    "generated/images",
    "generated/videos",
    "generated/audio",
    "generated/code",
    "generated/project",
    "generated/docs",
    "generated/html",
    "generated/other",
];

/// `{workspace}/generated/{kind}`（不 create；调用方 `create_dir_all`）。
pub fn generated_dir(workspace: &Path, kind: GeneratedKind) -> PathBuf {
    workspace.join("generated").join(kind.dir_name())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const ALL_KINDS: [GeneratedKind; 8] = [
        GeneratedKind::Images,
        GeneratedKind::Videos,
        GeneratedKind::Audio,
        GeneratedKind::Code,
        GeneratedKind::Project,
        GeneratedKind::Docs,
        GeneratedKind::Html,
        GeneratedKind::Other,
    ];

    #[test]
    fn generated_dir_joins_kind_subdir() {
        let ws = Path::new("/Users/a/.astro/workspace");
        for kind in ALL_KINDS {
            let expected = ws.join("generated").join(kind.dir_name());
            assert_eq!(generated_dir(ws, kind), expected);
        }
    }

    #[test]
    fn generated_subdirs_matches_kind_dir_names() {
        assert_eq!(GENERATED_SUBDIRS.len(), ALL_KINDS.len());
        for (kind, rel) in ALL_KINDS.iter().zip(GENERATED_SUBDIRS.iter()) {
            assert_eq!(*rel, format!("generated/{}", kind.dir_name()));
        }
    }
}
