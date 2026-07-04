use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use serde::Deserialize;

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// Structured failure so main() can map to stable exit codes.
#[derive(Debug)]
pub enum ApiError {
    /// 401/403 — bad or missing credentials.
    Auth(String),
    /// The prompt or an input image was blocked by moderation.
    ModerationBlocked(String),
    /// Anything else the API rejected or that failed in transit.
    Other(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Auth(m) => write!(f, "authentication failed: {m}"),
            ApiError::ModerationBlocked(m) => write!(f, "blocked by moderation: {m}"),
            ApiError::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ApiError {}

#[derive(Debug, Deserialize)]
pub struct ImageData {
    pub b64_json: Option<String>,
    pub url: Option<String>,
    pub revised_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImagesResponse {
    pub data: Vec<ImageData>,
    pub usage: Option<serde_json::Value>,
    pub size: Option<String>,
    pub quality: Option<String>,
    pub output_format: Option<String>,
    pub background: Option<String>,
}

impl ImageData {
    pub fn decode(&self, client: &Client) -> Result<Vec<u8>> {
        if let Some(b64) = &self.b64_json {
            return base64::engine::general_purpose::STANDARD
                .decode(b64)
                .context("failed to decode base64 image data");
        }
        if let Some(url) = &self.url {
            let resp = client
                .http
                .get(url)
                .send()
                .with_context(|| format!("failed to download image from {url}"))?;
            return Ok(resp.bytes().context("failed to read image bytes")?.to_vec());
        }
        bail!("API response contained neither b64_json nor url");
    }
}

/// Options shared by generate and edit requests.
#[derive(Debug, Default)]
pub struct ImageRequest {
    pub prompt: String,
    pub model: String,
    pub n: u8,
    pub size: Option<String>,
    pub quality: Option<String>,
    pub output_format: Option<String>,
    pub output_compression: Option<u8>,
    pub background: Option<String>,
    pub moderation: Option<String>,
    // edit-only
    pub images: Vec<std::path::PathBuf>,
    pub mask: Option<std::path::PathBuf>,
    pub input_fidelity: Option<String>,
}

pub struct Client {
    http: reqwest::blocking::Client,
    base_url: String,
    api_key: String,
}

impl Client {
    pub fn new(api_key: String, base_url: Option<String>, timeout_secs: u64) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent(concat!("imagegen-cli/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("failed to build HTTP client")?;
        let base_url = base_url
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        Ok(Client {
            http,
            base_url,
            api_key,
        })
    }

    pub fn generate(&self, req: &ImageRequest) -> Result<ImagesResponse> {
        let mut body = serde_json::json!({
            "model": req.model,
            "prompt": req.prompt,
            "n": req.n,
        });
        let obj = body.as_object_mut().unwrap();
        if let Some(v) = &req.size {
            obj.insert("size".into(), v.clone().into());
        }
        if let Some(v) = &req.quality {
            obj.insert("quality".into(), v.clone().into());
        }
        if let Some(v) = &req.output_format {
            obj.insert("output_format".into(), v.clone().into());
        }
        if let Some(v) = req.output_compression {
            obj.insert("output_compression".into(), v.into());
        }
        if let Some(v) = &req.background {
            obj.insert("background".into(), v.clone().into());
        }
        if let Some(v) = &req.moderation {
            obj.insert("moderation".into(), v.clone().into());
        }

        let resp = self
            .http
            .post(format!("{}/images/generations", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .context("request to OpenAI failed (network error or timeout)")?;
        parse_response(resp)
    }

    pub fn edit(&self, req: &ImageRequest) -> Result<ImagesResponse> {
        let mut form = reqwest::blocking::multipart::Form::new()
            .text("model", req.model.clone())
            .text("prompt", req.prompt.clone())
            .text("n", req.n.to_string());

        // The API accepts a single `image` part or repeated `image[]` parts.
        let image_field = if req.images.len() > 1 {
            "image[]"
        } else {
            "image"
        };
        for path in &req.images {
            form = form.part(image_field.to_string(), file_part(path)?);
        }
        if let Some(mask) = &req.mask {
            form = form.part("mask", file_part(mask)?);
        }
        if let Some(v) = &req.size {
            form = form.text("size", v.clone());
        }
        if let Some(v) = &req.quality {
            form = form.text("quality", v.clone());
        }
        if let Some(v) = &req.output_format {
            form = form.text("output_format", v.clone());
        }
        if let Some(v) = req.output_compression {
            form = form.text("output_compression", v.to_string());
        }
        if let Some(v) = &req.background {
            form = form.text("background", v.clone());
        }
        if let Some(v) = &req.input_fidelity {
            form = form.text("input_fidelity", v.clone());
        }

        let resp = self
            .http
            .post(format!("{}/images/edits", self.base_url))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .context("request to OpenAI failed (network error or timeout)")?;
        parse_response(resp)
    }

    /// List model ids that look image-related.
    pub fn list_image_models(&self) -> Result<Vec<String>> {
        #[derive(Deserialize)]
        struct Model {
            id: String,
        }
        #[derive(Deserialize)]
        struct ModelList {
            data: Vec<Model>,
        }
        let resp = self
            .http
            .get(format!("{}/models", self.base_url))
            .bearer_auth(&self.api_key)
            .send()
            .context("request to OpenAI failed (network error or timeout)")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().unwrap_or_default();
            return Err(classify_error(status, &text).into());
        }
        let list: ModelList = resp.json().context("failed to parse model list")?;
        let mut ids: Vec<String> = list
            .data
            .into_iter()
            .map(|m| m.id)
            .filter(|id| id.contains("image") || id.starts_with("dall-e"))
            .collect();
        ids.sort();
        Ok(ids)
    }
}

fn file_part(path: &Path) -> Result<reqwest::blocking::multipart::Part> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read input image {}", path.display()))?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image.png")
        .to_string();
    let mime = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
    {
        Some(ext) if ext == "jpg" || ext == "jpeg" => "image/jpeg",
        Some(ext) if ext == "webp" => "image/webp",
        _ => "image/png",
    };
    Ok(reqwest::blocking::multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str(mime)?)
}

fn parse_response(resp: reqwest::blocking::Response) -> Result<ImagesResponse> {
    let status = resp.status();
    let text = resp.text().context("failed to read API response body")?;
    if !status.is_success() {
        return Err(classify_error(status, &text).into());
    }
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse API response: {}", truncate(&text, 500)))
}

/// Map an API error body onto a typed error with a readable message.
pub fn classify_error(status: reqwest::StatusCode, body: &str) -> ApiError {
    #[derive(Deserialize)]
    struct ErrBody {
        error: ErrDetail,
    }
    #[derive(Deserialize)]
    struct ErrDetail {
        message: Option<String>,
        code: Option<String>,
        #[serde(rename = "type")]
        kind: Option<String>,
    }

    let detail: Option<ErrDetail> = serde_json::from_str::<ErrBody>(body).ok().map(|b| b.error);
    let message = detail
        .as_ref()
        .and_then(|d| d.message.clone())
        .unwrap_or_else(|| truncate(body, 300));
    let code = detail
        .as_ref()
        .and_then(|d| d.code.clone())
        .unwrap_or_default();
    let kind = detail
        .as_ref()
        .and_then(|d| d.kind.clone())
        .unwrap_or_default();

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return ApiError::Auth(message);
    }
    if code == "moderation_blocked" || kind == "image_generation_user_error" {
        return ApiError::ModerationBlocked(message);
    }
    ApiError::Other(format!("API error ({}): {message}", status.as_u16()))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

/// Validate a --size value: "auto" or WIDTHxHEIGHT.
pub fn validate_size(size: &str) -> Result<()> {
    if size == "auto" {
        return Ok(());
    }
    let parts: Vec<&str> = size.split('x').collect();
    let ok = parts.len() == 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    if !ok {
        return Err(anyhow!(
            "invalid size '{size}': expected WIDTHxHEIGHT (e.g. 1024x1024) or 'auto'"
        ));
    }
    let (w, h): (u32, u32) = (parts[0].parse()?, parts[1].parse()?);
    if w == 0 || h == 0 {
        return Err(anyhow!(
            "invalid size '{size}': dimensions must be positive"
        ));
    }
    Ok(())
}

/// gpt-image-2 accepts flexible resolutions with these constraints; returns a
/// human-readable warning when the requested size will likely be rejected.
pub fn gpt_image_2_size_hint(size: &str) -> Option<String> {
    if size == "auto" {
        return None;
    }
    let mut parts = size.split('x');
    let w: u64 = parts.next()?.parse().ok()?;
    let h: u64 = parts.next()?.parse().ok()?;
    let (long, short) = (w.max(h), w.min(h));
    let pixels = w * h;
    let mut problems = Vec::new();
    if !w.is_multiple_of(16) || !h.is_multiple_of(16) {
        problems.push("both dimensions must be multiples of 16".to_string());
    }
    if long > 3840 {
        problems.push("max edge is 3840px".to_string());
    }
    if short * 3 < long {
        problems.push("aspect ratio must be at most 3:1".to_string());
    }
    if !(655_360..=8_294_400).contains(&pixels) {
        problems.push(format!(
            "total pixels must be between 655,360 and 8,294,400 (got {pixels})"
        ));
    }
    if problems.is_empty() {
        None
    } else {
        Some(format!(
            "size {size} may be rejected by gpt-image-2: {}",
            problems.join("; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_size_accepts_auto_and_dimensions() {
        assert!(validate_size("auto").is_ok());
        assert!(validate_size("1024x1024").is_ok());
        assert!(validate_size("3840x2160").is_ok());
    }

    #[test]
    fn validate_size_rejects_garbage() {
        assert!(validate_size("big").is_err());
        assert!(validate_size("1024").is_err());
        assert!(validate_size("1024x").is_err());
        assert!(validate_size("x1024").is_err());
        assert!(validate_size("1024x1024x3").is_err());
        assert!(validate_size("0x100").is_err());
        assert!(validate_size("-10x100").is_err());
    }

    #[test]
    fn size_hint_flags_gpt_image_2_constraints() {
        assert!(gpt_image_2_size_hint("auto").is_none());
        assert!(gpt_image_2_size_hint("1024x1024").is_none());
        assert!(gpt_image_2_size_hint("3840x2160").is_none());
        // not multiple of 16
        assert!(gpt_image_2_size_hint("1000x1000").is_some());
        // too large an edge
        assert!(gpt_image_2_size_hint("4096x2160").is_some());
        // ratio > 3:1
        assert!(gpt_image_2_size_hint("3840x1024").is_some());
        // too few pixels
        assert!(gpt_image_2_size_hint("640x640").is_some());
    }

    #[test]
    fn classify_moderation_error() {
        let body = r#"{"error":{"message":"Your request was blocked.","type":"image_generation_user_error","code":"moderation_blocked"}}"#;
        match classify_error(reqwest::StatusCode::BAD_REQUEST, body) {
            ApiError::ModerationBlocked(m) => assert!(m.contains("blocked")),
            other => panic!("expected moderation error, got {other:?}"),
        }
    }

    #[test]
    fn classify_auth_error() {
        let body = r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error","code":"invalid_api_key"}}"#;
        match classify_error(reqwest::StatusCode::UNAUTHORIZED, body) {
            ApiError::Auth(_) => {}
            other => panic!("expected auth error, got {other:?}"),
        }
    }

    #[test]
    fn classify_other_error_with_unparseable_body() {
        match classify_error(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "<html>oops</html>",
        ) {
            ApiError::Other(m) => assert!(m.contains("500")),
            other => panic!("expected other error, got {other:?}"),
        }
    }
}
