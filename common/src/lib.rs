//! 跨 crate 共享类型：消息、工具描述、统一错误与 SQLite 打开协议。

pub mod auxiliary_target;
pub mod chat_target;
pub mod error;
pub mod grpc_addr;
pub mod media;
pub mod message;
pub mod model_spec;
pub mod notify;
pub mod sqlite;
pub mod text;
pub mod title;
pub mod tool;
pub mod tool_spill;

pub use auxiliary_target::{AuxiliaryTargetChain, AuxiliaryTask};
pub use chat_target::*;
pub use grpc_addr::{
    grpc_bind_address, resolve_grpc_address, runtime_grpc_address, set_runtime_grpc_address,
};
pub use media::{
    append_media_sidecar, extract_tool_media, parse_generated_labels, MediaAsset, MediaKind,
    MediaRef,
};
pub use model_spec::{ModelRole, ModelSpec};
pub use notify::{
    dream_success_body, notify_important, notify_kind, set_important_notify_handler,
    set_notify_locale, truncate_notify, ImportantKind, ImportantNotice,
};
pub use sqlite::{delete_sqlite_files, open_wal, ExampleSqliteStore, SqliteStore};
pub use title::sanitize_title;
pub use tool_spill::{
    is_externalized_view, make_prune_view, make_spill_view, spill_path_for_prompt,
    write_tool_spill, DEFAULT_SPILL_THRESHOLD_BYTES, PRUNE_MIN_CHARS, TOOL_LLM_COMPRESS_MARK,
    TOOL_PRUNE_MARK, TOOL_SPILL_MARK,
};

pub use text::{truncate_chars, truncate_tool_result, truncate_utf8, MAX_TOOL_RESULT_BYTES};
