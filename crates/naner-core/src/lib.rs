//! Shared library for the naner launcher (`naner`) and bootstrapper
//! (`naner-init`).
//!
//! Phase status (MIGRATION_ANALYSIS §6): Phase 0 shipped the console spike
//! and scaffolding; Phase 1 adds the pure-logic foundations — `constants`,
//! `paths` (root discovery, expansion, PATH assembly), `config` (models,
//! JSON/YAML providers, env overrides, validator), `env_export`, `logger`,
//! and `version`. Phases 2–4 build the launcher, vendor pipeline, and init
//! on top.

pub mod config;
pub mod console;
pub mod constants;
pub mod env_export;
pub mod logger;
pub mod paths;
pub mod version;
