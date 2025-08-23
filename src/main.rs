use std::fs::File;
use std::io::Write;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STD;
use clap::Parser;
use clap::ValueEnum;
use figment::{
    Figment,
    providers::{Format, Toml},
};
use serde::Deserialize;
use slug::slugify;
use ureq::unversioned::multipart::Form;

mod spinner;

#[derive(Deserialize)]
struct ImageData {
    #[serde(rename = "b64_json")]
    b64_json: String,
}

#[derive(Deserialize)]
struct GenerationResponse {
    data: Vec<ImageData>,
}

#[derive(clap::ValueEnum, Clone)]
enum Provider {
    Azure,
    OpenAI,
}

#[derive(ValueEnum, Clone)]
enum ImageQuality {
    High,
    Medium,
    Low,
}

impl std::fmt::Display for ImageQuality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let quality_str = match self {
            ImageQuality::High => "high",
            ImageQuality::Medium => "medium",
            ImageQuality::Low => "low",
        };
        write!(f, "{quality_str}")
    }
}

#[derive(ValueEnum, Clone)]
enum ImageResolution {
    #[value(name = "1024x1024")]
    R1024x1024,
    #[value(name = "1024x1536")]
    R1024x1536,
    #[value(name = "1536x1024")]
    R1536x1024,
}

impl std::fmt::Display for ImageResolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_possible_value().unwrap().get_name())
    }
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[clap(short, long)]
    provider: Provider,

    prompt: String,

    #[arg(long, default_value_t = ImageQuality::High)]
    quality: ImageQuality,

    #[arg(long, default_value_t = ImageResolution::R1024x1024)]
    resolution: ImageResolution,

    #[arg(long, short, default_value_t = 1)]
    count: u8,

    #[clap(long, short)]
    reference: Option<std::path::PathBuf>,
}

#[derive(Deserialize)]
struct AzureConfig {
    api_base: String,
    api_key: String,
    deployment: String,
}

#[derive(Deserialize)]
struct Config {
    azure: Option<AzureConfig>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let xdg_dirs = xdg::BaseDirectories::with_prefix("imgmc");
    let xdg_file = xdg_dirs
        .get_config_file("config.toml")
        .ok_or("Could not get config file")?;

    if !std::path::Path::new(&xdg_file).exists() {
        eprintln!("Config file not found at: {}", xdg_file.display());
        std::process::exit(1);
    }

    let config: Config = Figment::new().merge(Toml::file(xdg_file)).extract()?;

    let azure_config = match config.azure {
        Some(cfg) => cfg,
        None => {
            eprintln!("Azure configuration is missing");
            std::process::exit(1);
        }
    };

    let api_key = azure_config.api_key;
    let api_base = azure_config.api_base;
    let api_version = "2025-04-01-preview";
    let deployment = azure_config.deployment;

    let gen_url = format!(
        "{}/openai/deployments/{}/images/generations?api-version={}",
        api_base, deployment, api_version
    );

    let edits_url = format!(
        "{}/openai/deployments/{}/images/edits?api-version={}",
        api_base, deployment, api_version
    );

    let size = cli.resolution.to_string();
    let quality = cli.quality.to_string();
    let n = cli.count;

    let sp = spinner::Spinner::start("Calling API...");

    let gen_resp: GenerationResponse = if let Some(ref_path) = cli.reference.as_ref() {
        let n = n.to_string();

        // Use the edits endpoint with multipart/form-data
        let form = Form::new()
            .text("prompt", &cli.prompt)
            .text("n", &n)
            .text("size", &size)
            .text("quality", &quality)
            .text("output_format", "png")
            .file("image", ref_path)?;

        ureq::post(&edits_url)
            .header("api-key", &api_key)
            .send(form)?
            .body_mut()
            .read_json::<GenerationResponse>()?
    } else {
        // Use the generations endpoint with JSON
        let body = serde_json::json!({
            "prompt": cli.prompt,
            "n": n,
            "size": size,
            "quality": quality,
            "output_format": "png"
        });

        ureq::post(&gen_url)
            .header("Content-Type", "application/json")
            .header("api-key", &api_key)
            .send_json(body)?
            .body_mut()
            .read_json::<GenerationResponse>()
            .unwrap()
    };

    drop(sp);

    // Save each returned image
    for (i, item) in gen_resp.data.iter().enumerate() {
        let bytes = BASE64_STD
            .decode(&item.b64_json)
            .map_err(|e| format!("Base64 decode failed: {e}"))?;

        let slug = slugify(&cli.prompt);
        let trimmed_slug = if slug.len() > 50 {
            slug[..50].to_string()
        } else {
            slug
        };

        let mut counter = i + 1;
        let filename = loop {
            let candidate = format!("{trimmed_slug}_{counter}.png");
            if !std::path::Path::new(&candidate).exists() {
                break candidate;
            }
            counter = counter
                .checked_add(1)
                .ok_or("Counter overflow: too many files with similar names")?;
        };

        let mut file = File::create(&filename)?;
        file.write_all(&bytes)?;
        println!("Image saved to: {filename}");
    }

    Ok(())
}
