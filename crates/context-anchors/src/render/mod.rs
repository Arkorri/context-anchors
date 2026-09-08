pub mod coverage;
pub mod human;
pub mod json;

/// Sites beyond this many are summarized as a count, in `check` groups and `coverage` groups
/// alike.
pub(crate) const MAX_LISTED_SITES: usize = 40;
