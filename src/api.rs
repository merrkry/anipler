use std::sync::Arc;

use axum::response::IntoResponse;
use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
    routing::{get, post},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tower_http::trace::TraceLayer;
use tracing::instrument;

use crate::task::ArtifactInfo;
use crate::{
    config::DaemonConfig,
    storage::{FinalizeArtifactError, StorageManager},
};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Unauthorized")]
    Unauthorized,
    #[error("Artifact not found")]
    NotFound,
    #[error("Artifact already archived")]
    AlreadyArchived,
    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::AlreadyArchived => StatusCode::CONFLICT,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, self.to_string()).into_response()
    }
}

#[derive(Clone)]
pub struct ApiState {
    pub store: Arc<StorageManager>,
    pub api_key: String,
}

fn auth(state: &ApiState, request: &Request) -> Result<(), ApiError> {
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or(ApiError::Unauthorized)?;

    if state.api_key == token {
        Ok(())
    } else {
        Err(ApiError::Unauthorized)
    }
}

#[instrument(skip(state, request))]
async fn list_artifacts(
    State(state): State<ApiState>,
    request: Request,
) -> Result<Json<Vec<ArtifactInfo>>, ApiError> {
    auth(&state, &request)?;

    tracing::info!("Requested list of ready artifacts");

    let artifacts = state.store.list_ready_artifacts().await.map_err(|e| {
        tracing::error!(error = %e, "Failed to list artifacts");
        ApiError::Internal(e.to_string())
    })?;

    Ok(Json(artifacts))
}

#[instrument(skip(state, request))]
async fn confirm_artifact(
    State(state): State<ApiState>,
    axum::extract::Path(hash): axum::extract::Path<String>,
    request: Request,
) -> Result<StatusCode, ApiError> {
    auth(&state, &request)?;

    tracing::info!("Confirming artifact transfer");

    match state.store.finalize_artifact(&hash).await {
        Ok(()) => Ok(StatusCode::OK),
        Err(FinalizeArtifactError::NotFound) => {
            tracing::warn!(hash = %hash, "Artifact not found");
            Err(ApiError::NotFound)
        }
        Err(FinalizeArtifactError::AlreadyArchived) => {
            tracing::warn!(hash = %hash, "Artifact already archived");
            Err(ApiError::AlreadyArchived)
        }
        Err(FinalizeArtifactError::Storage(e)) => {
            tracing::error!(error = %e, hash = %hash, "Failed to finalize artifact");
            Err(ApiError::Internal(e.to_string()))
        }
    }
}

struct ApiServerInner {
    addr: std::net::SocketAddr,
}

#[derive(Clone)]
pub struct ApiServer {
    api_state: ApiState,
    inner: Arc<ApiServerInner>,
}

impl ApiServer {
    /// Create a new API server from the given configuration.
    pub fn from_config(config: &DaemonConfig, store: Arc<StorageManager>) -> Self {
        let api_state = ApiState {
            store,
            api_key: config.api_key.clone(),
        };

        let addr = config.api_addr;

        let inner = ApiServerInner { addr };

        Self {
            api_state,
            inner: Arc::new(inner),
        }
    }

    /// Run the API server, returning a handle to the spawned task.
    ///
    /// # Errors
    ///
    /// The outer `Result` indicates if the server was successfully started.
    /// The inner `Result` indicates graceful shutdown or undocumented errors from axum.
    pub fn run(&self, cancel: CancellationToken) -> anyhow::Result<JoinHandle<anyhow::Result<()>>> {
        let inner = self.inner.clone();
        let api_state = self.api_state.clone();

        let handle = tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(&inner.addr).await?;
            tracing::info!(address = %inner.addr, "API server listening");

            let router = Router::new()
                .route("/api/artifacts", get(list_artifacts))
                .route("/api/artifacts/{hash}/confirm", post(confirm_artifact))
                .with_state(api_state)
                .layer(TraceLayer::new_for_http());

            let server = axum::serve(listener, router).with_graceful_shutdown(async move {
                cancel.cancelled().await;
            });

            server
                .await
                .map_err(|e| anyhow::anyhow!("API server error: {e}"))
        });

        Ok(handle)
    }
}
