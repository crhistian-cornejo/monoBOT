//! UI-independent services shared by the native migration and Tauri.
pub mod accounts;
pub mod agent;
pub mod app_access;
pub mod app_update;
pub mod automations;
pub mod catalog;
pub mod checkpoint;
pub mod citations;
pub mod connectors;
pub mod context;
pub mod desktop_preferences;
pub mod dictation;
pub mod diff;
pub mod external_editor;
pub mod files;
pub mod follow_up;
pub mod git;
pub mod github_star;
pub mod harness_updates;
pub mod images;
pub mod inbox;
pub mod keep_awake;
pub mod mobile;
pub mod mono;
pub mod notes;
pub mod office;
pub mod poki;
pub mod process;
pub mod project_appearance;
pub mod provider;
pub mod provider_retry;
pub mod remote;
pub mod research;
pub mod resources;
pub mod response;
pub mod scheduled_send;
pub mod scm;
pub mod session_filters;
pub mod session_groups;
pub mod session_title;
pub mod store;
pub mod time;
pub mod tool;
pub mod transcript;
pub mod usage;
pub mod user_links;
pub mod visual;
pub mod visual_bridge;
pub mod web;

pub mod composer;
pub mod composer_quotes;
pub mod contributions;
pub mod orchestration;
pub mod papers;
pub mod plan;
pub mod skills;
pub mod sounds;

pub mod search;

pub mod cli;
pub mod mcp;

pub mod worktree_management;

#[cfg(test)]
mod ts_literal;
