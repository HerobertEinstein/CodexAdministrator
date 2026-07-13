use std::sync::Arc;

use anyhow::{Result, bail};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, put},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use serde::Deserialize;
use subtle::ConstantTimeEq;
use tokio::sync::RwLock;

use crate::{AgentMode, ModeState};

const UI_HTML: &str = include_str!("../assets/ui.html");
const UI_APP_JS: &str = include_str!("../assets/ui-app.js");

#[derive(Clone)]
pub struct CompanionContext {
    capability: Arc<str>,
    state: Arc<RwLock<ModeState>>,
}

impl CompanionContext {
    pub fn new(capability: &str) -> Result<Self> {
        let capability = capability.trim();
        if capability.len() < 16 {
            bail!("companion capability must contain at least 16 bytes");
        }
        if capability.len() > 512 {
            bail!("companion capability cannot exceed 512 bytes");
        }
        Ok(Self {
            capability: Arc::from(capability),
            state: Arc::new(RwLock::new(ModeState::default())),
        })
    }

    fn capability_matches(&self, candidate: &str) -> bool {
        let expected = self.capability.as_bytes();
        let candidate = candidate.as_bytes();
        expected.len() == candidate.len() && bool::from(expected.ct_eq(candidate))
    }
}

pub fn build_companion_router(context: CompanionContext) -> Router {
    Router::new()
        .route("/api/state", get(get_state))
        .route("/api/state/mode", put(set_mode))
        .route("/ui/", get(show_ui))
        .route("/ui/app.js", get(show_ui_app))
        .with_state(context)
}

#[derive(Debug, Deserialize)]
struct ModeUpdate {
    mode: AgentMode,
}

async fn get_state(
    State(context): State<CompanionContext>,
    authorization: Option<TypedHeader<Authorization<Bearer>>>,
) -> Response {
    if !is_authorized(&context, authorization.as_ref()) {
        return unauthorized();
    }
    Json(context.state.read().await.clone()).into_response()
}

async fn set_mode(
    State(context): State<CompanionContext>,
    authorization: Option<TypedHeader<Authorization<Bearer>>>,
    Json(update): Json<ModeUpdate>,
) -> Response {
    if !is_authorized(&context, authorization.as_ref()) {
        return unauthorized();
    }
    let mut state = context.state.write().await;
    state.set_mode(update.mode);
    Json(state.clone()).into_response()
}

async fn show_ui() -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; connect-src 'self'; object-src 'none'; base-uri 'none'"
            .parse()
            .unwrap(),
    );
    (headers, Html(UI_HTML)).into_response()
}

async fn show_ui_app() -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "text/javascript; charset=utf-8".parse().unwrap(),
    );
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    (headers, UI_APP_JS).into_response()
}

fn is_authorized(
    context: &CompanionContext,
    authorization: Option<&TypedHeader<Authorization<Bearer>>>,
) -> bool {
    authorization.is_some_and(|value| context.capability_matches(value.token()))
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
}
