use serde::Deserialize;

/// OAuth 2.0 provider configuration.
#[derive(Debug, Clone)]
pub struct OAuthProviderConfig {
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub token_type: Option<String>,
    pub scope: Option<String>,
}

/// Generate the OAuth authorization URL for a provider.
pub fn generate_auth_url(config: &OAuthProviderConfig, redirect_uri: &str, state: &str) -> String {
    format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&access_type=offline&prompt=consent",
        config.auth_url,
        urlencoding::encode(&config.client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(&config.scopes.join(" ")),
        urlencoding::encode(state),
    )
}

/// Exchange an authorization code for tokens.
pub async fn exchange_code(
    config: &OAuthProviderConfig,
    code: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(&config.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", &config.client_id),
            ("client_secret", &config.client_secret),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await
        .map_err(|e| format!("Token exchange failed: {}", e))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Token exchange error: {}", text));
    }

    resp.json::<TokenResponse>()
        .await
        .map_err(|e| format!("Failed to parse token response: {}", e))
}

/// Refresh an expired access token.
pub async fn refresh_token(
    config: &OAuthProviderConfig,
    refresh_token: &str,
) -> Result<TokenResponse, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(&config.token_url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &config.client_id),
            ("client_secret", &config.client_secret),
        ])
        .send()
        .await
        .map_err(|e| format!("Token refresh failed: {}", e))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Token refresh error: {}", text));
    }

    resp.json::<TokenResponse>()
        .await
        .map_err(|e| format!("Failed to parse refresh response: {}", e))
}

// -- Pre-configured providers --

pub fn gmail_provider(client_id: &str, client_secret: &str) -> OAuthProviderConfig {
    OAuthProviderConfig {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
        token_url: "https://oauth2.googleapis.com/token".to_string(),
        scopes: vec![
            "https://www.googleapis.com/auth/gmail.readonly".to_string(),
            "https://www.googleapis.com/auth/gmail.send".to_string(),
            "https://www.googleapis.com/auth/gmail.modify".to_string(),
            "https://www.googleapis.com/auth/calendar".to_string(),
            "https://www.googleapis.com/auth/drive.readonly".to_string(),
        ],
    }
}

pub fn slack_provider(client_id: &str, client_secret: &str) -> OAuthProviderConfig {
    OAuthProviderConfig {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
        auth_url: "https://slack.com/oauth/v2/authorize".to_string(),
        token_url: "https://slack.com/api/oauth.v2.access".to_string(),
        scopes: vec![
            "channels:read".to_string(),
            "channels:history".to_string(),
            "chat:write".to_string(),
            "users:read".to_string(),
        ],
    }
}

pub fn github_provider(client_id: &str, client_secret: &str) -> OAuthProviderConfig {
    OAuthProviderConfig {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
        auth_url: "https://github.com/login/oauth/authorize".to_string(),
        token_url: "https://github.com/login/oauth/access_token".to_string(),
        scopes: vec!["repo".to_string(), "read:user".to_string()],
    }
}

pub fn jira_provider(client_id: &str, client_secret: &str) -> OAuthProviderConfig {
    OAuthProviderConfig {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
        auth_url: "https://auth.atlassian.com/authorize".to_string(),
        token_url: "https://auth.atlassian.com/oauth/token".to_string(),
        scopes: vec![
            "read:jira-work".to_string(),
            "write:jira-work".to_string(),
            "read:jira-user".to_string(),
        ],
    }
}

pub fn linear_provider(client_id: &str, client_secret: &str) -> OAuthProviderConfig {
    OAuthProviderConfig {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
        auth_url: "https://linear.app/oauth/authorize".to_string(),
        token_url: "https://api.linear.app/oauth/token".to_string(),
        scopes: vec!["read".to_string(), "write".to_string()],
    }
}

/// Get the OAuth provider config for a given app ID, using credentials from IntegrationsConfig.
pub fn get_provider(
    app_id: &str,
    integrations: &crate::IntegrationsConfig,
) -> Option<OAuthProviderConfig> {
    match app_id {
        "gmail" | "google" | "calendar" | "drive" => integrations
            .gmail
            .as_ref()
            .map(|c| gmail_provider(&c.client_id, &c.client_secret)),
        "slack" => integrations
            .slack
            .as_ref()
            .map(|c| slack_provider(&c.client_id, &c.client_secret)),
        "github" => integrations
            .github
            .as_ref()
            .map(|c| github_provider(&c.client_id, &c.client_secret)),
        "jira" => integrations
            .jira
            .as_ref()
            .map(|c| jira_provider(&c.client_id, &c.client_secret)),
        "linear" => integrations
            .linear
            .as_ref()
            .map(|c| linear_provider(&c.client_id, &c.client_secret)),
        _ => None,
    }
}
