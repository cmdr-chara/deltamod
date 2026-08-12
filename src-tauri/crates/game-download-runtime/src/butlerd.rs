use crate::{CancellationToken, ProviderMetadata, RuntimeError};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

const MAX_LINE: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ButlerConfig {
    pub executable: PathBuf,
    pub executable_sha256: String,
    pub database: PathBuf,
    pub user_agent: String,
}

#[derive(Clone, Debug)]
pub struct ButlerProgress {
    pub phase: &'static str,
    pub completed: u64,
    pub total: Option<u64>,
    pub current_item: Option<String>,
}

#[derive(Debug)]
pub struct ButlerdAdapter {
    config: ButlerConfig,
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl ButlerdAdapter {
    pub fn new(config: ButlerConfig) -> Result<Self, RuntimeError> {
        verify_executable(&config.executable, &config.executable_sha256)?;
        if let Some(parent) = config.database.parent() {
            fs::create_dir_all(parent).map_err(|_| RuntimeError::Storage)?;
        }
        Ok(Self { config })
    }

    pub fn install<F>(
        &self,
        metadata: &ProviderMetadata,
        destination: &Path,
        cancelled: &CancellationToken,
        mut progress: F,
    ) -> Result<PathBuf, RuntimeError>
    where
        F: FnMut(ButlerProgress),
    {
        let ProviderMetadata::Itch {
            homepage,
            file_id,
            game_id,
        } = metadata
        else {
            return Err(RuntimeError::InvalidCatalog(
                "butlerd requires Itch metadata",
            ));
        };
        if destination.exists() || !destination.is_absolute() {
            return Err(RuntimeError::Storage);
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|_| RuntimeError::Storage)?;
        }
        let result = self.install_inner(
            homepage,
            file_id,
            game_id,
            destination,
            cancelled,
            &mut progress,
        );
        if result.is_err() {
            let _ = fs::remove_dir_all(destination);
        }
        result.map(|()| destination.to_path_buf())
    }

