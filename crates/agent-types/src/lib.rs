//! 跨 crate 共享类型：消息、工具描述、统一错误与 SQLite 打开协议。

pub mod approval;
pub mod async_user_input;
pub mod auxiliary_target;
pub mod compact_scope;
pub mod credentials;
pub mod desktop_pet;
pub mod error;
pub mod grpc_addr;
pub mod interaction_mode;
pub mod media;
pub mod memory_citation;
pub mod model_profile;
pub mod model_spec;
pub mod model_target;
pub mod model_tool;
pub mod network_policy;
pub mod notify;
pub mod permissions;
pub mod sqlite;
pub mod text;
pub mod thread_memory_mode;
pub mod title;
pub mod tool;
pub mod tool_call;
pub mod tool_entry;
pub mod tool_mode;
pub mod tool_output;
pub mod tool_spill;
pub mod ui_style;

pub use auxiliary_target::{AuxiliaryTargetChain, AuxiliaryTask};
pub use compact_scope::CompactTokenLimitScope;
pub use grpc_addr::{
    grpc_bind_address, resolve_grpc_address, runtime_grpc_address, set_runtime_grpc_address,
};
pub use media::{
    append_media_sidecar, extract_tool_media, parse_generated_labels, MediaAsset, MediaKind,
    MediaRef,
};
pub use model_spec::{ModelRole, ModelSpec};
pub use model_target::*;
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
    GrantScope, NetworkAccess, NetworkHeaderInjection, NetworkPolicy, PermissionCapability,
    PermissionPreset, PermissionProfile, PermissionProfileError, PermissionReason,
    PermissionRequest, PermissionsConfig, SandboxMode, SessionPermissions,
    DANGER_FULL_ACCESS_PROFILE, READ_ONLY_PROFILE, WORKSPACE_PROFILE,
};
pub use sqlite::{AstroDb, DbSpec, SqlitePool, SqliteStore};
pub use title::sanitize_title;
pub use tool_spill::{
    is_externalized_view, make_prune_view, make_spill_view, spill_path_for_prompt,
    write_tool_spill, write_tool_spill_with_key, DEFAULT_SPILL_THRESHOLD_BYTES, PRUNE_MIN_CHARS,
    TOOL_LLM_COMPRESS_MARK, TOOL_PRUNE_MARK, TOOL_SPILL_MARK,
};
pub use ui_style::{
    active_ui_style_path, notify_ui_style_changed, read_active_ui_style,
    set_ui_style_change_handler, ui_style_root, UiStyleIconMotion, UiStyleIcons, UiStyleManifest,
    UiStyleTokens, UiStyleWallpaper, UiStyleWallpaperFit, UI_STYLE_SCHEMA_VERSION,
};

pub use approval::{ApprovalAction, ApprovalDecision, ApprovalMode};
pub use async_user_input::AsyncUserInputQuestion;
pub use credentials::{ImageGenCreds, ImageGenParts, ImageGenTargets, ModelCredentials};
pub use desktop_pet::{
    desktop_pet_root, desktop_pet_state_path, notify_desktop_pet_changed, read_desktop_pet_state,
    set_desktop_pet_change_handler, update_desktop_pet_state, write_desktop_pet_state,
    DesktopPetState,
};
pub use interaction_mode::InteractionMode;
pub use memory_citation::MemoryCitation;
pub use model_profile::{
    ApplyPatchToolType, ModelInputModality, ModelMultiAgentVersion, ModelProfile, ModelVerbosity,
    WebSearchToolType,
};
pub use text::{truncate_chars, truncate_tool_result, truncate_utf8, MAX_TOOL_RESULT_BYTES};
pub use thread_memory_mode::ThreadMemoryMode;
pub use tool_call::{ParsedToolCall, ToolCallAccumulator, ToolCallDelta};
pub use tool_entry::{
    ExecApprovalRequirement, FreeformToolFormat, McpToolAnnotations, McpToolApproval,
    McpToolApprovalMode, McpToolApprovalRoute, NamespacedToolDef, SandboxablePreference, ToolEntry,
    ToolExposure, ToolName, ToolSpec,
};
pub use tool_mode::{deserialize_optional_tool_mode, ToolMode, ToolModeFeatureFlags};
pub use tool_output::{ToolFileChange, ToolFileChangeKind, ToolOutput};
