use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::{JsonlTransport, protocol::codex};

pub use crate::protocol::codex::ApprovalDecision as CodexApprovalDecision;

#[derive(Clone)]
pub struct CodexAppServerClient {
    transport: JsonlTransport,
    request_timeout: Duration,
    next_id: Arc<AtomicU64>,
    initialized: Arc<AtomicBool>,
}

impl CodexAppServerClient {
    pub fn new(transport: JsonlTransport, request_timeout: Duration) -> Self {
        Self {
            transport,
            request_timeout,
            next_id: Arc::new(AtomicU64::new(1)),
            initialized: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn initialize(&self) -> Result<Value> {
        if self.initialized.load(Ordering::Acquire) {
            bail!("Codex app-server is already initialized");
        }
        let response = self
            .transport
            .request(
                codex::initialize_request(
                    self.next_request_id(),
                    codex::ClientInfo {
                        name: "codex_administrator",
                        title: "Codex Administrator",
                        version: env!("CARGO_PKG_VERSION"),
                    },
                ),
                self.request_timeout,
            )
            .await?;
        require_success(&response, "Codex initialize")?;
        self.transport
            .notify(codex::initialized_notification())
            .await?;
        self.initialized.store(true, Ordering::Release);
        Ok(response)
    }

    pub async fn start_thread(&self) -> Result<String> {
        self.require_initialized()?;
        let response = self
            .transport
            .request(
                codex::thread_start_request(self.next_request_id()),
                self.request_timeout,
            )
            .await?;
        require_success(&response, "Codex thread/start")?;
        codex::parse_thread_id(&response)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("Codex thread/start response omitted result.thread.id"))
    }

    pub async fn initialize_with_experimental_api(&self) -> Result<Value> {
        if self.initialized.load(Ordering::Acquire) {
            bail!("Codex app-server is already initialized");
        }
        let response = self
            .transport
            .request(
                codex::initialize_request_with_experimental_api(
                    self.next_request_id(),
                    codex::ClientInfo {
                        name: "codex_administrator",
                        title: "Codex Administrator",
                        version: env!("CARGO_PKG_VERSION"),
                    },
                ),
                self.request_timeout,
            )
            .await?;
        require_success(&response, "Codex initialize")?;
        self.transport
            .notify(codex::initialized_notification())
            .await?;
        self.initialized.store(true, Ordering::Release);
        Ok(response)
    }

    pub async fn start_thread_with_model(
        &self,
        model_provider: &str,
        model: &str,
        cwd: &str,
    ) -> Result<String> {
        self.require_initialized()?;
        for (name, value) in [
            ("model provider", model_provider),
            ("model", model),
            ("working directory", cwd),
        ] {
            if value.trim().is_empty() || value.chars().any(char::is_control) {
                bail!("Codex {name} cannot be blank or contain control characters");
            }
        }
        let response = self
            .transport
            .request(
                codex::thread_start_with_model_request(
                    self.next_request_id(),
                    model_provider,
                    model,
                    cwd,
                ),
                self.request_timeout,
            )
            .await?;
        require_success(&response, "Codex thread/start")?;
        codex::parse_thread_id(&response)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("Codex thread/start response omitted result.thread.id"))
    }

    pub async fn start_thread_with_model_and_controls(
        &self,
        model_provider: &str,
        model: &str,
        cwd: &str,
        approval_policy: &str,
        sandbox: &str,
    ) -> Result<String> {
        self.require_initialized()?;
        for (name, value) in [
            ("model provider", model_provider),
            ("model", model),
            ("working directory", cwd),
            ("approval policy", approval_policy),
            ("sandbox", sandbox),
        ] {
            if value.trim().is_empty() || value.chars().any(char::is_control) {
                bail!("Codex {name} cannot be blank or contain control characters");
            }
        }
        let response = self
            .transport
            .request(
                codex::thread_start_with_model_and_controls_request(
                    self.next_request_id(),
                    model_provider,
                    model,
                    cwd,
                    approval_policy,
                    sandbox,
                ),
                self.request_timeout,
            )
            .await?;
        require_success(&response, "Codex thread/start")?;
        codex::parse_thread_id(&response)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("Codex thread/start response omitted result.thread.id"))
    }

    pub async fn resume_thread(&self, thread_id: &str) -> Result<String> {
        self.require_initialized()?;
        let response = self
            .transport
            .request(
                codex::thread_resume_request(self.next_request_id(), thread_id),
                self.request_timeout,
            )
            .await?;
        require_success(&response, "Codex thread/resume")?;
        codex::parse_thread_id(&response)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("Codex thread/resume response omitted result.thread.id"))
    }

    pub async fn start_text_turn(&self, thread_id: &str, text: &str) -> Result<String> {
        self.require_initialized()?;
        let response = self
            .transport
            .request(
                codex::turn_start_text_request(self.next_request_id(), thread_id, text),
                self.request_timeout,
            )
            .await?;
        require_success(&response, "Codex turn/start")?;
        codex::parse_turn_id(&response)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("Codex turn/start response omitted result.turn.id"))
    }

    pub async fn start_text_and_local_image_turn(
        &self,
        thread_id: &str,
        text: &str,
        image_path: &str,
    ) -> Result<String> {
        self.require_initialized()?;
        if image_path.trim().is_empty() || image_path.chars().any(char::is_control) {
            bail!("Codex local image path cannot be blank or contain control characters");
        }
        let response = self
            .transport
            .request(
                codex::turn_start_text_and_local_image_request(
                    self.next_request_id(),
                    thread_id,
                    text,
                    image_path,
                ),
                self.request_timeout,
            )
            .await?;
        require_success(&response, "Codex turn/start")?;
        codex::parse_turn_id(&response)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("Codex turn/start response omitted result.turn.id"))
    }

    pub async fn start_text_turn_with_workspace_controls(
        &self,
        thread_id: &str,
        text: &str,
        writable_root: &str,
    ) -> Result<String> {
        self.require_initialized()?;
        if writable_root.trim().is_empty() || writable_root.chars().any(char::is_control) {
            bail!("Codex writable root cannot be blank or contain control characters");
        }
        let response = self
            .transport
            .request(
                codex::turn_start_text_with_workspace_controls_request(
                    self.next_request_id(),
                    thread_id,
                    text,
                    writable_root,
                ),
                self.request_timeout,
            )
            .await?;
        require_success(&response, "Codex turn/start")?;
        codex::parse_turn_id(&response)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("Codex turn/start response omitted result.turn.id"))
    }

    pub async fn interrupt_turn(&self, thread_id: &str, turn_id: &str) -> Result<()> {
        self.require_initialized()?;
        let response = self
            .transport
            .request(
                codex::turn_interrupt_request(self.next_request_id(), thread_id, turn_id),
                self.request_timeout,
            )
            .await?;
        require_success(&response, "Codex turn/interrupt")?;
        Ok(())
    }

    pub async fn respond_approval(&self, id: Value, decision: CodexApprovalDecision) -> Result<()> {
        self.require_initialized()?;
        self.transport
            .respond(codex::approval_response(id, decision))
            .await
    }

    fn next_request_id(&self) -> Value {
        json!(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    fn require_initialized(&self) -> Result<()> {
        if !self.initialized.load(Ordering::Acquire) {
            bail!("Codex app-server must initialize before this operation");
        }
        Ok(())
    }
}

fn require_success<'a>(response: &'a Value, operation: &str) -> Result<&'a Value> {
    if let Some(error) = response.get("error") {
        bail!("{operation} failed: {error}");
    }
    response
        .get("result")
        .ok_or_else(|| anyhow::anyhow!("{operation} response omitted result"))
}
