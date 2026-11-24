//! Core library of the Perdura network.

/// Returns the version of this crate.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    /// Smoke test: proves the test harness runs.
    #[test]
    fn version_is_not_empty() {
        assert!(!version().is_empty());
    }
}
