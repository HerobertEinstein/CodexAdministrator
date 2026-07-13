use std::{
    collections::{HashMap, hash_map::Entry},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::{Mutex, mpsc, oneshot},
};

type PendingResult = std::result::Result<Value, String>;
type PendingRequests = Arc<Mutex<HashMap<String, oneshot::Sender<PendingResult>>>>;
type SharedWriter = Arc<Mutex<Box<dyn AsyncWrite + Unpin + Send>>>;

#[derive(Debug, Clone, PartialEq)]
pub enum JsonlEvent {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    OrphanResponse {
        message: Value,
    },
    ProtocolError {
        message: String,
    },
    Closed {
        reason: String,
    },
}

#[derive(Clone)]
pub struct JsonlTransport {
    writer: SharedWriter,
    pending: PendingRequests,
    max_line_bytes: usize,
}

impl JsonlTransport {
    pub fn spawn<R, W>(
        reader: R,
        writer: W,
        max_line_bytes: usize,
    ) -> (Self, mpsc::UnboundedReceiver<JsonlEvent>)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        tokio::spawn(read_loop(
            reader,
            pending.clone(),
            event_tx,
            max_line_bytes.max(1),
        ));
        (
            Self {
                writer: Arc::new(Mutex::new(Box::new(writer))),
                pending,
                max_line_bytes: max_line_bytes.max(1),
            },
            event_rx,
        )
    }

    pub async fn request(&self, message: Value, timeout: Duration) -> Result<Value> {
        let id = message
            .get("id")
            .ok_or_else(|| anyhow::anyhow!("outbound request id is required"))?;
        if message.get("method").and_then(Value::as_str).is_none() {
            bail!("outbound request method is required");
        }
        let key = request_id_key(id)?;
        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            match pending.entry(key.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(sender);
                }
                Entry::Occupied(_) => bail!("duplicate in-flight request id"),
            }
        }

        if let Err(error) = self.write_message(&message).await {
            self.pending.lock().await.remove(&key);
            return Err(error);
        }

        match tokio::time::timeout(timeout, receiver).await {
            Err(_) => {
                self.pending.lock().await.remove(&key);
                bail!("request timed out")
            }
            Ok(Err(_)) => bail!("JSONL transport closed before the request completed"),
            Ok(Ok(Err(error))) => bail!(error),
            Ok(Ok(Ok(response))) => Ok(response),
        }
    }

    pub async fn notify(&self, message: Value) -> Result<()> {
        if message.get("method").and_then(Value::as_str).is_none() {
            bail!("outbound notification method is required");
        }
        if message.get("id").is_some() {
            bail!("outbound notification must not contain an id");
        }
        self.write_message(&message).await
    }

    pub async fn respond(&self, message: Value) -> Result<()> {
        let id = message
            .get("id")
            .ok_or_else(|| anyhow::anyhow!("outbound response id is required"))?;
        request_id_key(id)?;
        if message.get("method").is_some() {
            bail!("outbound response must not contain a method");
        }
        if message.get("result").is_none() && message.get("error").is_none() {
            bail!("outbound response must contain result or error");
        }
        self.write_message(&message).await
    }

    async fn write_message(&self, message: &Value) -> Result<()> {
        let encoded = serde_json::to_vec(message).context("failed to serialize JSONL message")?;
        if encoded.len() > self.max_line_bytes {
            bail!("outbound JSONL message exceeds the configured line limit");
        }
        let mut writer = self.writer.lock().await;
        writer
            .write_all(&encoded)
            .await
            .context("failed to write JSONL message")?;
        writer
            .write_all(b"\n")
            .await
            .context("failed to terminate JSONL message")?;
        writer
            .flush()
            .await
            .context("failed to flush JSONL message")
    }
}

async fn read_loop<R>(
    reader: R,
    pending: PendingRequests,
    events: mpsc::UnboundedSender<JsonlEvent>,
    max_line_bytes: usize,
) where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let read = match reader.read_until(b'\n', &mut buffer).await {
            Ok(read) => read,
            Err(error) => {
                close_transport(
                    &pending,
                    &events,
                    format!("failed to read JSONL stream: {error}"),
                )
                .await;
                return;
            }
        };
        if read == 0 {
            close_transport(&pending, &events, "JSONL stream reached EOF".into()).await;
            return;
        }
        if buffer.len() > max_line_bytes.saturating_add(1) {
            close_transport(
                &pending,
                &events,
                "inbound JSONL message exceeds the configured line limit".into(),
            )
            .await;
            return;
        }
        if buffer.last() == Some(&b'\n') {
            buffer.pop();
        }
        if buffer.last() == Some(&b'\r') {
            buffer.pop();
        }
        let message: Value = match serde_json::from_slice(&buffer) {
            Ok(message) => message,
            Err(error) => {
                let _ = events.send(JsonlEvent::ProtocolError {
                    message: format!("invalid JSONL message: {error}"),
                });
                continue;
            }
        };
        dispatch_message(message, &pending, &events).await;
    }
}

async fn dispatch_message(
    message: Value,
    pending: &PendingRequests,
    events: &mpsc::UnboundedSender<JsonlEvent>,
) {
    let id = message.get("id").cloned();
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(method) = method {
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let event = match id {
            Some(id) => JsonlEvent::Request { id, method, params },
            None => JsonlEvent::Notification { method, params },
        };
        let _ = events.send(event);
        return;
    }

    if let Some(id) = id
        && (message.get("result").is_some() || message.get("error").is_some())
    {
        match request_id_key(&id) {
            Ok(key) => {
                let sender = pending.lock().await.remove(&key);
                match sender {
                    Some(sender) => {
                        let _ = sender.send(Ok(message));
                    }
                    None => {
                        let _ = events.send(JsonlEvent::OrphanResponse { message });
                    }
                }
            }
            Err(error) => {
                let _ = events.send(JsonlEvent::ProtocolError {
                    message: error.to_string(),
                });
            }
        }
        return;
    }

    let _ = events.send(JsonlEvent::ProtocolError {
        message: "JSONL message is neither a request, notification, nor response".into(),
    });
}

async fn close_transport(
    pending: &PendingRequests,
    events: &mpsc::UnboundedSender<JsonlEvent>,
    reason: String,
) {
    let senders = pending
        .lock()
        .await
        .drain()
        .map(|(_, sender)| sender)
        .collect::<Vec<_>>();
    for sender in senders {
        let _ = sender.send(Err(reason.clone()));
    }
    let _ = events.send(JsonlEvent::Closed { reason });
}

fn request_id_key(id: &Value) -> Result<String> {
    match id {
        Value::String(value) => Ok(format!("s:{value}")),
        Value::Number(value) if value.is_i64() || value.is_u64() => Ok(format!("n:{value}")),
        _ => bail!("request id must be a string or integer"),
    }
}
