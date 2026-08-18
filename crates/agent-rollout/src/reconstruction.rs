use std::io;
use std::path::Path;

use tokio::io::{AsyncBufReadExt, BufReader};

use crate::RolloutItem;

#[derive(Debug, Clone, PartialEq)]
pub struct RolloutRead {
    pub items: Vec<RolloutItem>,
    pub parse_errors: usize,
}

pub async fn read_rollout(path: &Path) -> io::Result<Vec<RolloutItem>> {
    Ok(read_rollout_with_diagnostics(path).await?.items)
}

pub async fn read_rollout_with_diagnostics(path: &Path) -> io::Result<RolloutRead> {
    let file = tokio::fs::File::open(path).await?;
    let mut lines = BufReader::new(file).lines();
    let mut items = Vec::new();
    let mut parse_errors = 0;

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(&line) {
            Ok(item) => items.push(item),
            Err(_) => parse_errors += 1,
        }
    }

    Ok(RolloutRead {
        items,
        parse_errors,
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::read_rollout_with_diagnostics;
    use crate::RolloutItem;

    #[tokio::test]
    async fn retains_valid_items_across_a_malformed_middle_line() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"data\":{\"index\":1}}\n",
                "not json\n",
                "{\"type\":\"session_meta\",\"data\":{\"index\":2}}\n"
            ),
        )
        .unwrap();

        let rollout = read_rollout_with_diagnostics(&path).await.unwrap();
        assert_eq!(rollout.parse_errors, 1);
        assert_eq!(rollout.items.len(), 2);
        assert!(matches!(rollout.items[0], RolloutItem::SessionMeta(_)));
        assert!(matches!(rollout.items[1], RolloutItem::SessionMeta(_)));
    }

    #[tokio::test]
    async fn retains_valid_prefix_before_a_truncated_final_line() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"data\":{\"index\":1}}\n",
                "{\"type\":\"session_meta\",\"data\":"
            ),
        )
        .unwrap();

        let rollout = read_rollout_with_diagnostics(&path).await.unwrap();
        assert_eq!(rollout.parse_errors, 1);
        assert_eq!(rollout.items.len(), 1);
    }
}
