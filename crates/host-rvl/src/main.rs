#![forbid(unsafe_code)]

use std::path::PathBuf;

use async_trait::async_trait;
use axum::http::StatusCode;
use host_core::{
    FileInput, HostError, HostedTool, RunContext, ToolRun, parse_delimiter,
    rewrite_report_file_labels, serve,
};
use rvl::cli::args::Args;
use rvl::cli::exit::Outcome;
use serde::Deserialize;

#[derive(Clone, Debug, Default)]
struct RvlHost;

#[derive(Debug, Deserialize)]
struct RvlRequest {
    old: FileInput,
    new: FileInput,
    #[serde(default)]
    key: Option<String>,
    #[serde(default = "default_threshold")]
    threshold: f64,
    #[serde(default = "default_tolerance")]
    tolerance: f64,
    #[serde(default)]
    delimiter: Option<String>,
    #[serde(default)]
    explicit: bool,
}

fn default_threshold() -> f64 {
    0.95
}

fn default_tolerance() -> f64 {
    1e-9
}

#[async_trait]
impl HostedTool for RvlHost {
    type Request = RvlRequest;

    fn tool_name(&self) -> &'static str {
        "rvl"
    }

    fn tool_version(&self) -> &'static str {
        "0.5.1"
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

        let delimiter = request
            .delimiter
            .as_deref()
            .map(|raw| {
                parse_delimiter(raw).ok_or_else(|| {
                    HostError::bad_request(format!("unsupported delimiter value: {raw}"))
                })
            })
            .transpose()?;

        let mut args = Args::new(
            PathBuf::from(old.path()),
            PathBuf::from(new.path()),
            request.key,
            request.threshold,
            request.tolerance,
            delimiter,
            true,
        );
        args.no_witness = true;
        args.explicit = request.explicit;

        let result = rvl::orchestrator::run(&args)
            .map_err(|error| HostError::internal(format!("rvl execution failed: {error}")))?;
        let status = match result.outcome {
            Outcome::NoRealChange | Outcome::RealChange => StatusCode::OK,
            Outcome::Refusal => StatusCode::UNPROCESSABLE_ENTITY,
        };

        Ok(ToolRun::json(
            status,
            rewrite_report_file_labels(&result.output, &old_label, &new_label),
        ))
    }
}

#[tokio::main]
async fn main() {
    if let Err(error) = serve(RvlHost).await {
        eprintln!("fatal: {error}");
        std::process::exit(1);
    }
}
