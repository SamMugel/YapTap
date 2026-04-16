//! macOS Keychain helpers for storing the CompactifAI API key.

use anyhow::Result;
use security_framework::passwords::{get_generic_password, set_generic_password};

const SERVICE: &str = "yaptap";
const ACCOUNT: &str = "MULTIVERSE_IAM_API_KEY";

/// Reads `MULTIVERSE_IAM_API_KEY` from the macOS Keychain.
///
/// Service: `"yaptap"`, Account: `"MULTIVERSE_IAM_API_KEY"`.
/// Returns `None` if the item does not exist.
pub(crate) fn keychain_get_api_key() -> Option<String> {
    match get_generic_password(SERVICE, ACCOUNT) {
        Ok(bytes) => String::from_utf8(bytes).ok(),
        Err(_) => None,
    }
}

/// Writes `MULTIVERSE_IAM_API_KEY` to the macOS Keychain.
///
/// Creates the item if absent; updates it if already present.
///
/// # Errors
/// Returns an error if the Keychain operation fails (e.g. user denied access,
/// Keychain locked, or `security-framework` internal error).
pub(crate) fn keychain_set_api_key(key: &str) -> Result<()> {
    set_generic_password(SERVICE, ACCOUNT, key.as_bytes())
        .map_err(|e| anyhow::anyhow!("Keychain write failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires real macOS Keychain access — cannot run in CI"]
    fn round_trip_api_key() {
        let test_key = "test-keychain-round-trip-value";
        keychain_set_api_key(test_key).expect("set should succeed");
        let retrieved = keychain_get_api_key().expect("get should return Some");
        assert_eq!(retrieved, test_key);
    }
}
