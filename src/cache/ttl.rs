/// Standard TTL for user/profile/tweet data.
pub const DEFAULT: u64 = 5 * 60; // 5 minutes

/// Serve a stale profile this long past [`DEFAULT`] while one refresh runs.
pub const DEFAULT_STALE: u64 = 20 * 60;

/// First-page search, short so results stay live but a repeat skips X.
pub const SEARCH: u64 = 60;

/// Serve a stale first search page this long past [`SEARCH`].
pub const SEARCH_STALE: u64 = 4 * 60;

/// Long TTL for immutable mappings like user ID -> username.
pub const USER_ID_MAPPING: u64 = 24 * 60 * 60; // 1 day

/// Translations are immutable for a tweet revision.
pub const TRANSLATION: u64 = 24 * 60 * 60; // 1 day

pub const ACCOUNT_CONTEXT: u64 = 60 * 24 * 60 * 60; // 60 days
