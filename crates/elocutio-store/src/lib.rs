#![forbid(unsafe_code)]

//! Local persistence boundary.
//!
//! The desktop app owns the SQLite connection; this crate defines the schema
//! version and shared storage constants. Repositories and migrations arrive in
//! M1, once the desktop shell is forked from Harper.

/// Current schema version. Bump on every migration.
pub const SCHEMA_VERSION: u32 = 1;

/// Database filename under the app support directory.
pub const DB_FILENAME: &str = "elocutio.sqlite3";

/// Return the schema version expected by this build.
pub const fn schema_version() -> u32 {
    SCHEMA_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_version_is_positive() {
        assert!(schema_version() >= 1);
    }

    #[test]
    fn database_name_is_stable() {
        assert_eq!(DB_FILENAME, "elocutio.sqlite3");
    }
}
