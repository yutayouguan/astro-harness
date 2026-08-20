//! 跨 crate 共享类型：消息、工具描述、统一错误与 SQLite 打开协议。

pub mod approval;
pub mod auxiliary_target;
pub mod chat_target;
pub mod credentials;
pub mod error;
pub mod grpc_addr;
pub mod interaction_mode;
pub mod media;
pub mod message;
pub mod model_spec;
pub mod network_policy;
pub mod notify;
pub mod permissions;
pub mod sqlite;
pub mod text;
pub mod title;
pub mod tool;
pub mod tool_call;
pub mod tool_entry;
pub mod tool_output;
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
pub use network_policy::{
    NetworkApprovalContext, NetworkApprovalProtocol, NetworkDecisionSource, NetworkPolicyAmendment,
    NetworkPolicyDecision, NetworkPolicyDecisionPayload, NetworkPolicyRuleAction,
};
pub use notify::{
    dream_success_body, notify_important, notify_kind, set_important_notify_handler,
    set_notify_locale, truncate_notify, ImportantKind, ImportantNotice,
};
pub use permissions::{
    is_builtin_profile, ApprovalPolicy, ApprovalsReviewer, FilesystemAccess, FilesystemPolicy,
    GrantScope, NetworkAccess, NetworkPolicy, PermissionCapability, PermissionPreset,
    PermissionProfile, PermissionProfileError, PermissionReason, PermissionRequest,
    PermissionsConfig, SandboxMode, SessionPermissions, DANGER_FULL_ACCESS_PROFILE,
    READ_ONLY_PROFILE, WORKSPACE_PROFILE,
};
pub use sqlite::{delete_sqlite_files, open_wal, ExampleSqliteStore, SqliteStore};
pub use title::sanitize_title;
pub use tool_spill::{
    is_externalized_view, make_prune_view, make_spill_view, spill_path_for_prompt,
    write_tool_spill, DEFAULT_SPILL_THRESHOLD_BYTES, PRUNE_MIN_CHARS, TOOL_LLM_COMPRESS_MARK,
    TOOL_PRUNE_MARK, TOOL_SPILL_MARK,
};

pub use approval::{ApprovalAction, ApprovalDecision, ApprovalMode};
pub use credentials::{ImageGenCreds, ImageGenParts, ImageGenTargets, ModelCredentials};
pub use interaction_mode::InteractionMode;
pub use text::{truncate_chars, truncate_tool_result, truncate_utf8, MAX_TOOL_RESULT_BYTES};
pub use tool_call::{
    extract_tool_calls, resolve_tool_calls, ParsedToolCall, ToolCallAccumulator, ToolCallDelta,
};
pub use tool_entry::{
    McpToolAnnotations, McpToolApproval, McpToolApprovalMode, McpToolApprovalRoute,
    SandboxablePreference, ToolEntry,
};
pub use tool_output::ToolOutput;
