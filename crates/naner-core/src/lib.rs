//! Shared library for the naner launcher (`naner`) and bootstrapper
//! (`naner-init`).
//!
//! Phase 0 (MIGRATION_ANALYSIS §6) ships only the console subsystem spike —
//! the highest-fidelity-risk piece of the port — plus the workspace/CI
//! scaffolding around it. The Phase 1 modules (`constants`, `paths`, `config`,
//! `env_export`, `logger`, `version`) land next.

pub mod console;
