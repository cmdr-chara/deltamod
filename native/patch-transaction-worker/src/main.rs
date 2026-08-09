#![forbid(unsafe_code)]

use deltamod_native_core::patch_transaction::{backup, restore, validate, Journal};
use serde::Deserialize;
use std::io::{self, Read, Write};
use std::path::PathBuf;

const MAX_INPUT: u64 = 2 * 1024 * 1024;
const MAX_MESSAGE: usize = 512;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    action: String,
    game_root: String,
    journal: Journal,
    #[serde(default)]
    target: Option<String>,
}

fn main() {
    let mut input = Vec::new();
    let response = if io::stdin()
        .take(MAX_INPUT + 1)
        .read_to_end(&mut input)
        .is_err()
        || input.len() as u64 > MAX_INPUT
    {
        serde_json::json!({"ok":false,"code":"PATCH_TRANSACTION_INVALID","message":"Transaction input exceeds the size limit."})
    } else {
        run(&input)
    };
    let mut out = io::stdout().lock();
    let _ = serde_json::to_writer(&mut out, &response);
    let _ = out.write_all(b"\n");
}

fn run(input: &[u8]) -> serde_json::Value {
    let request: Request = match serde_json::from_slice(input) {
        Ok(value) => value,
        Err(_) => return failure("Transaction input is malformed."),
    };
    let root = PathBuf::from(request.game_root);
    let journal_path = root.join(".deltamod-community-patch-journal.json");
    let result = match request.action.as_str() {
        "validate" => validate(&request.journal, &root),
        "backup" => request
            .target
            .as_deref()
            .ok_or(deltamod_native_core::patch_transaction::TransactionError::Invalid)
            .and_then(|target| {
                let mut journal = request.journal;
                backup(&root, &journal_path, &mut journal, target)
            }),
        "restore" => {
            let mut journal = request.journal;
            restore(&root, &journal_path, &mut journal)
        }
        _ => Err(deltamod_native_core::patch_transaction::TransactionError::Invalid),
    };
    match result {
        Ok(()) => serde_json::json!({"ok":true}),
        Err(error) => failure(&error.to_string()),
    }
}

fn failure(message: &str) -> serde_json::Value {
    serde_json::json!({"ok":false,"code":"PATCH_TRANSACTION_INVALID","message":message.chars().take(MAX_MESSAGE).collect::<String>()})
}
