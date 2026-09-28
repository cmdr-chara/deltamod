// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Instant,
};
#[path = "test_vectors.rs"]
mod vectors;
use vectors::*;

struct Server {
    url: Url,
    worker: Option<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    received: Arc<AtomicBool>,
    sent_parts: Arc<AtomicUsize>,
}
impl Server {
    fn new(parts: Vec<(Duration, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!(
            "http://{}/artifact",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let received = Arc::new(AtomicBool::new(false));
        let sent_parts = Arc::new(AtomicUsize::new(0));
        let observed_request = received.clone();
        let observed_parts = sent_parts.clone();
        let worker = thread::spawn(move || {
            let mut socket = loop {
                if stopped.load(Ordering::SeqCst) {
                    return;
                }
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("test listener: {e}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 512];
            while request.len() < 8192 && !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match socket.read(&mut chunk) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => request.extend_from_slice(&chunk[..n]),
                }
            }
            observed_request.store(true, Ordering::Release);
            for (delay, bytes) in parts {
                let start = Instant::now();
                while start.elapsed() < delay {
                    if stopped.load(Ordering::SeqCst) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                if socket.write_all(&bytes).is_err() {
                    return;
                }
                observed_parts.fetch_add(1, Ordering::Release);
            }
        });
        Self {
            url,
            worker: Some(worker),
            stop,
            received,
            sent_parts,
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
fn immediate(bytes: Vec<u8>) -> Vec<(Duration, Vec<u8>)> {
    vec![(Duration::ZERO, bytes)]
}
fn run<T>(work: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(work)
}
async fn download_test(
    server: &Server,
    limit: u64,
    control: &UpdateControl,
    progress: &mut dyn FnMut(u64, Option<u64>) -> Result<(), UpdateError>,
) -> Result<VerifiedDownload, UpdateError> {
    let verifier = PublisherVerifier::parse(PUBLIC_KEY, SIGNATURE)?;
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    cancellable(control, async {
        let response = client
            .get(server.url.clone())
            .send()
            .await
            .map_err(|_| UpdateError::Adapter("test request failed".into()))?;
        receive_response(response, &verifier, limit, control, progress).await
    })
    .await
}
fn headers(size: usize) -> Vec<u8> {
    format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n").into_bytes()
}

#[test]
fn publisher_verification_accepts_independent_modern_and_legacy_vectors() {
    for signature in [SIGNATURE, LEGACY_SIGNATURE] {
        let verifier = PublisherVerifier::parse(PUBLIC_KEY, signature).unwrap();
        assert_eq!(
            verifier.verify(PAYLOAD.to_vec()).unwrap().as_bytes(),
            PAYLOAD
        );
        assert!(matches!(
            verifier.verify(b"modified".to_vec()),
            Err(UpdateError::SignatureVerification(_))
        ));
    }
}
#[test]
fn key_id_and_authenticated_comment_are_checked() {
    let public = String::from_utf8(STANDARD.decode(PUBLIC_KEY).unwrap()).unwrap();
    let lines: Vec<_> = public.lines().collect();
    let mut packet = STANDARD.decode(lines[1]).unwrap();
    packet[2] ^= 1;
    let wrong_key = STANDARD.encode(format!("{}\n{}\n", lines[0], STANDARD.encode(packet)));
    let verifier = PublisherVerifier::parse(&wrong_key, SIGNATURE).unwrap();
    assert!(verifier.verify(PAYLOAD.to_vec()).is_err());
    let signature = String::from_utf8(STANDARD.decode(SIGNATURE).unwrap())
        .unwrap()
        .replace("timestamp:0", "timestamp:1");
    let verifier = PublisherVerifier::parse(PUBLIC_KEY, &STANDARD.encode(signature)).unwrap();
    assert!(verifier.verify(PAYLOAD.to_vec()).is_err());
}
#[test]
fn malformed_and_oversized_signature_metadata_fail_before_transport() {
    for bad in [
        String::new(),
        "not base64".into(),
        "A".repeat(MAX_SIGNATURE_TEXT + 1),
        STANDARD.encode([255; 16]),
    ] {
        assert!(PublisherVerifier::parse(&bad, SIGNATURE).is_err());
        assert!(PublisherVerifier::parse(PUBLIC_KEY, &bad).is_err());
    }
}
#[test]
fn artifact_urls_and_redirect_hosts_are_strictly_scoped() {
    let prefix = "https://github.com/cmdr-chara/deltamod/releases/download/";
    for file in [
        "community-v2.0.19/Deltamod_2.0.19_x64.exe",
        "community-v2.0.19/Deltamod%20Community.app.tar.gz",
    ] {
        assert!(validate_release_url(&Url::parse(&format!("{prefix}{file}")).unwrap()).is_ok());
    }
    for value in [
        "http://127.0.0.1/private",
        "https://github.com.evil.example/cmdr-chara/deltamod/releases/download/a/b",
        "https://github.com/other/repo/releases/download/a/b",
        "https://user:secret@github.com/cmdr-chara/deltamod/releases/download/a/b",
        "https://github.com:8443/cmdr-chara/deltamod/releases/download/a/b",
        "https://github.com/cmdr-chara/deltamod/releases/download/a/b?x=1",
        "https://github.com/cmdr-chara/deltamod/releases/download/a/%2Fetc",
        "https://github.com/cmdr-chara/deltamod/releases/download/a/",
    ] {
        assert!(
            validate_release_url(&Url::parse(value).unwrap()).is_err(),
            "{value}"
        );
    }
    assert!(allowed_host(
        &Url::parse("https://release-assets.githubusercontent.com/release/file?token=fixture")
            .unwrap()
    ));
    for value in [
        "http://github.com/",
        "https://127.0.0.1/",
        "https://github.com.evil.example/",
    ] {
        assert!(!allowed_host(&Url::parse(value).unwrap()));
    }
}
#[test]
fn signed_loopback_payload_reaches_verified_type_with_exact_progress() {
    let mut response = headers(PAYLOAD.len());
    response.extend_from_slice(PAYLOAD);
    let server = Server::new(immediate(response));
    let control = UpdateControl::default();
    let _session = control.begin_download().unwrap();
    let mut seen = 0;
    let result = run(download_test(
        &server,
        1024,
        &control,
        &mut |chunk, total| {
            seen += chunk;
            assert_eq!(total, Some(PAYLOAD.len() as u64));
            Ok(())
        },
    ))
    .unwrap();
    assert_eq!(result.as_bytes(), PAYLOAD);
    assert_eq!(seen, PAYLOAD.len() as u64);
}
#[test]
fn header_budget_rejects_without_waiting_for_body_or_reporting_progress() {
    let server = Server::new(vec![
        (Duration::ZERO, headers(1025)),
        (Duration::from_secs(3), vec![0; 1025]),
    ]);
    let started = Instant::now();
    let result = run(download_test(
        &server,
        1024,
        &UpdateControl::default(),
        &mut |_, _| panic!("body must not be accepted"),
    ));
    assert!(matches!(
        result,
        Err(UpdateError::ArtifactTooLarge { limit: 1024 })
    ));
    assert!(started.elapsed() < Duration::from_secs(1));
}
#[test]
fn chunked_budget_rejects_before_growing_the_buffer_or_reporting_overlimit_progress() {
    let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\ntest\r\n4\r\ndata\r\n0\r\n\r\n";
    let server = Server::new(immediate(response.to_vec()));
    let mut completed = 0;
    let result = run(download_test(
        &server,
        5,
        &UpdateControl::default(),
        &mut |n, _| {
            completed += n;
            Ok(())
        },
    ));
    assert!(matches!(
        result,
        Err(UpdateError::ArtifactTooLarge { limit: 5 })
    ));
    assert!(completed <= 5);
}
#[test]
fn progress_rejection_is_not_deferred_until_the_download_finishes() {
    let mut first = headers(PAYLOAD.len());
    first.extend_from_slice(&PAYLOAD[..4]);
    let server = Server::new(vec![
        (Duration::ZERO, first),
        (Duration::from_secs(3), PAYLOAD[4..].to_vec()),
    ]);
    let started = Instant::now();
    let result = run(download_test(
        &server,
        1024,
        &UpdateControl::default(),
        &mut |_, _| Err(UpdateError::Cancelled),
    ));
    assert!(matches!(result, Err(UpdateError::Cancelled)));
    assert!(started.elapsed() < Duration::from_secs(1));
}
#[test]
fn cancellation_interrupts_both_stalled_headers_and_stalled_body() {
    for before_headers in [true, false] {
        let mut response = headers(PAYLOAD.len());
        response.extend_from_slice(PAYLOAD);
        let parts = if before_headers {
            vec![(Duration::from_secs(3), response)]
        } else {
            vec![
                (Duration::ZERO, headers(PAYLOAD.len())),
                (Duration::from_secs(3), PAYLOAD.to_vec()),
            ]
        };
        let server = Server::new(parts);
        let control = UpdateControl::default();
        let _session = control.begin_download().unwrap();
        run(async {
            let cancel_after_request = async {
                tokio::time::timeout(Duration::from_secs(5), async {
                    while !server.received.load(Ordering::Acquire)
                        || (!before_headers && server.sent_parts.load(Ordering::Acquire) == 0)
                    {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                })
                .await
                .expect("test server did not observe the request/headers");
                let started = Instant::now();
                assert!(control.cancel());
                started
            };
            let mut progress = |_, _| Ok(());
            let (result, cancelled_at) = tokio::join!(
                download_test(&server, 1024, &control, &mut progress),
                cancel_after_request
            );
            assert!(
                matches!(result, Err(UpdateError::Cancelled)),
                "unexpected result: {:?}",
                result.err()
            );
            assert!(cancelled_at.elapsed() < Duration::from_secs(1));
        });
    }
}
#[test]
fn failed_http_truncated_and_modified_payloads_never_verify() {
    let mut truncated = headers(PAYLOAD.len() + 1);
    truncated.extend_from_slice(PAYLOAD);
    let mut modified = headers(4);
    modified.extend_from_slice(b"fake");
    let encoded = b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec();
    for response in [
        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        truncated,
        modified,
        encoded,
        headers(0),
    ] {
        let server = Server::new(immediate(response));
        assert!(run(download_test(
            &server,
            1024,
            &UpdateControl::default(),
            &mut |_, _| Ok(())
        ))
        .is_err());
    }
}
#[test]
fn already_cancelled_work_is_not_polled_and_its_resources_are_dropped() {
    struct Guard(Arc<AtomicUsize>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let control = UpdateControl::default();
    let _session = control.begin_download().unwrap();
    assert!(control.cancel());
    let dropped = Arc::new(AtomicUsize::new(0));
    let resource = Guard(dropped.clone());
    let work = async move {
        let _guard = resource;
        panic!("cancelled work was polled");
        #[allow(unreachable_code)]
        Ok(())
    };
    assert!(matches!(
        run(cancellable(&control, work)),
        Err(UpdateError::Cancelled)
    ));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}
