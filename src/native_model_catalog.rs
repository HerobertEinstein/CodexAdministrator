use std::{
    fs::{self, File},
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use toml_edit::{DocumentMut, Item, value};

use crate::{InjectedModelDescriptor, install_bootstrap_atomically};

const OFFICIAL_CATALOG_PREFIXES: [&[u8]; 2] = [b"{\n  \"models\": [", b"{\r\n  \"models\": ["];
const SCAN_BUFFER_BYTES: usize = 1024 * 1024;
const MAX_OFFICIAL_BINARY_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_MODEL_CATALOG_BYTES: usize = 4 * 1024 * 1024;
const CONSERVATIVE_CLIENT_CONTEXT_WINDOW: u64 = 32_768;
const NATIVE_GPT_IDENTITY: &str = "You are Codex, an agent based on GPT-5.";

pub fn install_grok_native_model_catalog(
    official_codex_binary: &Path,
    catalog_path: &Path,
    config_path: &Path,
    injected_models: &[InjectedModelDescriptor],
) -> Result<()> {
    if injected_models.is_empty() {
        bail!("Grok native model catalog requires at least one injected model");
    }
    if !catalog_path.is_absolute() {
        bail!("Grok native model catalog path must be absolute");
    }
    for model in injected_models {
        model.validate()?;
    }

    let mut catalog = extract_official_model_catalog(official_codex_binary)?;
    normalize_official_model_catalog(&mut catalog)?;
    append_grok_models(&mut catalog, injected_models)?;
    let rendered = serde_json::to_vec_pretty(&catalog)
        .context("failed to serialize the Grok native model catalog")?;
    install_if_changed(catalog_path, &rendered)?;
    install_catalog_reference(config_path, catalog_path)?;
    Ok(())
}

fn normalize_official_model_catalog(catalog: &mut Value) -> Result<()> {
    let models = catalog
        .get_mut("models")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow::anyhow!("bundled official model catalog has no models array"))?;
    for model in models {
        let model = model.as_object_mut().ok_or_else(|| {
            anyhow::anyhow!("bundled official model catalog entry is not an object")
        })?;
        if !model.contains_key("supports_reasoning_summaries") {
            let supports = model
                .get("default_reasoning_summary")
                .and_then(Value::as_str)
                .is_some_and(|summary| !summary.eq_ignore_ascii_case("none"));
            model.insert("supports_reasoning_summaries".into(), json!(supports));
        }
    }
    Ok(())
}

pub fn remove_grok_native_model_catalog(catalog_path: &Path, config_path: &Path) -> Result<bool> {
    if !catalog_path.is_absolute() {
        bail!("Grok native model catalog path must be absolute");
    }
    let mut changed = false;
    let existing = match fs::read_to_string(config_path) {
        Ok(existing) => Some(existing),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };
    if let Some(existing) = existing
        && !existing.trim().is_empty()
    {
        let mut document = existing
            .parse::<DocumentMut>()
            .with_context(|| format!("failed to parse {}", config_path.display()))?;
        let catalog = catalog_path.to_str().ok_or_else(|| {
            anyhow::anyhow!("Grok native model catalog path is not valid Unicode")
        })?;
        if document.get("model_catalog_json").and_then(Item::as_str) == Some(catalog) {
            document.remove("model_catalog_json");
            install_bootstrap_atomically(config_path, document.to_string().as_bytes())?;
            changed = true;
        }
    }

    match fs::symlink_metadata(catalog_path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                bail!("Grok native model catalog is not a regular project-owned file");
            }
            fs::remove_file(catalog_path).with_context(|| {
                format!(
                    "failed to remove stale Grok native model catalog {}",
                    catalog_path.display()
                )
            })?;
            changed = true;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to inspect stale Grok native model catalog {}",
                    catalog_path.display()
                )
            });
        }
    }
    Ok(changed)
}

