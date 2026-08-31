use std::io;
use std::path::Path;

use tokio::io::{AsyncBufReadExt, BufReader};

use crate::{RolloutItem, RolloutLine};

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
    let mut reader = BufReader::new(file);
    let mut items = Vec::new();
    let mut parse_errors = 0;
    let mut record = Vec::new();

    while reader.read_until(b'\n', &mut record).await? != 0 {
        if record.last() == Some(&b'\n') {
            record.pop();
        }
        if record.last() == Some(&b'\r') {
            record.pop();
        }
        if record.iter().all(u8::is_ascii_whitespace) {
            record.clear();
            continue;
        }
        match serde_json::from_slice::<RolloutLine>(&record) {
            Ok(line) => items.push(line.item),
            Err(_) => parse_errors += 1,
        }
        record.clear();
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
                "{\"timestamp\":\"2026-08-31T00:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"index\":1}}\n",
                "not json\n",
                "{\"timestamp\":\"2026-08-31T00:00:01.000Z\",\"type\":\"session_meta\",\"payload\":{\"index\":2}}\n"
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
                "{\"timestamp\":\"2026-08-31T00:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"index\":1}}\n",
                "{\"timestamp\":\"2026-08-31T00:00:01.000Z\",\"type\":\"session_meta\",\"payload\":"
            ),
        )
        .unwrap();

        let rollout = read_rollout_with_diagnostics(&path).await.unwrap();
        assert_eq!(rollout.parse_errors, 1);
        assert_eq!(rollout.items.len(), 1);
    }

    #[tokio::test]
    async fn retains_valid_prefix_before_a_truncated_utf8_final_record() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("rollout.jsonl");
        let mut bytes = b"{\"timestamp\":\"2026-08-31T00:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"index\":1}}\n".to_vec();
        bytes.extend_from_slice(&[0xe4, 0xb8]);
        std::fs::write(&path, bytes).unwrap();

        let rollout = read_rollout_with_diagnostics(&path).await.unwrap();
        assert_eq!(rollout.parse_errors, 1);
        assert_eq!(rollout.items.len(), 1);
    }
}
