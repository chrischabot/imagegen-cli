mod api;
mod auth;
mod output;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};

use api::{ApiError, Client, ImageRequest};

/// Exit codes (stable, for scripts and agents):
/// 0 success · 1 API/network error · 2 missing/invalid credentials ·
/// 3 blocked by moderation · 4 invalid arguments or input files
#[derive(Parser)]
#[command(
    name = "imagegen",
    version,
    about = "Generate and edit images with OpenAI's gpt-image models",
    long_about = "Generate and edit images with OpenAI's gpt-image models (default: gpt-image-2).\n\
                  \n\
                  Authentication (in order of precedence):\n  \
                  1. --api-key flag\n  \
                  2. OPENAI_API_KEY environment variable\n  \
                  3. ~/.codex/auth.json (Codex CLI, API-key logins only)\n\
                  \n\
                  Saved image paths are printed to stdout, one per line; everything else\n\
                  goes to stderr. Use --json for machine-readable output.\n\
                  \n\
                  Exit codes: 0 success · 1 API/network error · 2 auth · 3 moderation · 4 bad input"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate images from a text prompt
    #[command(visible_alias = "gen")]
    Generate {
        /// Text description of the image to generate
        prompt: String,
        #[command(flatten)]
        opts: SharedOpts,
    },
    /// Edit or combine existing images guided by a prompt
    Edit {
        /// Text description of the edit to make
        prompt: String,
        /// Input image(s); repeat for multiple reference images
        #[arg(short = 'i', long = "input", required = true)]
        inputs: Vec<PathBuf>,
        /// Optional PNG mask; transparent areas mark regions to replace
        #[arg(long)]
        mask: Option<PathBuf>,
        /// How closely to preserve input image details (gpt-image-1/1.5;
        /// gpt-image-2 is always high fidelity)
        #[arg(long, value_enum)]
        fidelity: Option<Fidelity>,
        #[command(flatten)]
        opts: SharedOpts,
    },
    /// List image-capable models available to your API key
    Models {
        #[command(flatten)]
        conn: ConnOpts,
        /// Output as JSON array
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
struct SharedOpts {
    /// Model to use
    #[arg(short, long, default_value = "gpt-image-2")]
    model: String,

    /// Image size: WIDTHxHEIGHT or 'auto'. gpt-image-2 accepts any size where
    /// both edges are multiples of 16, max edge 3840, ratio <= 3:1
    #[arg(short, long, default_value = "auto")]
    size: String,

    /// Rendering quality (higher = better and slower and pricier)
    #[arg(short, long, value_enum, default_value_t = Quality::Auto)]
    quality: Quality,

    /// Output image format
    #[arg(short = 'f', long, value_enum, default_value_t = Format::Png)]
    format: Format,

    /// Compression level 0-100 for jpeg/webp output
    #[arg(short = 'c', long, value_parser = clap::value_parser!(u8).range(0..=100))]
    compression: Option<u8>,

    /// Background style (transparent is not supported by gpt-image-2)
    #[arg(short, long, value_enum)]
    background: Option<Background>,

    /// Moderation strictness (generate only)
    #[arg(long, value_enum)]
    moderation: Option<ModerationLevel>,

    /// Number of images to generate
    #[arg(short, long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=10))]
    n: u8,

    /// Output file or directory (default: auto-named file in current directory)
    #[arg(short, long)]
    out: Option<PathBuf>,

    /// Print machine-readable JSON (paths, usage, metadata) to stdout
    #[arg(long)]
    json: bool,

    /// Suppress progress messages on stderr
    #[arg(long)]
    quiet: bool,

    #[command(flatten)]
    conn: ConnOpts,
}

#[derive(Args)]
struct ConnOpts {
    /// OpenAI API key (overrides OPENAI_API_KEY and ~/.codex/auth.json)
    #[arg(long, value_name = "KEY")]
    api_key: Option<String>,

    /// API base URL, for proxies and compatible endpoints
    #[arg(long, env = "OPENAI_BASE_URL", default_value = api::DEFAULT_BASE_URL)]
    base_url: String,

    /// Request timeout in seconds (large/high-quality images can take minutes)
    #[arg(long, default_value_t = 300)]
    timeout: u64,
}

#[derive(Copy, Clone, ValueEnum)]
enum Quality {
    Auto,
    Low,
    Medium,
    High,
}

#[derive(Copy, Clone, ValueEnum)]
enum Format {
    Png,
    Jpeg,
    Webp,
}

impl Format {
    fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpeg",
            Format::Webp => "webp",
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
enum Background {
    Auto,
    Opaque,
    Transparent,
}

#[derive(Copy, Clone, ValueEnum)]
enum ModerationLevel {
    Auto,
    Low,
}

#[derive(Copy, Clone, ValueEnum)]
enum Fidelity {
    Low,
    High,
}

fn enum_str<T: ValueEnum>(v: T) -> String {
    v.to_possible_value().unwrap().get_name().to_string()
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Generate { prompt, opts } => run_image_command(prompt, opts, None),
        Command::Edit {
            prompt,
            inputs,
            mask,
            fidelity,
            opts,
        } => run_image_command(prompt, opts, Some((inputs, mask, fidelity))),
        Command::Models { conn, json } => run_models(conn, json),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(exit_code_for(&err))
        }
    }
}

