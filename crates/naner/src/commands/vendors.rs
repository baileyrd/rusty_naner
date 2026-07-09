//! Phase 2 stubs for the vendor commands. The real `install` /
//! `update-vendors` pipeline is Phase 3 (MIGRATION_ANALYSIS §6); until then
//! the Rust launcher is a drop-in for launching only, and vendors stay
//! managed by the C# naner.exe.

use naner_core::logger;

pub fn execute_install(_args: &[String]) -> i32 {
    logger::warning(
        "'install' is not implemented in the Rust port yet (Phase 3) - use the C# naner.exe for vendor management",
    );
    1
}

pub fn execute_update(_args: &[String]) -> i32 {
    logger::warning(
        "'update-vendors' is not implemented in the Rust port yet (Phase 3) - use the C# naner.exe for vendor management",
    );
    1
}