    fn install_inner<F>(
        &self,
        homepage: &str,
        file_id: &str,
        game_id: &str,
        destination: &Path,
        cancelled: &CancellationToken,
        progress: &mut F,
    ) -> Result<(), RuntimeError>
    where
        F: FnMut(ButlerProgress),
    {
        if cancelled.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        let mut command = Command::new(&self.config.executable);
        command
            .arg("daemon")
            .arg("--json")
            .arg("--transport")
            .arg("tcp")
            .arg("--keep-alive")
            .arg("--dbpath")
            .arg(&self.config.database)
            .arg("--address")
            .arg("https://itch.io")
            .arg("--user-agent")
            .arg(&self.config.user_agent)
            .arg("--destiny-pid")
            .arg(std::process::id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = ChildGuard(
            command
                .spawn()
                .map_err(|_| RuntimeError::ButlerUnavailable)?,
        );
        let stdout = child.0.stdout.take().ok_or(RuntimeError::ButlerProtocol(
            "listen notification unavailable",
        ))?;
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            for _ in 0..32 {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) if line.len() as u64 <= MAX_LINE => {
                        let is_listen = serde_json::from_str::<Value>(&line)
                            .ok()
                            .and_then(|value| {
                                value.get("type").and_then(Value::as_str).map(str::to_owned)
                            })
                            .as_deref()
                            == Some("butlerd/listen-notification");
                        if is_listen {
                            let _ = sender.send(Ok(line));
                            return;
                        }
                    }
                    Ok(_) => break,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        return;
                    }
                }
            }
            let _ = sender.send(Err(std::io::Error::other("listen notification missing")));
        });
        let line = receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| RuntimeError::ButlerProtocol("listen notification timed out"))?
            .map_err(|_| RuntimeError::ButlerProtocol("listen notification failed"))?;
        if line.len() as u64 > MAX_LINE {
            return Err(RuntimeError::ButlerProtocol(
                "listen notification too large",
            ));
        }
        let listen: Value = serde_json::from_str(&line)
            .map_err(|_| RuntimeError::ButlerProtocol("invalid listen notification"))?;
        if listen.get("type").and_then(Value::as_str) != Some("butlerd/listen-notification") {
            return Err(RuntimeError::ButlerProtocol("invalid listen notification"));
        }
        let secret = listen
            .get("secret")
            .and_then(Value::as_str)
            .filter(|value| value.len() <= 128)
            .ok_or(RuntimeError::ButlerProtocol("listen secret missing"))?;
        let address: SocketAddr = listen
            .pointer("/tcp/address")
            .and_then(Value::as_str)
            .and_then(|value| value.parse().ok())
            .filter(|value: &SocketAddr| {
                matches!(value.ip(), IpAddr::V4(ip) if ip.is_loopback())
                    || matches!(value.ip(), IpAddr::V6(ip) if ip.is_loopback())
            })
            .ok_or(RuntimeError::ButlerProtocol("non-local listen address"))?;
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
            .map_err(|_| RuntimeError::ButlerProtocol("connection failed"))?;
        stream
            .set_read_timeout(Some(Duration::from_millis(250)))
            .map_err(|_| RuntimeError::ButlerProtocol("connection setup failed"))?;
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|_| RuntimeError::ButlerProtocol("connection setup failed"))?,
        );
        rpc(
            &mut stream,
            &mut reader,
            1,
            "Meta.Authenticate",
            json!({"secret":secret}),
            cancelled,
            None,
            progress,
        )?;
        let profiles = rpc(
            &mut stream,
            &mut reader,
            2,
            "Profile.List",
            json!({}),
            cancelled,
            None,
            progress,
        )?;
        let profile_id = profiles
            .get("profiles")
            .and_then(Value::as_array)
            .and_then(|profiles| profiles.first())
            .and_then(|profile| profile.get("id"))
            .and_then(Value::as_u64)
            .ok_or(RuntimeError::ItchAuthRequired)?;
        rpc(
            &mut stream,
            &mut reader,
            3,
            "Profile.UseSavedLogin",
            json!({"profileId":profile_id}),
            cancelled,
            None,
            progress,
        )
        .map_err(|error| {
            if matches!(error, RuntimeError::Cancelled) {
                error
            } else {
                RuntimeError::ItchAuthRequired
            }
        })?;
        let game_result = rpc(
            &mut stream,
            &mut reader,
            4,
            "Fetch.Game",
            json!({"gameId":game_id.parse::<u64>().map_err(|_| RuntimeError::InvalidCatalog("invalid Itch game id"))?,"fresh":true}),
            cancelled,
            None,
            progress,
        )?;
        let game = game_result
            .get("game")
            .cloned()
            .ok_or(RuntimeError::InvalidProviderResponse)?;
        if game
            .get("id")
            .and_then(Value::as_u64)
            .map(|id| id.to_string())
            .as_deref()
            != Some(game_id)
            || game.get("url").and_then(Value::as_str) != Some(homepage)
        {
            return Err(RuntimeError::InvalidProviderResponse);
        }
        let uploads = rpc(
            &mut stream,
            &mut reader,
            5,
            "Fetch.GameUploads",
            json!({"gameId":game_id.parse::<u64>().unwrap_or_default(),"compatible":true,"fresh":true}),
            cancelled,
            None,
            progress,
        )?;
        let upload = uploads
            .get("uploads")
            .and_then(Value::as_array)
            .and_then(|uploads| {
                uploads.iter().find(|upload| {
                    upload
                        .get("id")
                        .and_then(Value::as_u64)
                        .map(|id| id.to_string())
                        .as_deref()
                        == Some(file_id)
                })
            })
            .cloned()
            .ok_or(RuntimeError::ArtifactUnavailable)?;
        let queued = rpc(
            &mut stream,
            &mut reader,
            6,
            "Install.Queue",
            json!({
                "reason":"install","noCave":true,"installFolder":destination,
                "game":game,"upload":upload,"queueDownload":true,"profileId":profile_id
            }),
            cancelled,
            None,
            progress,
        )?;
        let task = queued
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| value.len() <= 128)
            .ok_or(RuntimeError::InvalidProviderResponse)?
            .to_owned();
        let staging = queued
            .get("stagingFolder")
            .and_then(Value::as_str)
            .filter(|value| value.len() <= 4096)
            .ok_or(RuntimeError::InvalidProviderResponse)?
            .to_owned();
        rpc(
            &mut stream,
            &mut reader,
            7,
            "Install.Perform",
            json!({"id":task,"stagingFolder":staging}),
            cancelled,
            Some(&task),
            progress,
        )?;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)] // Keeps one bounded conversation loop for every method.
