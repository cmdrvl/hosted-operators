#![forbid(unsafe_code)]

use std::path::PathBuf;

use async_trait::async_trait;
use axum::http::StatusCode;
use host_core::{
    FileInput, HostError, HostedTool, RunContext, ToolRun, parse_delimiter,
    rewrite_report_file_labels, serve,
};
use serde::Deserialize;
use shape::checks::suite::Outcome as ShapeOutcome;
use shape::cli::args::Args;

#[derive(Clone, Debug, Default)]
struct ShapeHost;

#[derive(Debug, Deserialize)]
struct ShapeRequest {
    old: FileInput,
    new: FileInput,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    delimiter: Option<String>,
    #[serde(default)]
    explicit: bool,
}

#[async_trait]
impl HostedTool for ShapeHost {
    type Request = ShapeRequest;

    fn tool_name(&self) -> &'static str {
        "shape"
    }

    fn tool_version(&self) -> &'static str {
        "0.4.1"
    }

    fn operator_json(&self) -> &'static str {
        include_str!("../assets/operator.json")
    }

    fn output_schema_json(&self) -> &'static str {
        include_str!("../assets/output-schema.json")
    }

    async fn run(&self, request: Self::Request, ctx: RunContext) -> Result<ToolRun, HostError> {
        let old_label = request.old.display_name("old.csv");
        let new_label = request.new.display_name("new.csv");
        let old = request
            .old
            .materialize_tempfile(&ctx, "old", ".csv")
            .await?;
        let new = request
            .new
            .materialize_tempfile(&ctx, "new", ".csv")
            .await?;

        if let Some(raw) = request.delimiter.as_deref()
            && parse_delimiter(raw).is_none()
        {
            return Err(HostError::bad_request(format!(
                "unsupported delimiter value: {raw}"
            )));
        }

        let args = Args {
            old: Some(PathBuf::from(old.path())),
            new: Some(PathBuf::from(new.path())),
            key: request.key,
            delimiter: request.delimiter,
            json: true,
            no_witness: true,
            capsule_dir: None,
            profile: None,
            profile_id: None,
            lock: Vec::new(),
            max_rows: None,
            max_bytes: None,
            explicit: request.explicit,
            schema: false,
            describe: false,
            command: None,
        };

        let result = shape::orchestrator::run(&args)
            .map_err(|error| HostError::internal(format!("shape execution failed: {error}")))?;
        let status = if result.outcome == ShapeOutcome::Refusal {
            StatusCode::UNPROCESSABLE_ENTITY
        } else {
            StatusCode::OK
        };

        Ok(ToolRun::json(
            status,
            rewrite_report_file_labels(&result.output, &old_label, &new_label),
        ))
    }
}

#[tokio::main]
async fn main() {
    if let Err(error) = serve(ShapeHost).await {
        eprintln!("fatal: {error}");
        std::process::exit(1);
    }
}
