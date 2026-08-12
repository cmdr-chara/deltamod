use deltamod_network_runtime::{Client, Provider, RuntimeError};
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::test]
async fn mock_server_is_not_contacted_for_insecure_endpoint() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let accepted = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_ok()
    });

    let client = Client::new(Duration::from_secs(2), 1, Duration::ZERO).unwrap();
    let result = client
        .json::<serde_json::Value>(
            Provider::Nexus,
            &format!("http://{address}/redirect"),
            Some("credential-must-not-be-sent"),
        )
        .await;
    assert!(matches!(result, Err(RuntimeError::Url(_))));
    assert!(!accepted.await.unwrap());
}
