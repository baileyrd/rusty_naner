//! Shared library for the naner launcher (`naner`) and bootstrapper
//! (`naner-init`).
//!
//! Phase status (MIGRATION_ANALYSIS §6): Phase 0 shipped the console spike
//! and scaffolding; Phase 1 adds the pure-logic foundations — `constants`,
//! `paths` (root discovery, expansion, PATH assembly), `config` (models,
//! JSON/YAML providers, env overrides, validator), `env_export`, `logger`,
//! and `version`. Phases 2–4 build the launcher, vendor pipeline, and init
//! on top.

pub mod archives;
pub mod checksum;
pub mod collections;
pub mod config;
pub mod console;
pub mod constants;
pub mod digest;
pub mod env_export;
pub mod env_isolation;
mod fs_atomic;
pub mod github;
pub mod home_junctions;
pub mod http;
pub mod leak_reclaim;
pub mod lockfile;
pub mod logger;
pub mod paths;
pub mod regex_shim;
pub mod timestamp;
pub mod updater;
pub mod user_path;
pub mod vendors;
pub mod version;
