use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::{
    ControlOperation, ControlRequest, ControlResponse, CredentialStore, DEFAULT_GROK_ACTION_PATH,
    DiscoveredModel, LauncherSettings, RendererAddonSettings, bind_provider_credential,
    is_reviewed_grok_model_id, resolve_bound_provider_credential, save_launcher_settings,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscoverPayload {
    base_url: String,
    action_path: String,
    action_path_auto: bool,
    #[serde(default)]
    credential: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyPayload {
    base_url: String,
    action_path: String,
    action_path_auto: bool,
    selected_models: Vec<String>,
    #[serde(default)]
    renderer_addons: Option<Vec<RendererAddonSettings>>,
    sync_native_auth: bool,
    sync_native_sessions: bool,
}

pub struct GrokControlOutcome {
    pub response: ControlResponse,
    pub restart_required: bool,
}

struct PendingCredential {
    action_path: String,
    base_url: String,
    secret: Zeroizing<String>,
}

impl PendingCredential {
    fn new(secret: Zeroizing<String>, settings: &LauncherSettings) -> Self {
        Self {
            action_path: settings.action_path.clone(),
            base_url: settings.base_url.clone(),
            secret,
        }
    }

    fn matches(&self, settings: &LauncherSettings) -> bool {
        self.base_url == settings.base_url && self.action_path == settings.action_path
    }
}

pub struct GrokControlBroker {
    nonce: String,
    settings: LauncherSettings,
    credential_present: bool,
    pending_credential: Option<PendingCredential>,
    settings_path: PathBuf,
}

impl GrokControlBroker {
    pub fn new(
        nonce: impl Into<String>,
        settings: LauncherSettings,
        credential_present: bool,
        settings_path: PathBuf,
    ) -> Result<Self> {
        let nonce = nonce.into();
        if nonce.len() != 64 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("control broker nonce is invalid");
        }
        settings.validate()?;
        if !settings_path.is_absolute() {
            bail!("control broker settings path must be absolute");
        }
        Ok(Self {
            nonce,
            settings,
            credential_present,
            pending_credential: None,
            settings_path,
        })
    }

    pub fn handle<S, F>(
        &mut self,
        request: ControlRequest,
        credential_store: &S,
        discover: F,
    ) -> GrokControlOutcome
    where
        S: CredentialStore,
        F: FnOnce(&str, &str) -> Result<Vec<DiscoveredModel>>,
    {
        let (id, operation, payload) = request.into_parts();
        let result = match operation {
            ControlOperation::StateRead => self.state_read(payload),
            ControlOperation::ModelsDiscover => {
                self.models_discover(payload, credential_store, discover)
            }
            ControlOperation::ConfigApply => self.config_apply(payload, credential_store),
            ControlOperation::CredentialClear => self.credential_clear(payload, credential_store),
        };
        match result {
            Ok((value, restart_required)) => GrokControlOutcome {
                response: ControlResponse::success(id, self.nonce.clone(), value),
                restart_required,
            },
            Err(error) => GrokControlOutcome {
                response: ControlResponse::error(id, self.nonce.clone(), &error.to_string()),
                restart_required: false,
            },
        }
    }

    fn state_read(&self, payload: Value) -> Result<(Value, bool)> {
        require_empty_payload(payload)?;
        Ok((self.state_value(), false))
    }

    fn models_discover<S, F>(
        &mut self,
        payload: Value,
        credential_store: &S,
        discover: F,
    ) -> Result<(Value, bool)>
    where
        S: CredentialStore,
        F: FnOnce(&str, &str) -> Result<Vec<DiscoveredModel>>,
    {
        let payload: DiscoverPayload =
            serde_json::from_value(payload).context("model discovery payload is invalid")?;
        let action_path = if payload.action_path_auto {
            DEFAULT_GROK_ACTION_PATH.to_string()
        } else {
            payload.action_path
        };
        let mut candidate = self.settings.clone();
        candidate.base_url = payload.base_url;
        candidate.action_path = action_path;
        candidate.action_path_auto = payload.action_path_auto;
        candidate.selected_models.clear();
        candidate.cached_models.clear();
        candidate.validate()?;

        let fresh_credential = if payload.credential.is_empty() {
            None
        } else {
            Some(Zeroizing::new(payload.credential))
        };
        let stored_credential = if fresh_credential.is_none() && self.pending_credential.is_none() {
            if !same_provider_endpoint(&candidate, &self.settings) {
                bail!("changing the provider endpoint requires a fresh API key");
            }
            let stored = credential_store.read()?.map(Zeroizing::new);
            match stored.as_deref() {
                Some(stored) => resolve_bound_provider_credential(
                    &candidate.base_url,
                    &candidate.action_path,
                    stored,
                )?
                .map(Zeroizing::new),
                None => None,
            }
        } else {
            None
        };
        let credential = if let Some(credential) = fresh_credential.as_deref() {
            credential
        } else if let Some(credential) = self.pending_credential.as_ref() {
            if !credential.matches(&candidate) {
                bail!("the pending provider API key belongs to a different endpoint");
            }
            credential.secret.as_str()
        } else {
            stored_credential
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("provider API key is required"))?
        };

        let models = discover(&candidate.base_url, credential)?;
        candidate.cached_models = models
            .into_iter()
            .filter(|model| is_reviewed_grok_model_id(&model.id))
            .collect();
        if candidate.cached_models.is_empty() {
            bail!("model endpoint returned no Grok models with reviewed capability profiles");
        }
        candidate.selected_models = candidate
            .cached_models
            .iter()
            .map(|model| model.id.clone())
            .collect();
        candidate.validate()?;

        if let Some(credential) = fresh_credential {
            self.pending_credential = Some(PendingCredential::new(credential, &candidate));
        }
        self.credential_present = true;
        self.settings = candidate;
        Ok((self.state_value(), false))
    }

    fn config_apply<S: CredentialStore>(
        &mut self,
        payload: Value,
        credential_store: &S,
    ) -> Result<(Value, bool)> {
        let payload: ApplyPayload =
            serde_json::from_value(payload).context("provider apply payload is invalid")?;
        let mut candidate = self.settings.clone();
        candidate.base_url = payload.base_url;
        candidate.action_path = payload.action_path;
        candidate.action_path_auto = payload.action_path_auto;
        candidate.selected_models = payload.selected_models;
        if let Some(renderer_addons) = payload.renderer_addons {
            candidate.renderer_addons = renderer_addons;
        }
        candidate.sync_native_auth = payload.sync_native_auth;
        candidate.sync_native_sessions = payload.sync_native_sessions;
        candidate.validate()?;

        if let Some(credential) = self.pending_credential.as_ref()
            && !credential.matches(&candidate)
        {
            bail!("the pending provider API key belongs to a different endpoint");
        }
        if !same_provider_endpoint(&candidate, &self.settings) && self.pending_credential.is_none()
        {
            bail!("changing the provider endpoint requires a freshly verified API key");
        }

        let previous_credential = if self.pending_credential.is_some() {
            credential_store.read()?.map(Zeroizing::new)
        } else {
            None
        };
        let credential_written = if let Some(credential) = self.pending_credential.as_ref() {
            let stored = Zeroizing::new(bind_provider_credential(
                &candidate.base_url,
                &candidate.action_path,
                credential.secret.as_str(),
            )?);
            credential_store.write(stored.as_str())?;
            true
        } else {
            false
        };
        if let Err(settings_error) = save_launcher_settings(&self.settings_path, &candidate) {
            if credential_written {
                let rollback = match previous_credential.as_deref() {
                    Some(previous) => credential_store.write(previous),
                    None => credential_store.delete().map(|_| ()),
                };
                if let Err(rollback_error) = rollback {
                    return Err(anyhow::anyhow!(
                        "launcher settings update failed and credential rollback failed: {settings_error}; {rollback_error}"
                    ));
                }
            }
            return Err(settings_error);
        }
        if credential_written {
            self.pending_credential.take();
            self.credential_present = true;
        }
        self.settings = candidate;
        Ok((json!({ "restart_required": true }), true))
    }

    fn credential_clear<S: CredentialStore>(
        &mut self,
        payload: Value,
        credential_store: &S,
    ) -> Result<(Value, bool)> {
        require_empty_payload(payload)?;
        self.pending_credential = None;
        credential_store.delete()?;
        self.credential_present = false;
        Ok((self.state_value(), true))
    }

    fn state_value(&self) -> Value {
        json!({
            "model_picker": {
                "actionPath": self.settings.action_path,
                "actionPathAuto": self.settings.action_path_auto,
                "baseUrl": self.settings.base_url,
                "credentialPresent": self.credential_present,
                "rendererAddons": self.settings.renderer_addons,
                "syncNativeAuth": self.settings.sync_native_auth,
                "syncNativeSessions": self.settings.sync_native_sessions,
            },
            "models": self.settings.cached_models,
            "selected_models": self.settings.selected_models,
        })
    }
}

fn same_provider_endpoint(left: &LauncherSettings, right: &LauncherSettings) -> bool {
    left.base_url == right.base_url && left.action_path == right.action_path
}

fn require_empty_payload(payload: Value) -> Result<()> {
    if payload.as_object().is_some_and(serde_json::Map::is_empty) {
        Ok(())
    } else {
        bail!("control operation payload must be empty")
    }
}
