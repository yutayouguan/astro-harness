// === Flat modules (unchanged) ===
pub(crate) mod auxiliary;
pub(crate) mod common;
pub(crate) mod environment_dependencies;
pub(crate) mod media;
pub(crate) mod terminal;

// === Directory modules ===
pub(crate) mod agents;
pub(crate) mod automation;
pub(crate) mod chat;
#[path = "evolution/mod.rs"]
mod evolution_impl;
pub(crate) mod extensions;
#[path = "memory/mod.rs"]
mod memory_impl;
pub(crate) mod providers;
pub(crate) mod ui;
pub(crate) mod workspace;

// === Re-exports: preserve the flat API surface ===
pub(crate) use chat::branches;
pub(crate) use chat::compaction;
pub(crate) use chat::session;

pub(crate) use providers::config;
pub(crate) use providers::model_catalog;
pub(crate) use providers::openrouter_rankings;

pub(crate) use memory_impl::compression_settings;
pub(crate) use memory_impl::core as memory;
pub(crate) use memory_impl::dreaming;

pub(crate) use evolution_impl::run as evolution_run;
pub(crate) use evolution_impl::settings as evolution;

pub(crate) use workspace::artifacts;
pub(crate) use workspace::files;

pub(crate) use agents::agent;
pub(crate) use agents::subagents;

pub(crate) use extensions::mcp_oauth;
pub(crate) use extensions::skills;

pub(crate) use automation::cron;
pub(crate) use automation::loops;

pub(crate) use ui::browser;
pub(crate) use ui::icon;
pub(crate) use ui::onboarding;
pub(crate) use ui::ui_style;
pub(crate) use ui::updater;
pub(crate) use ui::wallpaper;
