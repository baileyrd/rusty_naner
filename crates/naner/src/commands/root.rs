//! `naner root` — additive composable primitive (MIGRATION_ANALYSIS §2.4
//! tier 2): the discovered NANER_ROOT on pure stdout, errors on stderr,
//! nothing else. Enables `cd $(naner root)`.

use naner_core::{constants, paths};

pub fn execute() -> i32 {
    match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(root) => {
            println!("{}", root.display());
            0
        }
        Err(err) => {
            eprintln!("{}", err.message);
            1
        }
    }
}
