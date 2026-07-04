use std::path::PathBuf;

/// Where the resolved API key came from, for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Flag,
    Env,
    CodexAuthFile,
}

impl std::fmt::Display for KeySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeySource::Flag => write!(f, "--api-key flag"),
            KeySource::Env => write!(f, "OPENAI_API_KEY environment variable"),
            KeySource::CodexAuthFile => write!(f, "~/.codex/auth.json"),
        }
    }
}

/// Resolve the API key: --api-key flag > OPENAI_API_KEY env > ~/.codex/auth.json.
///
/// The Codex CLI stores an `OPENAI_API_KEY` field in its auth.json when the user
/// authenticated with an API key. ChatGPT OAuth tokens (`tokens.access_token`) are
/// scoped to the ChatGPT backend and are NOT valid for the platform Images API,
/// so they are deliberately not used here.
pub fn resolve_api_key(flag: Option<&str>) -> Option<(String, KeySource)> {
    if let Some(key) = flag.map(str::trim).filter(|k| !k.is_empty()) {
        return Some((key.to_string(), KeySource::Flag));
    }
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        let key = key.trim();
        if !key.is_empty() {
            return Some((key.to_string(), KeySource::Env));
        }
    }
    if let Some(key) = codex_auth_key() {
        return Some((key, KeySource::CodexAuthFile));
    }
    None
}

fn codex_auth_key() -> Option<String> {
    let path = codex_auth_path()?;
    let contents = std::fs::read_to_string(path).ok()?;
    key_from_codex_auth_json(&contents)
}

fn codex_auth_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    Some(home.join(".codex").join("auth.json"))
}

/// Extract a usable API key from Codex CLI auth.json contents, if present.
pub fn key_from_codex_auth_json(contents: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(contents).ok()?;
    let key = json.get("OPENAI_API_KEY")?.as_str()?.trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_api_key_from_codex_auth() {
        let json = r#"{"OPENAI_API_KEY": "sk-test123", "tokens": {"access_token": "oauth"}}"#;
        assert_eq!(
            key_from_codex_auth_json(json),
            Some("sk-test123".to_string())
        );
    }

    #[test]
    fn ignores_null_api_key_in_codex_auth() {
        let json = r#"{"OPENAI_API_KEY": null, "tokens": {"access_token": "oauth"}}"#;
        assert_eq!(key_from_codex_auth_json(json), None);
    }

    #[test]
    fn ignores_empty_api_key_in_codex_auth() {
        let json = r#"{"OPENAI_API_KEY": "  "}"#;
        assert_eq!(key_from_codex_auth_json(json), None);
    }

    #[test]
    fn ignores_malformed_codex_auth() {
        assert_eq!(key_from_codex_auth_json("not json"), None);
        assert_eq!(key_from_codex_auth_json("{}"), None);
    }

    #[test]
    fn flag_takes_precedence() {
        let (key, source) = resolve_api_key(Some("sk-flag")).unwrap();
        assert_eq!(key, "sk-flag");
        assert_eq!(source, KeySource::Flag);
    }
}
