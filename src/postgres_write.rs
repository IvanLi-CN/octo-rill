/// Return whether an error description contains a PostgreSQL transaction state
/// that is safe to retry as a whole transaction.
pub fn contains_retryable_sqlstate(error: &str) -> bool {
    let error_lower = error.to_ascii_lowercase();
    (error_lower.contains("40001") || error_lower.contains("serialization failure"))
        || (error_lower.contains("40p01") || error_lower.contains("deadlock detected"))
}

#[cfg(test)]
mod tests {
    use super::contains_retryable_sqlstate;

    #[test]
    fn serialization_failure_and_deadlock_are_retryable() {
        assert!(contains_retryable_sqlstate("SQLSTATE: 40001"));
        assert!(contains_retryable_sqlstate("deadlock detected (40P01)"));
        assert!(!contains_retryable_sqlstate("SQLSTATE: 23505"));
    }
}