fn rpc<F>(
    stream: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    id: u64,
    method: &str,
    params: Value,
    cancelled: &CancellationToken,
    task: Option<&str>,
    progress: &mut F,
) -> Result<Value, RuntimeError>
where
    F: FnMut(ButlerProgress),
{
    let request =
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .map_err(|_| RuntimeError::ButlerProtocol("request encoding failed"))?;
    if request.len() as u64 > MAX_LINE {
        return Err(RuntimeError::ButlerProtocol("request too large"));
    }
    stream
        .write_all(&request)
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|_| RuntimeError::ButlerProtocol("request failed"))?;
    let mut cancel_sent = false;
    loop {
        if cancelled.is_cancelled() && !cancel_sent {
            if let Some(task) = task {
                let cancel = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":9000 + id,"method":"Install.Cancel","params":{"id":task}})).map_err(|_| RuntimeError::ButlerProtocol("cancel encoding failed"))?;
                stream
                    .write_all(&cancel)
                    .and_then(|_| stream.write_all(b"\n"))
                    .map_err(|_| RuntimeError::ButlerProtocol("cancel failed"))?;
                cancel_sent = true;
            } else {
                return Err(RuntimeError::Cancelled);
            }
        }
        let mut bytes = Vec::new();
        match reader
            .by_ref()
            .take(MAX_LINE + 1)
            .read_until(b'\n', &mut bytes)
        {
            Ok(0) => return Err(RuntimeError::ButlerProtocol("daemon disconnected")),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(_) => return Err(RuntimeError::ButlerProtocol("response failed")),
        }
        if bytes.len() as u64 > MAX_LINE || !bytes.ends_with(b"\n") {
            return Err(RuntimeError::ButlerProtocol("response too large"));
        }
        let message: Value = serde_json::from_slice(&bytes)
            .map_err(|_| RuntimeError::ButlerProtocol("invalid response"))?;
        if message.get("id").and_then(Value::as_u64) == Some(id) {
            if cancel_sent {
                return Err(RuntimeError::Cancelled);
            }
            if message.get("error").is_some() {
                return Err(RuntimeError::ButlerProtocol("provider rejected request"));
            }
            return message
                .get("result")
                .cloned()
                .ok_or(RuntimeError::ButlerProtocol("result missing"));
        }
        if message.get("method").and_then(Value::as_str) == Some("Progress") {
            let amount = message
                .pointer("/params/progress")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            progress(ButlerProgress {
                phase: "download",
                completed: (amount * 10_000.0) as u64,
                total: Some(10_000),
                current_item: None,
            });
        }
        if let (Some(server_id), Some(_)) = (message.get("id").cloned(), message.get("method")) {
            let response = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":server_id,"error":{"code":-32800,"message":"interactive request cancelled"}})).map_err(|_| RuntimeError::ButlerProtocol("response encoding failed"))?;
            stream
                .write_all(&response)
                .and_then(|_| stream.write_all(b"\n"))
                .map_err(|_| RuntimeError::ButlerProtocol("interactive cancellation failed"))?;
        }
    }
}

fn verify_executable(path: &Path, expected: &str) -> Result<(), RuntimeError> {
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(RuntimeError::ButlerUnavailable);
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| RuntimeError::ButlerUnavailable)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > 256 * 1024 * 1024
    {
        return Err(RuntimeError::ButlerUnavailable);
    }
    let mut file = fs::File::open(path).map_err(|_| RuntimeError::ButlerUnavailable)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| RuntimeError::ButlerUnavailable)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if !hex::encode(hasher.finalize()).eq_ignore_ascii_case(expected) {
        return Err(RuntimeError::ButlerUnavailable);
    }
    Ok(())
}
