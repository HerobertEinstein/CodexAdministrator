use std::{
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use http::Uri;
use serde::Deserialize;
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

use crate::DirectCdpTarget;

const HEALTH_EXPRESSION: &str = "(() => { try { return window.__codexAdministrator?.health?.() ?? null; } catch { return null; } })()";
const UI_READY_EXPRESSION: &str =
    "Boolean(document.body?.innerText?.trim() && document.querySelector('button'))";

pub struct LoopbackCdpClient {
    poll_interval: Duration,
    request_timeout: Duration,
}

impl Default for LoopbackCdpClient {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(50),
            request_timeout: Duration::from_millis(500),
        }
    }
}

impl LoopbackCdpClient {
    pub fn wait_for_endpoint(&self, port: u16, timeout: Duration) -> Result<()> {
        self.wait_until(timeout, || {
            self.get_json(port, "/json/version")
                .map(|_: Value| Some(()))
        })
        .map(|_| ())
    }

    pub fn wait_for_app_target(&self, port: u16, timeout: Duration) -> Result<DirectCdpTarget> {
        self.wait_until(timeout, || {
            let targets: Vec<RawCdpTarget> = self.get_json(port, "/json/list")?;
            let mut matches = targets.into_iter().filter_map(|target| {
                if target.kind != "page" || target.url != "app://-/index.html" {
                    return None;
                }
                Some(DirectCdpTarget {
                    id: target.id,
                    page_url: target.url,
                    websocket_url: target.websocket_url?,
                })
            });
            let target = match matches.next() {
                Some(target) => target,
                None => return Ok(None),
            };
            if matches.next().is_some() {
                bail!("isolated CDP port exposed multiple official app renderers");
            }
            target.validate_for_port(port)?;
            Ok(Some(target))
        })
    }

    pub fn install_bootstrap(
        &self,
        target: &DirectCdpTarget,
        script: &str,
        timeout: Duration,
    ) -> Result<()> {
        let port = target_port(target)?;
        target.validate_for_port(port)?;
        let mut session = CdpSession::connect(target, timeout.max(self.request_timeout))?;
        session.evaluate(script)?;

        let deadline = Instant::now() + timeout;
        loop {
            if health_is_ready(&session.evaluate(HEALTH_EXPRESSION)?) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("injected bridge did not become healthy before the deadline");
            }
            thread::sleep(
                self.poll_interval
                    .min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }

    pub fn injection_healthy(&self, target: &DirectCdpTarget) -> Result<bool> {
        let port = target_port(target)?;
        target.validate_for_port(port)?;
        let mut session = CdpSession::connect(target, self.request_timeout)?;
        Ok(health_is_ready(&session.evaluate(HEALTH_EXPRESSION)?))
    }

    pub fn wait_for_ui_ready(&self, target: &DirectCdpTarget, timeout: Duration) -> Result<()> {
        let port = target_port(target)?;
        target.validate_for_port(port)?;
        let mut session = CdpSession::connect(target, timeout.max(self.request_timeout))?;
        let deadline = Instant::now() + timeout;
        loop {
            if session.evaluate(UI_READY_EXPRESSION)?.as_bool() == Some(true) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("official app UI did not become interactive before the deadline");
            }
            thread::sleep(
                self.poll_interval
                    .min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }

    fn get_json<T: serde::de::DeserializeOwned>(&self, port: u16, path: &str) -> Result<T> {
        let url = format!("http://127.0.0.1:{port}{path}");
        let response = ureq::get(&url)
            .timeout(self.request_timeout)
            .call()
            .with_context(|| format!("isolated CDP endpoint is unavailable at {url}"))?;
        serde_json::from_reader(response.into_reader())
            .with_context(|| format!("isolated CDP endpoint returned invalid JSON at {url}"))
    }

    fn wait_until<T>(
        &self,
        timeout: Duration,
        mut probe: impl FnMut() -> Result<Option<T>>,
    ) -> Result<T> {
        let deadline = Instant::now() + timeout;
        let mut last_error = None;
        loop {
            match probe() {
                Ok(Some(value)) => return Ok(value),
                Ok(None) => {}
                Err(error) => last_error = Some(error),
            }
            if Instant::now() >= deadline {
                if let Some(error) = last_error {
                    return Err(error);
                }
                bail!("isolated CDP target did not become available before the deadline");
            }
            thread::sleep(
                self.poll_interval
                    .min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawCdpTarget {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    websocket_url: Option<String>,
}

struct CdpSession {
    socket: WebSocket<TcpStream>,
    next_id: u64,
}

impl CdpSession {
    fn connect(target: &DirectCdpTarget, timeout: Duration) -> Result<Self> {
        let port = target_port(target)?;
        target.validate_for_port(port)?;
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        let stream = TcpStream::connect_timeout(&address.into(), timeout)
            .context("failed to connect to the isolated CDP websocket")?;
        stream
            .set_read_timeout(Some(timeout))
            .context("failed to set isolated CDP read timeout")?;
        stream
            .set_write_timeout(Some(timeout))
            .context("failed to set isolated CDP write timeout")?;
        let (socket, response) = tungstenite::client(target.websocket_url.as_str(), stream)
            .context("isolated CDP websocket handshake failed")?;
        if response.status().as_u16() != 101 {
            bail!("isolated CDP websocket returned a non-switching response");
        }
        Ok(Self { socket, next_id: 1 })
    }

    fn evaluate(&mut self, expression: &str) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.socket
            .send(Message::Text(
                json!({
                    "id": id,
                    "method": "Runtime.evaluate",
                    "params": {
                        "expression": expression,
                        "awaitPromise": true,
                        "returnByValue": true
                    }
                })
                .to_string()
                .into(),
            ))
            .context("failed to send isolated CDP Runtime.evaluate")?;

        loop {
            let message = self
                .socket
                .read()
                .context("failed to read isolated CDP Runtime.evaluate response")?;
            if message.is_ping() {
                self.socket
                    .send(Message::Pong(message.into_data()))
                    .context("failed to answer isolated CDP ping")?;
                continue;
            }
            let Some(text) = message.to_text().ok() else {
                continue;
            };
            let response: Value = serde_json::from_str(text)
                .context("isolated CDP websocket returned invalid JSON")?;
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown CDP protocol error");
                bail!("isolated CDP Runtime.evaluate failed: {message}");
            }
            if let Some(exception) = response
                .pointer("/result/exceptionDetails")
                .filter(|value| !value.is_null())
            {
                let message = exception
                    .get("text")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        exception
                            .pointer("/exception/description")
                            .and_then(Value::as_str)
                    })
                    .unwrap_or("unknown renderer exception");
                bail!("isolated renderer evaluation failed: {message}");
            }
            return Ok(response
                .pointer("/result/result/value")
                .cloned()
                .unwrap_or(Value::Null));
        }
    }
}

fn health_is_ready(value: &Value) -> bool {
    value.get("ok").and_then(Value::as_bool) == Some(true)
        && value.get("provider").and_then(Value::as_str) == Some("grok_native")
}

fn target_port(target: &DirectCdpTarget) -> Result<u16> {
    let uri: Uri = target
        .websocket_url
        .parse()
        .context("direct CDP websocket URL is invalid")?;
    uri.port_u16()
        .ok_or_else(|| anyhow::anyhow!("direct CDP websocket URL requires an explicit port"))
}
