from pathlib import Path
import re

lib = Path("src-tauri/crates/tools-runtime/src/lib.rs")
text = lib.read_text()
pattern = re.compile(
    r'''            if let Err\(error\) =\n                rustix::process::kill_process_group\(pid, rustix::process::Signal::KILL\)\n            \{\n                if error != rustix::io::Errno::SRCH \{\n                    containment_error = Some\(error\.to_string\(\)\);\n                \}\n            \}\n'''
)
replacement = '''            if let Err(error) =
                rustix::process::kill_process_group(pid, rustix::process::Signal::KILL)
            {
                // A short-lived Darwin parent can exit between the run loop and killpg().
                // If that leaves a zombie or a reused foreign PGID, BSD killpg reports EPERM.
                // Only tolerate EPERM after try_wait proves our parent already exited;
                // EPERM while the parent is live remains a hard containment failure.
                #[cfg(target_os = "macos")]
                let stale_darwin_group = if error == rustix::io::Errno::PERM {
                    child
                        .try_wait()
                        .map_err(|source| RuntimeError::Reap {
                            kind: self.kind,
                            source,
                        })?
                        .is_some()
                } else {
                    false
                };
                #[cfg(not(target_os = "macos"))]
                let stale_darwin_group = false;
                if error != rustix::io::Errno::SRCH && !stale_darwin_group {
                    containment_error = Some(error.to_string());
                }
            }
'''
text, count = pattern.subn(replacement, text)
if count != 1:
    raise SystemExit(f"expected one killpg block, found {count}")
lib.write_text(text)

test = Path("src-tauri/crates/tools-runtime/tests/fake_process.rs")
text = test.read_text()
pattern = re.compile(
    r'''    assert!\(matches!\(\n        error,\n        RuntimeError::OutputOverflow \{\n            kind: "G3MTool",\n            limit: 1000\n        \}\n    \)\);'''
)
replacement = '''    assert!(
        matches!(
            error,
            RuntimeError::OutputOverflow {
                kind: "G3MTool",
                limit: 1000
            }
        ),
        "unexpected overflow result: {error:?}"
    );'''
text, count = pattern.subn(replacement, text)
if count != 1:
    raise SystemExit(f"expected one overflow assertion, found {count}")
test.write_text(text)
