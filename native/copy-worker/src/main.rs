#![forbid(unsafe_code)]

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;

use deltamod_native_core::staged_copy::{
    copy_directory_staged, inspect_source_tree, EntryKind, StagedCopyError,
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Event<'a> {
    Entry {
        #[serde(rename = "entryType")]
        entry_type: &'static str,
        relative: &'a str,
        size: u64,
    },
    Inventory {
        #[serde(rename = "sourceRoot")]
        source_root: &'a str,
        #[serde(rename = "fileCount")]
        file_count: u64,
        #[serde(rename = "totalBytes")]
        total_bytes: u64,
    },
    Progress {
        completed: u64,
        total: u64,
        #[serde(rename = "currentItem")]
        current_item: &'a str,
    },
    Commit {
        completed: u64,
        total: u64,
        #[serde(rename = "currentItem")]
        current_item: &'a str,
    },
    Done,
    Error {
        code: &'a str,
        message: String,
    },
}

fn emit(event: &Event<'_>) -> Result<(), StagedCopyError> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, event)
        .map_err(|error| StagedCopyError::CopyFailed(io::Error::other(error)))?;
    output
        .write_all(b"\n")
        .and_then(|_| output.flush())
        .map_err(StagedCopyError::CopyFailed)
}

fn run() -> Result<(), StagedCopyError> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(args.next().ok_or(StagedCopyError::SourceUnreadable(
        io::Error::new(io::ErrorKind::InvalidInput, "missing source"),
    ))?);
    let destination = PathBuf::from(args.next().ok_or(StagedCopyError::CopyFailed(
        io::Error::new(io::ErrorKind::InvalidInput, "missing destination"),
    ))?);
    let operation_id = args
        .next()
        .and_then(|value| value.into_string().ok())
        .filter(|value| valid_operation_id(value))
        .ok_or(StagedCopyError::CopyFailed(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid operation id",
        )))?;
    let retries = args
        .next()
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| (1..=10).contains(value))
        .ok_or(StagedCopyError::CopyFailed(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid retries",
        )))?;
    let available_bytes = args
        .next()
        .and_then(|value| value.into_string().ok())
        .and_then(|value| {
            if value == "null" {
                Some(None)
            } else {
                value.parse::<u64>().ok().map(Some)
            }
        })
        .ok_or(StagedCopyError::CopyFailed(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid available bytes",
        )))?;
    if args.next().is_some() {
        return Err(StagedCopyError::CopyFailed(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unexpected argument",
        )));
    }

    let inventory = inspect_source_tree(&source)?;
    let required_bytes = inventory
        .total_bytes
        .checked_add(
            (inventory
                .total_bytes
                .checked_add(19)
                .ok_or(StagedCopyError::SourceOverflow)?
                / 20)
                .min(256 * 1024 * 1024),
        )
        .filter(|value| *value <= deltamod_native_core::staged_copy::JS_MAX_SAFE_INTEGER)
        .ok_or(StagedCopyError::SourceOverflow)?;
    if let Some(available) = available_bytes {
        if available < required_bytes {
            emit(&Event::Error {
                code: "INSUFFICIENT_SPACE",
                message: format!(
                    "Not enough free space. Required: {required_bytes} bytes; available: {available} bytes."
                ),
            })?;
            std::process::exit(1);
        }
    }
    let source_root = inventory.source_root.to_string_lossy();
    for entry in &inventory.entries {
        emit(&Event::Entry {
            entry_type: match entry.kind {
                EntryKind::Directory => "directory",
                EntryKind::File => "file",
            },
            relative: &entry.relative,
            size: entry.size,
        })?;
    }
    emit(&Event::Inventory {
        source_root: &source_root,
        file_count: inventory.file_count,
        total_bytes: inventory.total_bytes,
    })?;

    let parent = destination
        .parent()
        .ok_or(StagedCopyError::DestinationParentChanged)?;
    let basename = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(StagedCopyError::DestinationParentChanged)?;
    let staging = parent.join(format!(".{basename}.importing-{operation_id}"));
    copy_directory_staged(
        &inventory,
        &destination,
        &staging,
        retries,
        |completed, current_item| {
            emit(&Event::Progress {
                completed,
                total: inventory.total_bytes,
                current_item,
            })
        },
        || {
            emit(&Event::Commit {
                completed: inventory.total_bytes,
                total: inventory.total_bytes,
                current_item: basename,
            })
        },
        || false,
    )?;
    emit(&Event::Done)
}

fn valid_operation_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn main() {
    if let Err(error) = run() {
        let _ = emit(&Event::Error {
            code: error.code(),
            message: error.to_string(),
        });
        std::process::exit(1);
    }
}
