//! Command: `naner checksum` — retired.
//!
//! It never computed or wrote anything, and what it was meant to do is now
//! covered properly: resolvers carry the digest the distributor publishes, and
//! `naner.lock` records the exact version, URL and SHA-256 of what was
//! installed. Rather than leave a command that reports success without acting,
//! it points at the one that does the job.

use naner_core::logger;

pub fn execute(_args: &[String]) -> i32 {
    logger::failure("`naner checksum` has been removed.");
    logger::newline();
    logger::info("Vendor artifacts are now verified automatically:");
    logger::info("  - resolvers use the digest the distributor publishes, where there is one");
    logger::info("  - `naner lock` shows the pinned version, URL and SHA-256 per vendor");
    logger::info("  - a `checksum` block in vendors.json still pins an artifact by hand");
    2
}
