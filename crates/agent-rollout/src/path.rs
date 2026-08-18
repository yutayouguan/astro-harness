use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

pub fn new_rollout_path(root: &Path, thread_id: &str, now: DateTime<Utc>) -> PathBuf {
    root.join(now.format("%Y").to_string())
        .join(now.format("%m").to_string())
        .join(now.format("%d").to_string())
        .join(format!(
            "rollout-{}-{thread_id}.jsonl",
            now.format("%Y%m%dT%H%M%S%.3fZ")
        ))
}

pub fn find_rollout(root: &Path, thread_id: &str) -> io::Result<Option<PathBuf>> {
    if !root.is_dir() {
        return Ok(None);
    }

    let suffix = format!("-{thread_id}.jsonl");
    let mut matches = Vec::new();
    for year in fs::read_dir(root)? {
        let year = year?;
        if !year.file_type()?.is_dir() {
            continue;
        }
        for month in fs::read_dir(year.path())? {
            let month = month?;
            if !month.file_type()?.is_dir() {
                continue;
            }
            for day in fs::read_dir(month.path())? {
                let day = day?;
                if !day.file_type()?.is_dir() {
                    continue;
                }
                for file in fs::read_dir(day.path())? {
                    let file = file?;
                    if !file.file_type()?.is_file() {
                        continue;
                    }
                    let name = file.file_name();
                    if name.to_string_lossy().ends_with(&suffix) {
                        matches.push(file.path());
                    }
                }
            }
        }
    }
    matches.sort();
    Ok(matches.pop())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use tempfile::TempDir;

    use super::{find_rollout, new_rollout_path};

    #[test]
    fn find_rollout_returns_the_latest_lexical_timestamp() {
        let temp = TempDir::new().unwrap();
        let first = new_rollout_path(
            temp.path(),
            "thread-1",
            chrono::Utc.with_ymd_and_hms(2026, 8, 17, 9, 0, 0).unwrap(),
        );
        let second = new_rollout_path(
            temp.path(),
            "thread-1",
            chrono::Utc.with_ymd_and_hms(2026, 8, 18, 9, 0, 0).unwrap(),
        );
        std::fs::create_dir_all(first.parent().unwrap()).unwrap();
        std::fs::write(&first, b"{}").unwrap();
        std::fs::create_dir_all(second.parent().unwrap()).unwrap();
        std::fs::write(&second, b"{}").unwrap();

        assert_eq!(find_rollout(temp.path(), "thread-1").unwrap(), Some(second));
    }
}
