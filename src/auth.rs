/// Where the resolved API key came from, for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Flag,
    Env,
}

impl std::fmt::Display for KeySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeySource::Flag => write!(f, "--api-key flag"),
            KeySource::Env => write!(f, "OPENAI_API_KEY environment variable"),
        }
    }
}

/// Resolve the API key: --api-key flag > OPENAI_API_KEY env.
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
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_takes_precedence() {
        let (key, source) = resolve_api_key(Some("sk-flag")).unwrap();
        assert_eq!(key, "sk-flag");
        assert_eq!(source, KeySource::Flag);
    }

    #[test]
    fn blank_flag_is_ignored() {
        // Falls through to env (or None); must not return an empty key.
        if let Some((key, _)) = resolve_api_key(Some("  ")) {
            assert!(!key.trim().is_empty());
        }
    }
}
