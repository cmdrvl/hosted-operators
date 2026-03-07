#![forbid(unsafe_code)]

use std::env;
use std::fmt;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::DefaultBodyLimit;
use axum::extract::State;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tempfile::NamedTempFile;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub const DEFAULT_KOVREX_FILE_API: &str = "https://gateway.kovrex.ai/v1/files";
const DEFAULT_BODY_MB: usize = 50;

#[derive(Clone, Debug)]
pub struct HostConfig {
    pub bind: String,
    pub port: u16,
    pub api_token: Option<String>,
    pub allowed_origins: Vec<String>,
    pub max_body_bytes: usize,
    pub kovrex_file_api: String,
}

impl HostConfig {
    pub fn from_env() -> Self {
        let port = env::var("CMDRVL_HOST_PORT")
            .ok()
            .or_else(|| env::var("PORT").ok())
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(8080);
        let max_body_mb = env::var("CMDRVL_HOST_MAX_BODY_MB")
            .ok()
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(DEFAULT_BODY_MB);
        let allowed_origins = env::var("CMDRVL_HOST_ALLOWED_ORIGINS")
            .ok()
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();

        Self {
            bind: env::var("CMDRVL_HOST_BIND").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port,
            api_token: env::var("CMDRVL_HOST_API_TOKEN")
                .ok()
                .filter(|token| !token.is_empty()),
            allowed_origins,
            max_body_bytes: max_body_mb * 1024 * 1024,
            kovrex_file_api: env::var("CMDRVL_KOVREX_FILE_API")
                .unwrap_or_else(|_| DEFAULT_KOVREX_FILE_API.to_string()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RunContext {
    pub auth_header: Option<String>,
    pub client: reqwest::Client,
    pub config: HostConfig,
}

#[derive(Clone)]
struct AppState<T: HostedTool> {
    config: HostConfig,
    tool: T,
    client: reqwest::Client,
}

#[derive(Debug)]
pub struct HostError {
    status: StatusCode,
    message: String,
}

impl HostError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    pub fn bad_gateway(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for HostError {
    fn into_response(self) -> Response {
        (
            self.status,
            [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for HostError {}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileInput {
    InlineBase64 {
        content_b64: String,
        filename: Option<String>,
    },
    KovrexFile {
        file_id: String,
        filename: Option<String>,
    },
}

impl FileInput {
    pub fn display_name(&self, default_name: &str) -> String {
        match self {
            Self::InlineBase64 { filename, .. } => {
                filename.clone().unwrap_or_else(|| default_name.to_string())
            }
            Self::KovrexFile { file_id, filename } => {
                filename.clone().unwrap_or_else(|| file_id.clone())
            }
        }
    }

    pub async fn resolve_bytes(
        &self,
        ctx: &RunContext,
        field_name: &str,
    ) -> Result<Vec<u8>, HostError> {
        match self {
            Self::InlineBase64 { content_b64, .. } => BASE64.decode(content_b64).map_err(|error| {
                HostError::bad_request(format!("invalid base64 for '{field_name}': {error}"))
            }),
            Self::KovrexFile { file_id, .. } => fetch_kovrex_file(ctx, file_id).await,
        }
    }

    pub async fn materialize_tempfile(
        &self,
        ctx: &RunContext,
        field_name: &str,
        suffix: &str,
    ) -> Result<NamedTempFile, HostError> {
        let bytes = self.resolve_bytes(ctx, field_name).await?;
        let mut file = tempfile::Builder::new()
            .prefix("cmdrvl-host-")
            .suffix(suffix)
            .tempfile()
            .map_err(|error| HostError::internal(format!("failed to create temp file: {error}")))?;
        file.write_all(&bytes)
            .map_err(|error| HostError::internal(format!("failed to write temp file: {error}")))?;
        Ok(file)
    }
}

async fn fetch_kovrex_file(ctx: &RunContext, file_id: &str) -> Result<Vec<u8>, HostError> {
    let url = format!(
        "{}/{}",
        ctx.config.kovrex_file_api.trim_end_matches('/'),
        file_id
    );
    let mut request = ctx.client.get(url);

    if let Some(auth_header) = &ctx.auth_header {
        request = request.header(AUTHORIZATION, auth_header);
    }

    let response = request.send().await.map_err(|error| {
        HostError::bad_gateway(format!("failed to fetch Kovrex file '{file_id}': {error}"))
    })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(HostError::bad_gateway(format!(
            "Kovrex file API returned {status} for '{file_id}': {body}"
        )));
    }

    let bytes = response.bytes().await.map_err(|error| {
        HostError::bad_gateway(format!("failed to read Kovrex file '{file_id}': {error}"))
    })?;
    Ok(bytes.to_vec())
}

#[derive(Clone, Debug)]
pub struct ToolRun {
    status: StatusCode,
    report_json: String,
}

impl ToolRun {
    pub fn json(status: StatusCode, report_json: String) -> Self {
        Self {
            status,
            report_json,
        }
    }
}

impl IntoResponse for ToolRun {
    fn into_response(self) -> Response {
        (
            self.status,
            [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
            self.report_json,
        )
            .into_response()
    }
}

#[async_trait]
pub trait HostedTool: Clone + Send + Sync + 'static {
    type Request: DeserializeOwned + Send + 'static;

    fn tool_name(&self) -> &'static str;
    fn tool_version(&self) -> &'static str;
    fn operator_json(&self) -> &'static str;
    fn output_schema_json(&self) -> &'static str;
    async fn run(&self, request: Self::Request, ctx: RunContext) -> Result<ToolRun, HostError>;
}

pub async fn serve<T: HostedTool>(tool: T) -> Result<(), HostError> {
    init_tracing();

    let config = HostConfig::from_env();
    let addr: SocketAddr = format!("{}:{}", config.bind, config.port)
        .parse()
        .map_err(|error| HostError::internal(format!("invalid bind address: {error}")))?;
    let state = Arc::new(AppState {
        config: config.clone(),
        tool: tool.clone(),
        client: reqwest::Client::new(),
    });

    let mut app = Router::new()
        .route("/health", get(health::<T>))
        .route("/describe", get(describe::<T>))
        .route("/schema/output", get(output_schema::<T>))
        .route("/version", get(version::<T>))
        .route("/v1/run", post(run::<T>))
        .with_state(state)
        .layer(DefaultBodyLimit::max(config.max_body_bytes))
        .layer(TraceLayer::new_for_http());

    if let Some(cors) = cors_layer(&config)? {
        app = app.layer(cors);
    }

    tracing::info!(
        tool = tool.tool_name(),
        port = config.port,
        "starting hosted operator"
    );

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|error| HostError::internal(format!("failed to bind socket: {error}")))?;
    axum::serve(listener, app)
        .await
        .map_err(|error| HostError::internal(format!("server error: {error}")))
}

fn init_tracing() {
    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "host_core=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .try_init();
}

fn cors_layer(config: &HostConfig) -> Result<Option<CorsLayer>, HostError> {
    if config.allowed_origins.is_empty() {
        return Ok(None);
    }

    let base = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    if config.allowed_origins.len() == 1 && config.allowed_origins[0] == "*" {
        return Ok(Some(base.allow_origin(Any)));
    }

    let origins = config
        .allowed_origins
        .iter()
        .map(|origin| {
            HeaderValue::from_str(origin).map_err(|error| {
                HostError::internal(format!("invalid allowed origin '{origin}': {error}"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(base.allow_origin(AllowOrigin::list(origins))))
}

#[derive(Serialize)]
struct HealthResponse<'a> {
    status: &'static str,
    tool: &'a str,
    tool_version: &'a str,
    host_version: &'static str,
}

#[derive(Serialize)]
struct VersionResponse<'a> {
    tool: &'a str,
    tool_version: &'a str,
    host_version: &'static str,
}

async fn health<T: HostedTool>(State(state): State<Arc<AppState<T>>>) -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        tool: state.tool.tool_name(),
        tool_version: state.tool.tool_version(),
        host_version: env!("CARGO_PKG_VERSION"),
    })
}

async fn version<T: HostedTool>(State(state): State<Arc<AppState<T>>>) -> impl IntoResponse {
    Json(VersionResponse {
        tool: state.tool.tool_name(),
        tool_version: state.tool.tool_version(),
        host_version: env!("CARGO_PKG_VERSION"),
    })
}

async fn describe<T: HostedTool>(State(state): State<Arc<AppState<T>>>) -> impl IntoResponse {
    (
        [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
        state.tool.operator_json(),
    )
}

async fn output_schema<T: HostedTool>(State(state): State<Arc<AppState<T>>>) -> impl IntoResponse {
    (
        [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
        state.tool.output_schema_json(),
    )
}

async fn run<T: HostedTool>(
    State(state): State<Arc<AppState<T>>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let auth_header = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    if let Err(error) = authenticate(&state.config, auth_header.as_deref()) {
        return error.into_response();
    }

    let request = match serde_json::from_slice::<T::Request>(&body) {
        Ok(request) => request,
        Err(error) => {
            return HostError::bad_request(format!("invalid JSON body: {error}")).into_response();
        }
    };

    let ctx = RunContext {
        auth_header,
        client: state.client.clone(),
        config: state.config.clone(),
    };

    match state.tool.run(request, ctx).await {
        Ok(run) => run.into_response(),
        Err(error) => error.into_response(),
    }
}

fn authenticate(config: &HostConfig, auth_header: Option<&str>) -> Result<(), HostError> {
    let Some(expected_token) = &config.api_token else {
        return Ok(());
    };

    let provided = auth_header
        .unwrap_or_default()
        .strip_prefix("Bearer ")
        .or_else(|| auth_header.unwrap_or_default().strip_prefix("bearer "))
        .unwrap_or_default();

    if provided == expected_token {
        Ok(())
    } else {
        Err(HostError::unauthorized("invalid or missing bearer token"))
    }
}

pub fn parse_delimiter(raw: &str) -> Option<u8> {
    match raw.to_ascii_lowercase().as_str() {
        "comma" | "," => Some(b','),
        "tab" | "\\t" => Some(b'\t'),
        "semicolon" | ";" => Some(b';'),
        "pipe" | "|" => Some(b'|'),
        "caret" | "^" => Some(b'^'),
        _ if raw.starts_with("0x") || raw.starts_with("0X") => {
            u8::from_str_radix(&raw[2..], 16).ok()
        }
        _ if raw.len() == 1 => raw.as_bytes().first().copied(),
        _ => None,
    }
}

pub fn rewrite_report_file_labels(report_json: &str, old_label: &str, new_label: &str) -> String {
    let mut value = match serde_json::from_str::<serde_json::Value>(report_json) {
        Ok(value) => value,
        Err(_) => return report_json.to_string(),
    };

    if let Some(files) = value
        .get_mut("files")
        .and_then(serde_json::Value::as_object_mut)
    {
        files.insert(
            "old".to_string(),
            serde_json::Value::String(old_label.to_string()),
        );
        files.insert(
            "new".to_string(),
            serde_json::Value::String(new_label.to_string()),
        );
    }

    serde_json::to_string(&value).unwrap_or_else(|_| report_json.to_string())
}

#[cfg(test)]
mod tests {
    use super::{FileInput, parse_delimiter, rewrite_report_file_labels};

    #[test]
    fn parse_delimiter_supports_keywords_and_bytes() {
        assert_eq!(parse_delimiter("comma"), Some(b','));
        assert_eq!(parse_delimiter("|"), Some(b'|'));
        assert_eq!(parse_delimiter("0x09"), Some(b'\t'));
        assert_eq!(parse_delimiter("invalid"), None);
    }

    #[test]
    fn file_input_deserializes_tagged_union() {
        let inline: FileInput = serde_json::from_str(
            r#"{"kind":"inline_base64","content_b64":"YWJj","filename":"old.csv"}"#,
        )
        .expect("inline input should deserialize");
        assert!(matches!(&inline, FileInput::InlineBase64 { .. }));
        if let FileInput::InlineBase64 {
            content_b64,
            filename,
        } = inline
        {
            assert_eq!(content_b64, "YWJj");
            assert_eq!(filename.as_deref(), Some("old.csv"));
        }

        let remote: FileInput =
            serde_json::from_str(r#"{"kind":"kovrex_file","file_id":"kvx_file_123"}"#)
                .expect("kovrex file input should deserialize");
        assert!(matches!(&remote, FileInput::KovrexFile { .. }));
        if let FileInput::KovrexFile { file_id, filename } = remote {
            assert_eq!(file_id, "kvx_file_123");
            assert!(filename.is_none());
        }
    }

    #[test]
    fn rewrite_report_file_labels_replaces_temp_paths() {
        let output = rewrite_report_file_labels(
            r#"{"files":{"old":"/tmp/a.csv","new":"/tmp/b.csv"},"outcome":"OK"}"#,
            "old.csv",
            "new.csv",
        );
        let value: serde_json::Value =
            serde_json::from_str(&output).expect("rewritten JSON should parse");
        assert_eq!(value["files"]["old"], "old.csv");
        assert_eq!(value["files"]["new"], "new.csv");
    }
}
