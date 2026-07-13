use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::{
    JsonlTransport,
    protocol::{codex, grok},
};

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

#[derive(Clone)]
pub struct GrokAcpClient {
    transport: JsonlTransport,
    request_timeout: Duration,
    next_id: Arc<AtomicU64>,
    initialized: Arc<AtomicBool>,
}

impl GrokAcpClient {
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
            bail!("Grok ACP runtime is already initialized");
        }
        let response = self
            .transport
            .request(
                grok::initialize_request(self.next_request_id("initialize")),
                self.request_timeout,
            )
            .await?;
        let result = require_success(&response, "Grok initialize")?;
        if result.get("protocolVersion").and_then(Value::as_u64) != Some(1) {
            bail!("Grok initialize response did not negotiate ACP protocol version 1");
        }
        self.initialized.store(true, Ordering::Release);
        Ok(response)
    }

    pub async fn new_session(&self, cwd: &str) -> Result<String> {
        self.require_initialized()?;
        let response = self
            .transport
            .request(
                grok::session_new_request(self.next_request_id("session-new"), cwd)?,
                self.request_timeout,
            )
            .await?;
        require_success(&response, "Grok session/new")?;
        grok::parse_session_id(&response)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("Grok session/new response omitted result.sessionId"))
    }

    pub async fn load_session(&self, session_id: &str, cwd: &str) -> Result<()> {
        self.require_initialized()?;
        let response = self
            .transport
            .request(
                grok::session_load_request(self.next_request_id("session-load"), session_id, cwd)?,
                self.request_timeout,
            )
            .await?;
        require_success(&response, "Grok session/load")?;
        Ok(())
    }

    pub async fn prompt_text(&self, session_id: &str, text: &str) -> Result<String> {
        self.require_initialized()?;
        let response = self
            .transport
            .request(
                grok::session_prompt_text_request(
                    self.next_request_id("session-prompt"),
                    session_id,
                    text,
                ),
                self.request_timeout,
            )
            .await?;
        let result = require_success(&response, "Grok session/prompt")?;
        result
            .get("stopReason")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("Grok session/prompt response omitted stopReason"))
    }

    pub async fn cancel(&self, session_id: &str) -> Result<()> {
        self.require_initialized()?;
        self.transport
            .notify(grok::session_cancel_notification(session_id))
            .await
    }

    pub async fn select_permission(&self, id: Value, option_id: &str) -> Result<()> {
        self.require_initialized()?;
        self.transport
            .respond(grok::permission_selected_response(id, option_id)?)
            .await
    }

    pub async fn cancel_permission(&self, id: Value) -> Result<()> {
        self.require_initialized()?;
        self.transport
            .respond(grok::permission_cancelled_response(id))
            .await
    }

    fn next_request_id(&self, scope: &str) -> Value {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        Value::String(format!("ca:{scope}:{id}"))
    }

    fn require_initialized(&self) -> Result<()> {
        if !self.initialized.load(Ordering::Acquire) {
            bail!("Grok ACP runtime must initialize before this operation");
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
