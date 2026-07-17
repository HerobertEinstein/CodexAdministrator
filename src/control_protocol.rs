use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const CONTROL_VERSION: u8 = 1;
const MAX_CONTROL_REQUESTS: usize = 32;
const MAX_CONTROL_PAYLOAD_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlOperation {
    ConfigApply,
    CredentialClear,
    ModelsDiscover,
    StateRead,
}

impl ControlOperation {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "config.apply" => Ok(Self::ConfigApply),
            "credential.clear" => Ok(Self::CredentialClear),
            "models.discover" => Ok(Self::ModelsDiscover),
            "state.read" => Ok(Self::StateRead),
            _ => bail!("control request operation is unsupported"),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawControlRequest {
    version: u8,
    id: String,
    nonce: String,
    operation: String,
    payload: Value,
}

pub struct ControlRequest {
    id: String,
    operation: ControlOperation,
    payload: Value,
}

impl ControlRequest {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn operation(&self) -> ControlOperation {
        self.operation
    }

    pub fn into_payload(self) -> Value {
        self.payload
    }

    pub fn into_parts(self) -> (String, ControlOperation, Value) {
        (self.id, self.operation, self.payload)
    }
}

pub fn parse_control_requests(
    value: &mut Value,
    expected_nonce: &str,
) -> Result<Vec<ControlRequest>> {
    if expected_nonce.len() != 64 || !expected_nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("control nonce is invalid");
    }
    let taken = value.take();
    let requests = match taken {
        Value::Array(requests) => requests,
        _ => bail!("control drain result must be an array"),
    };
    if requests.len() > MAX_CONTROL_REQUESTS {
        bail!("control drain returned too many requests");
    }

    let mut parsed = Vec::with_capacity(requests.len());
    for request in requests {
        let raw: RawControlRequest =
            serde_json::from_value(request).context("control request has an invalid shape")?;
        if raw.version != CONTROL_VERSION {
            bail!("control request version is unsupported");
        }
        if raw.nonce != expected_nonce {
            bail!("control request nonce does not match the isolated instance");
        }
        if raw.id.is_empty()
            || raw.id.len() > 64
            || !raw
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            bail!("control request id is invalid");
        }
        if serde_json::to_vec(&raw.payload)?.len() > MAX_CONTROL_PAYLOAD_BYTES {
            bail!("control request payload is too large");
        }
        parsed.push(ControlRequest {
            id: raw.id,
            operation: ControlOperation::parse(&raw.operation)?,
            payload: raw.payload,
        });
    }
    Ok(parsed)
}

#[derive(Serialize)]
pub struct ControlResponse {
    version: u8,
    id: String,
    nonce: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl ControlResponse {
    pub fn success(id: impl Into<String>, nonce: impl Into<String>, result: Value) -> Self {
        Self {
            version: CONTROL_VERSION,
            id: id.into(),
            nonce: nonce.into(),
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: impl Into<String>, nonce: impl Into<String>, error: &str) -> Self {
        let mut error = error
            .chars()
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .take(512)
            .collect::<String>();
        if error.trim().is_empty() {
            error = "secure broker request failed".into();
        }
        Self {
            version: CONTROL_VERSION,
            id: id.into(),
            nonce: nonce.into(),
            ok: false,
            result: None,
            error: Some(error),
        }
    }

    pub fn into_value(self) -> Value {
        serde_json::to_value(self).expect("control response serialization is infallible")
    }
}
