#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::path::PathBuf;

use deltamod_native_core::patch_plan::{
    validate_patch_plan, PatchCandidate, PatchPlanError, PatchPlanRequest, PatchPlatform, PatchType,
};
use serde::{Deserialize, Serialize};

const MAX_INPUT_BYTES: u64 = 1024 * 1024;
const MAX_ERROR_CHARS: usize = 512;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(rename = "schemaVersion")]
    schema_version: u8,
    #[serde(rename = "gameRoot")]
    game_root: String,
    platform: Platform,
    patches: Vec<Candidate>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Platform {
    Win32,
    Linux,
    Darwin,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    #[serde(rename = "type")]
    patch_type: CandidateType,
    patch: String,
    to: String,
    #[serde(rename = "mappedTarget")]
    mapped_target: String,
    #[serde(rename = "modName")]
    mod_name: String,
    #[serde(rename = "modId")]
    mod_id: String,
    #[serde(rename = "modRoot")]
    mod_root: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum CandidateType {
    Override,
    Copy,
    Xdelta,
    G3mpatch,
    Csx,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Response {
    Success {
        ok: bool,
        #[serde(rename = "operationCount")]
        operation_count: usize,
        #[serde(rename = "patchCount")]
        patch_count: usize,
        #[serde(rename = "snapshotCount")]
        snapshot_count: usize,
    },
    Failure {
        ok: bool,
        code: &'static str,
        message: String,
    },
}

fn run() -> Response {
    let mut input = Vec::new();
    if io::stdin()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut input)
        .is_err()
        || input.len() as u64 > MAX_INPUT_BYTES
    {
        return failure(
            "PATCH_PLAN_INVALID",
            "Patch plan input exceeds the size limit.",
        );
    }
    let request: Request = match serde_json::from_slice(&input) {
        Ok(request) => request,
        Err(_) => return failure("PATCH_PLAN_INVALID", "Patch plan input is malformed."),
    };
    if request.schema_version != 1 {
        return failure(
            "PATCH_PLAN_INVALID",
            "Unsupported patch plan schema version.",
        );
    }
    let request = PatchPlanRequest {
        game_root: PathBuf::from(request.game_root),
        platform: match request.platform {
            Platform::Win32 => PatchPlatform::Win32,
            Platform::Linux => PatchPlatform::Linux,
            Platform::Darwin => PatchPlatform::Darwin,
        },
        patches: request
            .patches
            .into_iter()
            .map(|candidate| PatchCandidate {
                patch_type: match candidate.patch_type {
                    CandidateType::Override => PatchType::Override,
                    CandidateType::Copy => PatchType::Copy,
                    CandidateType::Xdelta => PatchType::Xdelta,
                    CandidateType::G3mpatch => PatchType::G3mPatch,
                    CandidateType::Csx => PatchType::Csx,
                },
                patch: candidate.patch,
                to: candidate.to,
                mapped_target: candidate.mapped_target,
                mod_name: candidate.mod_name,
                mod_id: candidate.mod_id,
                mod_root: PathBuf::from(candidate.mod_root),
            })
            .collect(),
    };
    match validate_patch_plan(&request) {
        Ok(approval) => Response::Success {
            ok: true,
            operation_count: approval.operation_count,
            patch_count: approval.patch_count,
            snapshot_count: approval.snapshot_count,
        },
        Err(error) => failure(error.code(), &error_message(&error)),
    }
}

fn error_message(error: &PatchPlanError) -> String {
    match error {
        PatchPlanError::Invalid(message) => message.clone(),
        PatchPlanError::Io(_) => "Patch plan paths could not be read safely.".to_owned(),
    }
}

fn failure(code: &'static str, message: &str) -> Response {
    Response::Failure {
        ok: false,
        code,
        message: message.chars().take(MAX_ERROR_CHARS).collect(),
    }
}

fn main() {
    let response = run();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    if serde_json::to_writer(&mut output, &response).is_err() || output.write_all(b"\n").is_err() {
        std::process::exit(1);
    }
}