fn extract_official_model_catalog(path: &Path) -> Result<Value> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect official Codex binary {}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_OFFICIAL_BINARY_BYTES {
        bail!("official Codex binary has an invalid file shape or size");
    }

    let mut file = File::open(path)
        .with_context(|| format!("failed to open official Codex binary {}", path.display()))?;
    let start = find_catalog_start(&mut file)?;
    file.seek(SeekFrom::Start(start))
        .context("failed to seek to the bundled official model catalog")?;
    let bytes = read_json_value(&mut file)?;
    let catalog: Value = serde_json::from_slice(&bytes)
        .context("bundled official model catalog is not valid JSON")?;
    let models = catalog
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("bundled official model catalog has no models array"))?;
    if models.is_empty() {
        bail!("bundled official model catalog is empty");
    }
    Ok(catalog)
}

fn find_catalog_start(file: &mut File) -> Result<u64> {
    file.seek(SeekFrom::Start(0))
        .context("failed to rewind the official Codex binary")?;
    let mut reader = BufReader::with_capacity(SCAN_BUFFER_BYTES, file);
    let mut buffer = vec![0_u8; SCAN_BUFFER_BYTES];
    let mut carry = Vec::new();
    let mut absolute = 0_u64;

    loop {
        let read = reader
            .read(&mut buffer)
            .context("failed while scanning the official Codex binary")?;
        if read == 0 {
            break;
        }
        let mut combined = Vec::with_capacity(carry.len() + read);
        combined.extend_from_slice(&carry);
        combined.extend_from_slice(&buffer[..read]);
        if let Some(index) = OFFICIAL_CATALOG_PREFIXES
            .iter()
            .filter_map(|prefix| find_bytes(&combined, prefix))
            .min()
        {
            return Ok(absolute - u64::try_from(carry.len()).unwrap_or(0)
                + u64::try_from(index).unwrap_or(0));
        }
        let keep = OFFICIAL_CATALOG_PREFIXES
            .iter()
            .map(|prefix| prefix.len())
            .max()
            .unwrap_or(1)
            .saturating_sub(1);
        carry.clear();
        carry.extend_from_slice(&combined[combined.len().saturating_sub(keep)..]);
        absolute = absolute.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
    }

    bail!("official Codex binary does not contain the bundled model catalog")
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn read_json_value(file: &mut File) -> Result<Vec<u8>> {
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut output = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut depth = 0_i64;
    let mut started = false;
    let mut in_string = false;
    let mut escaped = false;

    loop {
        let read = reader
            .read(&mut buffer)
            .context("failed to read the bundled official model catalog")?;
        if read == 0 {
            break;
        }
        for byte in &buffer[..read] {
            output.push(*byte);
            if output.len() > MAX_MODEL_CATALOG_BYTES {
                bail!("bundled official model catalog exceeds its size limit");
            }
            if in_string {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match *byte {
                b'"' => in_string = true,
                b'{' | b'[' => {
                    started = true;
                    depth += 1;
                }
                b'}' | b']' => {
                    depth -= 1;
                    if started && depth == 0 {
                        return Ok(output);
                    }
                    if depth < 0 {
                        bail!("bundled official model catalog closed before it opened");
                    }
                }
                _ => {}
            }
        }
    }

    bail!("bundled official model catalog is truncated")
}

fn append_grok_models(
    catalog: &mut Value,
    injected_models: &[InjectedModelDescriptor],
) -> Result<()> {
    let models = catalog
        .get_mut("models")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow::anyhow!("bundled official model catalog has no models array"))?;
    let template = models
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("bundled official model catalog is empty"))?;
    let official_instructions = template
        .get("base_instructions")
        .and_then(Value::as_str)
        .filter(|instructions| !instructions.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("official default model has no base instructions"))?
        .to_owned();
    let initial_model_count = models.len();

    for (index, descriptor) in injected_models.iter().enumerate() {
        if models.iter().any(|model| {
            model.get("slug").and_then(Value::as_str) == Some(descriptor.model.as_str())
        }) {
            bail!(
                "configured model {} collides with the official model catalog",
                descriptor.model
            );
        }
        let supported_reasoning_levels = descriptor
            .supported_reasoning_efforts
            .iter()
            .map(|effort| {
                json!({
                    "effort": effort.reasoning_effort,
                    "description": effort.description,
                })
            })
            .collect::<Vec<_>>();
        let grok_identity = format!(
            "You are Codex, running {} inside the official ChatGPT/Codex host.",
            descriptor.display_name
        );
        let base_instructions = if official_instructions.contains(NATIVE_GPT_IDENTITY) {
            official_instructions.replacen(NATIVE_GPT_IDENTITY, &grok_identity, 1)
        } else {
            format!("{grok_identity}\n\n{official_instructions}")
        };
        let priority = i64::try_from(initial_model_count + index).unwrap_or(i64::MAX);
        let mut model = template.clone();
        let model = model.as_object_mut().ok_or_else(|| {
            anyhow::anyhow!("official default model catalog entry is not an object")
        })?;
        model.insert("slug".into(), json!(descriptor.model));
        model.insert("display_name".into(), json!(descriptor.display_name));
        model.insert("description".into(), json!(descriptor.description));
        model.insert(
            "default_reasoning_level".into(),
            json!(descriptor.default_reasoning_effort),
        );
        model.insert(
            "supported_reasoning_levels".into(),
            json!(supported_reasoning_levels),
        );
        model.insert("shell_type".into(), json!("shell_command"));
        model.insert("visibility".into(), json!("none"));
        model.insert("supported_in_api".into(), json!(true));
        model.insert("priority".into(), json!(priority));
        model.insert("additional_speed_tiers".into(), json!([]));
        model.insert("service_tiers".into(), json!([]));
        model.insert("default_service_tier".into(), Value::Null);
        model.insert(
            "availability_nux".into(),
            json!({"message": "codex-administrator:grok-native-catalog-v1"}),
        );
        model.insert("upgrade".into(), Value::Null);
        model.insert("base_instructions".into(), json!(base_instructions));
        model.insert("model_messages".into(), Value::Null);
        model.insert("include_skills_usage_instructions".into(), json!(false));
        model.insert("supports_reasoning_summaries".into(), json!(false));
        model.insert("default_reasoning_summary".into(), json!("none"));
        model.insert("support_verbosity".into(), json!(false));
        model.insert("default_verbosity".into(), Value::Null);
        model.insert("apply_patch_tool_type".into(), Value::Null);
        model.insert("web_search_tool_type".into(), json!("text"));
        model.insert(
            "truncation_policy".into(),
            json!({"mode": "bytes", "limit": 10000}),
        );
        model.insert("supports_parallel_tool_calls".into(), json!(false));
        model.insert("supports_image_detail_original".into(), json!(false));
        model.insert(
            "context_window".into(),
            json!(CONSERVATIVE_CLIENT_CONTEXT_WINDOW),
        );
        model.insert(
            "max_context_window".into(),
            json!(CONSERVATIVE_CLIENT_CONTEXT_WINDOW),
        );
        model.insert("auto_compact_token_limit".into(), Value::Null);
        model.insert("comp_hash".into(), Value::Null);
        model.insert("effective_context_window_percent".into(), json!(95));
        model.insert("experimental_supported_tools".into(), json!([]));
        model.insert(
            "input_modalities".into(),
            json!(descriptor.input_modalities),
        );
        model.insert("supports_search_tool".into(), json!(false));
        model.insert("use_responses_lite".into(), json!(false));
        model.insert("auto_review_model_override".into(), Value::Null);
        model.insert("tool_mode".into(), Value::Null);
        model.insert("multi_agent_version".into(), Value::Null);
        models.push(Value::Object(model.clone()));
    }
    Ok(())
}

fn install_catalog_reference(config_path: &Path, catalog_path: &Path) -> Result<()> {
    let existing = match fs::read_to_string(config_path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };
    let mut document = if existing.trim().is_empty() {
        DocumentMut::new()
    } else {
        existing
            .parse::<DocumentMut>()
            .with_context(|| format!("failed to parse {}", config_path.display()))?
    };
    let catalog = catalog_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Grok native model catalog path is not valid Unicode"))?;
    document["model_catalog_json"] = value(catalog);
    install_if_changed(config_path, document.to_string().as_bytes())?;
    Ok(())
}

fn install_if_changed(path: &Path, content: &[u8]) -> Result<()> {
    match fs::read(path) {
        Ok(existing) if existing == content => Ok(()),
        Ok(_) => install_bootstrap_atomically(path, content).map(|_| ()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            install_bootstrap_atomically(path, content).map(|_| ())
        }
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}
