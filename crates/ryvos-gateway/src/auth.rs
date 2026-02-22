use ryvos_core::config::{ApiKeyRole, GatewayConfig};

/// Result of a successful authentication.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AuthResult {
    pub name: String,
    pub role: ApiKeyRole,
}

/// Full validation: Bearer header -> api_keys -> legacy token -> legacy password -> anonymous.
///
/// Returns `Some(AuthResult)` on success, `None` on auth failure.
pub fn validate_auth(
    config: &GatewayConfig,
    bearer: Option<&str>,
    query_token: Option<&str>,
    query_password: Option<&str>,
) -> Option<AuthResult> {
    // 1. Check bearer against api_keys, then legacy token
    if let Some(bearer_val) = bearer {
        for ak in &config.api_keys {
            if ak.key == bearer_val {
                return Some(AuthResult {
                    name: ak.name.clone(),
                    role: ak.role.clone(),
                });
            }
        }
        // Check legacy token
        if config.token.as_deref() == Some(bearer_val) {
            return Some(AuthResult {
                name: "legacy-token".into(),
                role: ApiKeyRole::Admin,
            });
        }
        return None; // Bearer provided but no match
    }

    // 2. Legacy query-string auth
    if let Some(expected) = &config.token {
        if query_token == Some(expected.as_str()) {
            return Some(AuthResult {
                name: "legacy-token".into(),
                role: ApiKeyRole::Admin,
            });
        }
        return None;
    }
    if let Some(expected) = &config.password {
        if query_password == Some(expected.as_str()) {
            return Some(AuthResult {
                name: "legacy-password".into(),
                role: ApiKeyRole::Admin,
            });
        }
        return None;
    }

    // 3. No auth configured = anonymous access (only if no api_keys either)
    if config.api_keys.is_empty() {
        Some(AuthResult {
            name: "anonymous".into(),
            role: ApiKeyRole::Admin,
        })
    } else {
        None
    }
}

/// Extract token from the query string (?token=...).
pub fn extract_token_from_query(query: &str) -> Option<&str> {
    for pair in query.split('&') {
        if let Some(val) = pair.strip_prefix("token=") {
            return Some(val);
        }
    }
    None
}

/// Extract password from the query string (?password=...).
pub fn extract_password_from_query(query: &str) -> Option<&str> {
    for pair in query.split('&') {
        if let Some(val) = pair.strip_prefix("password=") {
            return Some(val);
        }
    }
    None
}

/// Check if a role has at least viewer-level access.
pub fn has_viewer_access(role: &ApiKeyRole) -> bool {
    matches!(role, ApiKeyRole::Viewer | ApiKeyRole::Operator | ApiKeyRole::Admin)
}

/// Check if a role has at least operator-level access.
pub fn has_operator_access(role: &ApiKeyRole) -> bool {
    matches!(role, ApiKeyRole::Operator | ApiKeyRole::Admin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryvos_core::config::ApiKeyConfig;

    fn gateway(
        token: Option<&str>,
        password: Option<&str>,
        api_keys: Vec<ApiKeyConfig>,
    ) -> GatewayConfig {
        GatewayConfig {
            bind: "127.0.0.1:18789".to_string(),
            token: token.map(|s| s.to_string()),
            password: password.map(|s| s.to_string()),
            api_keys,
        }
    }

    #[test]
    fn test_no_auth_always_passes() {
        let config = gateway(None, None, vec![]);
        assert!(validate_auth(&config, None, None, None).is_some());
        assert!(validate_auth(&config, Some("anything"), None, None).is_none()); // Bearer with no match
        assert!(validate_auth(&config, None, Some("anything"), None).is_some()); // No token configured
    }

    #[test]
    fn test_token_auth() {
        let config = gateway(Some("secret"), None, vec![]);
        assert!(validate_auth(&config, None, None, None).is_none());
        assert!(validate_auth(&config, None, Some("wrong"), None).is_none());
        assert!(validate_auth(&config, None, Some("secret"), None).is_some());
        // Password ignored when token is configured
        assert!(validate_auth(&config, None, None, Some("secret")).is_none());
    }

    #[test]
    fn test_password_auth() {
        let config = gateway(None, Some("pass123"), vec![]);
        assert!(validate_auth(&config, None, None, None).is_none());
        assert!(validate_auth(&config, None, None, Some("wrong")).is_none());
        assert!(validate_auth(&config, None, None, Some("pass123")).is_some());
        // Token param is irrelevant when only password is configured
        assert!(validate_auth(&config, None, Some("pass123"), None).is_none());
    }

    #[test]
    fn test_token_takes_precedence_over_password() {
        let config = gateway(Some("tok"), Some("pass"), vec![]);
        assert!(validate_auth(&config, None, Some("tok"), None).is_some());
        assert!(validate_auth(&config, None, None, Some("pass")).is_none());
        assert!(validate_auth(&config, None, Some("pass"), Some("tok")).is_none());
    }

    #[test]
    fn test_bearer_api_key() {
        let keys = vec![ApiKeyConfig {
            name: "web-ui".to_string(),
            key: "rk_test123".to_string(),
            role: ApiKeyRole::Operator,
        }];
        let config = gateway(None, None, keys);

        // Bearer matches api_key
        let result = validate_auth(&config, Some("rk_test123"), None, None);
        assert!(result.is_some());
        let auth = result.unwrap();
        assert_eq!(auth.name, "web-ui");
        assert_eq!(auth.role, ApiKeyRole::Operator);

        // Wrong bearer
        assert!(validate_auth(&config, Some("wrong"), None, None).is_none());

        // No auth at all => denied (api_keys configured)
        assert!(validate_auth(&config, None, None, None).is_none());
    }

    #[test]
    fn test_bearer_falls_back_to_legacy_token() {
        let config = gateway(Some("legacy-tok"), None, vec![]);
        let result = validate_auth(&config, Some("legacy-tok"), None, None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "legacy-token");
    }

    #[test]
    fn test_api_key_roles() {
        let keys = vec![
            ApiKeyConfig {
                name: "viewer".to_string(),
                key: "rk_view".to_string(),
                role: ApiKeyRole::Viewer,
            },
            ApiKeyConfig {
                name: "admin".to_string(),
                key: "rk_admin".to_string(),
                role: ApiKeyRole::Admin,
            },
        ];
        let config = gateway(None, None, keys);

        let viewer = validate_auth(&config, Some("rk_view"), None, None).unwrap();
        assert_eq!(viewer.role, ApiKeyRole::Viewer);
        assert!(has_viewer_access(&viewer.role));
        assert!(!has_operator_access(&viewer.role));

        let admin = validate_auth(&config, Some("rk_admin"), None, None).unwrap();
        assert_eq!(admin.role, ApiKeyRole::Admin);
        assert!(has_operator_access(&admin.role));
    }

    #[test]
    fn test_extract_token() {
        assert_eq!(extract_token_from_query("token=abc"), Some("abc"));
        assert_eq!(extract_token_from_query("foo=bar&token=abc"), Some("abc"));
        assert_eq!(extract_token_from_query("foo=bar"), None);
    }

    #[test]
    fn test_extract_password() {
        assert_eq!(extract_password_from_query("password=abc"), Some("abc"));
        assert_eq!(
            extract_password_from_query("foo=bar&password=abc"),
            Some("abc")
        );
        assert_eq!(extract_password_from_query("foo=bar"), None);
    }
}
