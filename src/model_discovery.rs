use std::{collections::BTreeSet, io::Read, time::Duration};

use anyhow::{Context, Result, bail};
use http::Uri;
use serde::{Deserialize, Serialize};

const MAX_MODEL_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MODELS: usize = 4096;
const MAX_API_KEY_BYTES: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredModel {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owned_by: Option<String>,
}

#[derive(Deserialize)]
struct ModelListEnvelope {
    data: Vec<ModelListEntry>,
}

#[derive(Deserialize)]
struct ModelListEntry {
    id: String,
    #[serde(default)]
    owned_by: Option<String>,
}

pub fn model_list_url(base_url: &str) -> Result<String> {
    let uri: Uri = base_url.parse().context("provider base URL is invalid")?;
    let scheme = uri
        .scheme_str()
        .ok_or_else(|| anyhow::anyhow!("provider base URL requires a scheme"))?;
    let host = uri
        .host()
        .ok_or_else(|| anyhow::anyhow!("provider base URL requires a host"))?;
    let authority = uri
        .authority()
        .ok_or_else(|| anyhow::anyhow!("provider base URL requires an authority"))?;
    let loopback = matches!(host, "127.0.0.1" | "localhost" | "::1");
    if scheme != "https" && !(scheme == "http" && loopback) {
        bail!("remote provider base URL must use HTTPS");
    }
    if authority.as_str().contains('@') || uri.query().is_some() {
        bail!("provider base URL must not contain credentials or a query");
    }
    let path = uri.path().trim_end_matches('/');
    if !path.ends_with("/v1") {
        bail!("provider base URL must end in /v1");
    }
    Ok(format!("{}/models", base_url.trim_end_matches('/')))
}

pub fn parse_model_list(bytes: &[u8]) -> Result<Vec<DiscoveredModel>> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_MODEL_RESPONSE_BYTES {
        bail!("model endpoint returned an invalid response size");
    }
    let envelope: ModelListEnvelope =
        serde_json::from_slice(bytes).context("model endpoint returned invalid JSON")?;
    if envelope.data.len() > MAX_MODELS {
        bail!("model endpoint returned too many models");
    }

    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    for entry in envelope.data {
        validate_model_text(&entry.id, "model id", 256)?;
        if !seen.insert(entry.id.clone()) {
            continue;
        }
        let owned_by = match entry.owned_by {
            Some(owner) => {
                validate_model_text(&owner, "model owner", 128)?;
                Some(owner)
            }
            None => None,
        };
        models.push(DiscoveredModel {
            id: entry.id,
            owned_by,
        });
    }
    if models.is_empty() {
        bail!("model endpoint returned no usable models");
    }
    Ok(models)
}

pub fn search_models<'a>(models: &'a [DiscoveredModel], query: &str) -> Vec<&'a DiscoveredModel> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return models.iter().collect();
    }
    models
        .iter()
        .filter(|model| {
            model.id.to_lowercase().contains(&query)
                || model
                    .owned_by
                    .as_deref()
                    .is_some_and(|owner| owner.to_lowercase().contains(&query))
        })
        .collect()
}

pub fn injectable_grok_models(models: &[DiscoveredModel]) -> Vec<&DiscoveredModel> {
    models
        .iter()
        .filter(|model| is_reviewed_grok_model_id(&model.id))
        .collect()
}

pub fn is_reviewed_grok_model_id(model: &str) -> bool {
    if model == "grok-4.5" {
        return true;
    }
    ["xhigh", "high", "medium", "low"]
        .into_iter()
        .find_map(|effort| model.strip_suffix(&format!("-{effort}")))
        .is_some_and(|base| matches!(base, "grok-4.3" | "grok-4.20-multi-agent"))
}

pub fn fetch_model_list(base_url: &str, api_key: &str) -> Result<Vec<DiscoveredModel>> {
    validate_api_key(api_key)?;
    let endpoint = model_list_url(base_url)?;
    let authorization = format!("Bearer {api_key}");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(20))
        .timeout_write(Duration::from_secs(10))
        .build();
    let response = match agent
        .get(&endpoint)
        .set("Accept", "application/json")
        .set("Authorization", &authorization)
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(status, _)) => {
            bail!("model endpoint returned HTTP {status}")
        }
        Err(ureq::Error::Transport(error)) => {
            return Err(anyhow::anyhow!(
                "model endpoint request failed: {}",
                error.kind()
            ));
        }
    };
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_MODEL_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("failed to read model endpoint response")?;
    parse_model_list(&bytes)
}

fn validate_api_key(api_key: &str) -> Result<()> {
    if api_key.is_empty()
        || api_key.len() > MAX_API_KEY_BYTES
        || api_key.trim() != api_key
        || api_key.chars().any(char::is_control)
    {
        bail!("provider API key is invalid");
    }
    Ok(())
}

fn validate_model_text(value: &str, field: &str, max_len: usize) -> Result<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        bail!("model endpoint returned an invalid {field}");
    }
    Ok(())
}
