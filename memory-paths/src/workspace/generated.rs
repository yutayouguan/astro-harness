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

    #[test]
    fn generated_dir_joins_kind_subdir() {
        let ws = Path::new("/Users/a/.astro/workspace");
        assert_eq!(
            generated_dir(ws, GeneratedKind::Images),
            Path::new("/Users/a/.astro/workspace/generated/images")
        );
        assert_eq!(
            generated_dir(ws, GeneratedKind::Videos),
            Path::new("/Users/a/.astro/workspace/generated/videos")
        );
        assert_eq!(
            generated_dir(ws, GeneratedKind::Audio),
            Path::new("/Users/a/.astro/workspace/generated/audio")
        );
    }

    #[test]
    fn generated_subdirs_lists_all_seed_folders() {
        let expected = [
            "generated/images",
            "generated/videos",
            "generated/audio",
            "generated/code",
            "generated/project",
            "generated/docs",
            "generated/html",
            "generated/other",
        ];
        assert_eq!(GENERATED_SUBDIRS, &expected);
    }
}
