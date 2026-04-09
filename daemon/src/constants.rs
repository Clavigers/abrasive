pub const REQUIRED_ORG: &str = "Clavigers";
pub const USER_AGENT: &str = "abrasive-daemon";

/// Number of parallel workspace clones the daemon keeps per (team, scope).
/// Each slot has its own source tree + target/ so concurrent builds for
/// the same project don't clobber each other. A user is hashed to a
/// preferred slot for incremental cache affinity; on contention they
/// fall back to the first free slot.
pub const SLOTS_PER_SCOPE: usize = 4;
