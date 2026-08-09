#![forbid(unsafe_code)]

use std::env;
use std::io::{self, Write};
use std::path::Path;

use deltamod_native_core::archive_security::{
    validate_extracted_tree, ArchiveLimits, ExtractedTreeError, UnsafeArchiveError,
};
use serde::Serialize;

const JS_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Serialize)]
#[serde(untagged)]
enum Response {
    Success {
        ok: bool,
        #[serde(rename = "fileCount")]
        file_count: usize,
        #[serde(rename = "expandedBytes")]
        expanded_bytes: u64,
    },
    Failure {
        ok: bool,
        code: &'static str,
        message: &'static str,
    },
}

fn parse_limit(value: Option<String>) -> Result<u64, UnsafeArchiveError> {
    let value = value.ok_or(UnsafeArchiveError::InvalidLimit)?;
    let parsed = value
        .parse::<u64>()
        .map_err(|_| UnsafeArchiveError::InvalidLimit)?;
    if parsed > JS_MAX_SAFE_INTEGER {
        return Err(UnsafeArchiveError::InvalidLimit);
    }
    Ok(parsed)
}

fn run() -> Response {
    let mut args = env::args_os().skip(1);
    let Some(root) = args.next() else {
        return failure(UnsafeArchiveError::InvalidLimit.into());
    };
    let max_files = parse_limit(args.next().and_then(|value| value.into_string().ok()));
    let max_expanded_bytes = parse_limit(args.next().and_then(|value| value.into_string().ok()));
    let max_depth = parse_limit(args.next().and_then(|value| value.into_string().ok()));
    if args.next().is_some() {
        return failure(UnsafeArchiveError::InvalidLimit.into());
    }
    let limits = match (max_files, max_expanded_bytes, max_depth) {
        (Ok(max_files), Ok(max_expanded_bytes), Ok(max_depth)) => ArchiveLimits {
            max_files: match usize::try_from(max_files) {
                Ok(value) => value,
                Err(_) => return failure(UnsafeArchiveError::InvalidLimit.into()),
            },
            max_expanded_bytes,
            max_archive_bytes: 0,
            max_depth: match usize::try_from(max_depth) {
                Ok(value) => value,
                Err(_) => return failure(UnsafeArchiveError::InvalidLimit.into()),
            },
        },
        _ => return failure(UnsafeArchiveError::InvalidLimit.into()),
    };

    match validate_extracted_tree(Path::new(&root), limits) {
        Ok(inventory) => Response::Success {
            ok: true,
            file_count: inventory.file_count,
            expanded_bytes: inventory.expanded_bytes,
        },
        Err(error) => failure(error),
    }
}

fn failure(error: ExtractedTreeError) -> Response {
    let message = match &error {
        ExtractedTreeError::Unsafe(error) => match error {
            UnsafeArchiveError::InvalidLimit => "Invalid archive limit.",
            UnsafeArchiveError::FileLimit => "Extracted file-count limit exceeded.",
            UnsafeArchiveError::NestingLimit => "Extracted path is nested too deeply.",
            UnsafeArchiveError::LinkBlocked => "Extracted link is not allowed.",
            UnsafeArchiveError::SizeLimit => "Extracted size limit exceeded.",
            UnsafeArchiveError::PathEscape => "Extracted path escaped staging.",
            UnsafeArchiveError::EntryType => "Unsupported extracted entry.",
            _ => "Extracted tree validation failed.",
        },
        ExtractedTreeError::Io(_) => "Extracted tree could not be read safely.",
    };
    Response::Failure {
        ok: false,
        code: error.code(),
        message,
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
