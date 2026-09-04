use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDateTime, Utc};

const ROLLOUT_PREFIX: &str = "rollout-";
const TIMESTAMP_LEN: usize = 20;
const ROLLOUT_SUFFIX: &str = ".jsonl";

pub fn new_rollout_path(root: &Path, thread_id: &str, now: DateTime<Utc>) -> PathBuf {
    root.join(now.format("%Y").to_string())
        .join(now.format("%m").to_string())
        .join(now.format("%d").to_string())
        .join(format!(
            "{ROLLOUT_PREFIX}{}-{}.jsonl",
            now.format("%Y%m%dT%H%M%S%.3fZ"),
            encode_thread_id(thread_id)
        ))
}

pub fn find_rollout(root: &Path, thread_id: &str) -> io::Result<Option<PathBuf>> {
    Ok(find_rollouts(root, &[thread_id.to_string()])?.remove(thread_id))
}

/// Finds the latest rollout for each requested thread while scanning the dated tree once.
pub fn find_rollouts(root: &Path, thread_ids: &[String]) -> io::Result<HashMap<String, PathBuf>> {
    if !root.is_dir() {
        return Ok(HashMap::new());
    }

    let requested = thread_ids
        .iter()
        .map(|thread_id| (encode_thread_id(thread_id), thread_id.clone()))
        .collect::<HashMap<_, _>>();
    let mut matches = HashMap::<String, PathBuf>::new();
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
                    if let Some(thread_id) = name
                        .to_str()
                        .and_then(encoded_thread_id_from_file_name)
                        .and_then(|encoded| requested.get(encoded))
                    {
                        let path = file.path();
                        if matches.get(thread_id).is_none_or(|current| path > *current) {
                            matches.insert(thread_id.clone(), path);
                        }
                    }
                }
            }
        }
    }
    Ok(matches)
}

fn encode_thread_id(thread_id: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut encoded = String::with_capacity(thread_id.len());
    for byte in thread_id.bytes() {
        if is_safe_literal(byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[(byte >> 4) as usize]));
            encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
        }
    }
    encoded
}

fn encoded_thread_id_from_file_name(file_name: &str) -> Option<&str> {
    let remainder = file_name.strip_prefix(ROLLOUT_PREFIX)?;
    let timestamp = remainder.as_bytes().get(..TIMESTAMP_LEN)?;
    let timestamp = std::str::from_utf8(timestamp).ok()?;
    NaiveDateTime::parse_from_str(timestamp, "%Y%m%dT%H%M%S%.3fZ").ok()?;
    let encoded_thread_id = remainder
        .get(TIMESTAMP_LEN..)?
        .strip_prefix('-')?
        .strip_suffix(ROLLOUT_SUFFIX)?;
    is_encoded_thread_id(encoded_thread_id).then_some(encoded_thread_id)
}

fn is_encoded_thread_id(encoded: &str) -> bool {
    let bytes = encoded.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            byte if is_safe_literal(byte) => index += 1,
            b'%' if index + 2 < bytes.len()
                && is_upper_hex(bytes[index + 1])
                && is_upper_hex(bytes[index + 2]) =>
            {
                index += 3;
            }
            _ => return false,
        }
    }
    true
}

fn is_safe_literal(byte: u8) -> bool {
    byte.is_ascii_digit() || byte.is_ascii_lowercase() || matches!(byte, b'-' | b'_')
}

fn is_upper_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use tempfile::TempDir;

    use super::{find_rollout, find_rollouts, new_rollout_path};

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

    #[test]
    fn find_rollouts_scans_multiple_threads_and_keeps_latest_paths() {
        let temp = TempDir::new().unwrap();
        let first = new_rollout_path(
            temp.path(),
            "thread-1",
            chrono::Utc.with_ymd_and_hms(2026, 8, 17, 9, 0, 0).unwrap(),
        );
        let latest = new_rollout_path(
            temp.path(),
            "thread-1",
            chrono::Utc.with_ymd_and_hms(2026, 8, 18, 9, 0, 0).unwrap(),
        );
        let other = new_rollout_path(
            temp.path(),
            "thread-2",
            chrono::Utc.with_ymd_and_hms(2026, 8, 18, 10, 0, 0).unwrap(),
        );
        for path in [&first, &latest, &other] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"{}").unwrap();
        }

        let found = find_rollouts(
            temp.path(),
            &["thread-1".into(), "thread-2".into(), "missing".into()],
        )
        .unwrap();

        assert_eq!(found.get("thread-1"), Some(&latest));
        assert_eq!(found.get("thread-2"), Some(&other));
        assert!(!found.contains_key("missing"));
    }

    #[test]
    fn unsafe_thread_id_stays_in_its_dated_directory() {
        let temp = TempDir::new().unwrap();
        let now = chrono::Utc.with_ymd_and_hms(2026, 8, 18, 9, 0, 0).unwrap();
        let path = new_rollout_path(temp.path(), "../../outside", now);
        let dated_directory = temp.path().join("2026").join("08").join("18");

        assert_eq!(path.parent(), Some(dated_directory.as_path()));
        assert!(!path.file_name().unwrap().to_string_lossy().contains('/'));
        assert!(!path.file_name().unwrap().to_string_lossy().contains('\\'));
    }

    #[test]
    fn find_rollout_requires_the_entire_encoded_thread_id_to_match() {
        let temp = TempDir::new().unwrap();
        let path = new_rollout_path(
            temp.path(),
            "thread-1",
            chrono::Utc.with_ymd_and_hms(2026, 8, 18, 9, 0, 0).unwrap(),
        );
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{}").unwrap();

        assert_eq!(find_rollout(temp.path(), "1").unwrap(), None);
    }

    #[test]
    fn case_distinct_thread_ids_have_case_insensitive_distinct_paths_and_lookups() {
        let temp = TempDir::new().unwrap();
        let now = chrono::Utc.with_ymd_and_hms(2026, 8, 18, 9, 0, 0).unwrap();
        let uppercase = new_rollout_path(temp.path(), "Thread-A", now);
        let lowercase = new_rollout_path(temp.path(), "thread-a", now);

        assert_ne!(
            uppercase.to_string_lossy().to_ascii_lowercase(),
            lowercase.to_string_lossy().to_ascii_lowercase()
        );
        std::fs::create_dir_all(uppercase.parent().unwrap()).unwrap();
        std::fs::write(&uppercase, b"{}").unwrap();
        std::fs::write(&lowercase, b"{}").unwrap();

        assert_eq!(
            find_rollout(temp.path(), "Thread-A").unwrap(),
            Some(uppercase)
        );
        assert_eq!(
            find_rollout(temp.path(), "thread-a").unwrap(),
            Some(lowercase)
        );
    }
}
