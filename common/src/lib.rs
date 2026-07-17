//! 跨 crate 共享类型：消息、工具描述、统一错误与 SQLite 打开协议。

pub mod auxiliary_target;
pub mod chat_target;
pub mod error;
pub mod media;
pub mod message;
pub mod notify;
pub mod sqlite;
pub mod text;
pub mod title;
pub mod tool;

pub use auxiliary_target::{AuxiliaryTargetChain, AuxiliaryTask};
pub use chat_target::*;
pub use media::{
    append_media_sidecar, extract_tool_media, parse_generated_labels, MediaAsset, MediaKind,
    MediaRef,
};
pub use notify::{notify_important, set_important_notify_handler, ImportantNotice};
pub use sqlite::{delete_sqlite_files, open_wal, ExampleSqliteStore, SqliteStore};
pub use title::sanitize_title;

pub use text::{truncate_chars, truncate_tool_result, truncate_utf8, MAX_TOOL_RESULT_BYTES};
