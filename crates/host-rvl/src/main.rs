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

const PINNED_RVL_VERSION: &str = "0.7.1";

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
        PINNED_RVL_VERSION
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

#[cfg(test)]
mod tests {
    use std::fs;

    use host_core::HostedTool;
    use serde_json::Value;
    use tempfile::TempDir;

    use super::{PINNED_RVL_VERSION, RvlHost};
    use rvl::cli::args::Args;

    const PROFILE: &str =
        "include_columns: [unit_id, building, amount]\nkey: [unit_id, building]\n";

    fn run_rvl(old_csv: &str, new_csv: &str) -> Value {
        let dir = TempDir::new().expect("temporary directory");
        let old = dir.path().join("old.csv");
        let new = dir.path().join("new.csv");
        let profile = dir.path().join("profile.yaml");
        fs::write(&old, old_csv).expect("write old CSV");
        fs::write(&new, new_csv).expect("write new CSV");
        fs::write(&profile, PROFILE).expect("write profile");

        let mut args = Args::new(old, new, None, 0.95, 1e-9, None, true);
        args.profile = Some(profile);
        args.no_witness = true;
        let result = rvl::orchestrator::run(&args).expect("rvl comparison");
        serde_json::from_str(&result.output).expect("rvl JSON output")
    }

    fn assert_schema_valid(value: &Value) {
        let schema: Value = serde_json::from_str(RvlHost.output_schema_json())
            .expect("embedded output schema must be JSON");
        let validator = jsonschema::validator_for(&schema).expect("output schema must compile");
        let errors = validator
            .iter_errors(value)
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        assert!(errors.is_empty(), "schema validation failed: {errors:?}");
    }

    #[test]
    fn embedded_contract_matches_pinned_rvl_release() {
        let host = RvlHost;
        assert_eq!(host.tool_version(), PINNED_RVL_VERSION);

        let operator: Value =
            serde_json::from_str(host.operator_json()).expect("operator manifest must be JSON");
        assert_eq!(operator["version"], PINNED_RVL_VERSION);

        let manifest = include_str!("../Cargo.toml");
        assert!(
            manifest.contains(&format!("tag = \"v{PINNED_RVL_VERSION}\"")),
            "rvl dependency tag must match tool_version"
        );

        let lock = include_str!("../../../Cargo.lock");
        let rvl_package = lock
            .split("[[package]]")
            .find(|package| package.contains("\nname = \"rvl\"\n"))
            .expect("Cargo.lock must contain rvl");
        assert!(
            rvl_package.contains(&format!("version = \"{PINNED_RVL_VERSION}\"")),
            "locked rvl version must match tool_version"
        );
        assert!(
            rvl_package.contains(&format!("?tag=v{PINNED_RVL_VERSION}#")),
            "locked rvl source must match the pinned tag"
        );

        let schema: Value =
            serde_json::from_str(host.output_schema_json()).expect("schema must be JSON");
        assert_eq!(schema["properties"]["version"]["const"], "rvl.v0");
        assert_eq!(
            schema["properties"]["alignment"]["required"],
            serde_json::json!(["mode", "key_column", "key_columns"])
        );
        assert_eq!(
            schema["properties"]["contributors"]["items"]["properties"]["row_key"]["type"],
            "array"
        );
        assert_eq!(
            schema["properties"]["field_changes"]["items"]["properties"]["row_key"]["type"],
            "array"
        );
    }

    #[test]
    fn schema_accepts_composite_change_and_tuple_refusal() {
        let real_change = run_rvl(
            "unit_id,building,amount\nA,East,100\nA,West,200\n",
            "unit_id,building,amount\nA,West,250\nA,East,100\n",
        );
        assert_eq!(real_change["outcome"], "REAL_CHANGE");
        assert_eq!(
            real_change["alignment"]["key_columns"],
            serde_json::json!(["u8:unit_id", "u8:building"])
        );
        assert_eq!(
            real_change["contributors"][0]["row_key"],
            serde_json::json!(["u8:A", "u8:West"])
        );
        assert_schema_valid(&real_change);

        let refusal = run_rvl(
            "unit_id,building,amount\nA,East,100\nA,East,200\n",
            "unit_id,building,amount\nA,East,100\nA,West,200\n",
        );
        assert_eq!(refusal["outcome"], "REFUSAL");
        assert_eq!(refusal["refusal"]["code"], "E_KEY_DUP");
        assert_eq!(
            refusal["refusal"]["detail"]["key_values"],
            serde_json::json!(["u8:A", "u8:East"])
        );
        assert_schema_valid(&refusal);
    }
}
