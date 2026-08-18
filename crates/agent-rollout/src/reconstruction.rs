use std::io;
use std::path::Path;

use tokio::io::{AsyncBufReadExt, BufReader};

use crate::RolloutItem;

pub async fn read_rollout(path: &Path) -> io::Result<Vec<RolloutItem>> {
    let file = tokio::fs::File::open(path).await?;
    let mut lines = BufReader::new(file).lines();
    let mut items = Vec::new();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let item = serde_json::from_str(&line).map_err(io::Error::other)?;
        items.push(item);
    }

    Ok(items)
}
