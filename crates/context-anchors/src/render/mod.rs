pub mod coverage;
pub mod human;
pub mod json;

#[cfg(test)]
mod tests;

/// Sites beyond this many are summarized as a count, in `check` groups and `coverage` groups
/// alike.
pub(crate) const MAX_LISTED_SITES: usize = 40;

/// The plural suffix for a count, so the five call sites that report one agree on the wording.
pub(crate) fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