fn exit_code_for(err: &anyhow::Error) -> u8 {
    match err.downcast_ref::<ApiError>() {
        Some(ApiError::Auth(_)) => 2,
        Some(ApiError::ModerationBlocked(_)) => 3,
        Some(ApiError::Other(_)) => 1,
        None => {
            if err.downcast_ref::<UsageError>().is_some() {
                4
            } else {
                1
            }
        }
    }
}

/// Marker for user-input problems (exit code 4).
#[derive(Debug)]
struct UsageError(String);

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for UsageError {}

fn usage_err(msg: String) -> anyhow::Error {
    anyhow!(UsageError(msg))
}

fn no_key_err() -> anyhow::Error {
    anyhow!(ApiError::Auth(
        "no API key found. Pass --api-key, set OPENAI_API_KEY, or log in to Codex CLI with an API key".to_string()
    ))
}

type EditArgs = (Vec<PathBuf>, Option<PathBuf>, Option<Fidelity>);

fn run_image_command(prompt: String, opts: SharedOpts, edit: Option<EditArgs>) -> Result<()> {
    api::validate_size(&opts.size).map_err(|e| usage_err(e.to_string()))?;
    if prompt.trim().is_empty() {
        return Err(usage_err("prompt must not be empty".to_string()));
    }
    if let Some((inputs, mask, _)) = &edit {
        for path in inputs.iter().chain(mask.iter()) {
            if !path.is_file() {
                return Err(usage_err(format!(
                    "input file not found: {}",
                    path.display()
                )));
            }
        }
    }
    if opts.model == "gpt-image-2" {
        if let Some(hint) = api::gpt_image_2_size_hint(&opts.size) {
            eprintln!("warning: {hint}");
        }
        if matches!(opts.background, Some(Background::Transparent)) {
            eprintln!("warning: gpt-image-2 does not support transparent backgrounds; use gpt-image-1.5 or gpt-image-1");
        }
    }
    if opts.compression.is_some() && matches!(opts.format, Format::Png) {
        eprintln!("warning: --compression only applies to jpeg/webp output");
    }

    let (api_key, key_source) =
        auth::resolve_api_key(opts.conn.api_key.as_deref()).ok_or_else(no_key_err)?;

    let client = Client::new(api_key, Some(opts.conn.base_url.clone()), opts.conn.timeout)?;

    let is_edit = edit.is_some();
    let request = ImageRequest {
        prompt: prompt.clone(),
        model: opts.model.clone(),
        n: opts.n,
        size: (opts.size != "auto").then(|| opts.size.clone()),
        quality: (!matches!(opts.quality, Quality::Auto)).then(|| enum_str(opts.quality)),
        output_format: (!matches!(opts.format, Format::Png)).then(|| enum_str(opts.format)),
        output_compression: opts.compression,
        background: opts.background.map(enum_str),
        moderation: if is_edit {
            None
        } else {
            opts.moderation.map(enum_str)
        },
        images: edit.as_ref().map(|(i, _, _)| i.clone()).unwrap_or_default(),
        mask: edit.as_ref().and_then(|(_, m, _)| m.clone()),
        input_fidelity: edit.as_ref().and_then(|(_, _, f)| f.map(enum_str)),
    };

    if !opts.quiet {
        eprintln!(
            "{} {} image(s) with {} (quality: {}, size: {}, auth: {})...",
            if is_edit { "editing" } else { "generating" },
            opts.n,
            opts.model,
            enum_str(opts.quality),
            opts.size,
            key_source,
        );
    }

    let started = std::time::Instant::now();
    let response = if is_edit {
        client.edit(&request)?
    } else {
        client.generate(&request)?
    };
    let elapsed = started.elapsed();

    if response.data.is_empty() {
        return Err(anyhow!("API returned no images"));
    }

    // The API reports the format it actually used; trust it for file extensions.
    let ext = response
        .output_format
        .clone()
        .unwrap_or_else(|| opts.format.ext().to_string());
    let paths = output::plan_paths(opts.out.as_deref(), &prompt, &ext, response.data.len());

    let mut saved = Vec::new();
    for (image, path) in response.data.iter().zip(&paths) {
        let bytes = image.decode(&client)?;
        output::save_image(path, &bytes)?;
        saved.push((path.clone(), bytes.len(), image.revised_prompt.clone()));
    }

    if !opts.quiet {
        eprintln!(
            "done in {:.1}s: {} image(s), {} total",
            elapsed.as_secs_f64(),
            saved.len(),
            human_bytes(saved.iter().map(|(_, b, _)| *b).sum::<usize>()),
        );
    }

    if opts.json {
        let payload = serde_json::json!({
            "model": opts.model,
            "operation": if is_edit { "edit" } else { "generate" },
            "elapsed_seconds": (elapsed.as_secs_f64() * 10.0).round() / 10.0,
            "size": response.size,
            "quality": response.quality,
            "output_format": response.output_format,
            "background": response.background,
            "usage": response.usage,
            "images": saved.iter().map(|(path, bytes, revised)| serde_json::json!({
                "path": std::path::absolute(path).unwrap_or_else(|_| path.clone()),
                "bytes": bytes,
                "revised_prompt": revised,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        for (path, _, _) in &saved {
            println!(
                "{}",
                std::path::absolute(path)
                    .unwrap_or_else(|_| path.clone())
                    .display()
            );
        }
    }
    Ok(())
}

fn run_models(conn: ConnOpts, json: bool) -> Result<()> {
    let (api_key, _) = auth::resolve_api_key(conn.api_key.as_deref()).ok_or_else(no_key_err)?;
    let client = Client::new(api_key, Some(conn.base_url), conn.timeout)?;
    let models = client.list_image_models()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&models)?);
    } else {
        for model in models {
            println!("{model}");
        }
    }
    Ok(())
}

fn human_bytes(bytes: usize) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn human_bytes_formats() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KB");
        assert_eq!(human_bytes(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn enum_str_matches_api_values() {
        assert_eq!(enum_str(Quality::High), "high");
        assert_eq!(enum_str(Format::Jpeg), "jpeg");
        assert_eq!(enum_str(Background::Transparent), "transparent");
        assert_eq!(enum_str(ModerationLevel::Low), "low");
        assert_eq!(enum_str(Fidelity::High), "high");
    }
}
