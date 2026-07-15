/// Standard TTL for user/profile/tweet data.
pub const DEFAULT: u64 = 5 * 60;

/// Long TTL for immutable mappings like user ID -> username.
pub const USER_ID_MAPPING: u64 = 24 * 60 * 60;

/// Translations are immutable for a tweet revision.
pub const TRANSLATION: u64 = 24 * 60 * 60;
