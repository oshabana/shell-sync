use std::time::Duration;
use tracing::info;

const SERVICE_TYPE: &str = "_shell-sync._tcp.local.";

/// Discover a shell-sync server on the local network via mDNS.
/// Returns the server URL (e.g., "http://192.168.1.100:8888") or None if not found.
pub async fn discover_server(timeout: Duration) -> Option<String> {
    info!("Searching for shell-sync server via mDNS...");

    let mdns = mdns_sd::ServiceDaemon::new().ok()?;
    let receiver = mdns.browse(SERVICE_TYPE).ok()?;

    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, tokio::task::spawn_blocking({
            let receiver = receiver.clone();
            move || receiver.recv_timeout(Duration::from_millis(500))
        }))
        .await
        {
            Ok(Ok(Ok(mdns_sd::ServiceEvent::ServiceResolved(info)))) => {
                let port = info.get_port();
                if let Some(addr) = info.get_addresses().iter().next() {
                    let url = format!("http://{}:{}", addr, port);
                    info!(url = %url, "Found server via mDNS");
                    let _ = mdns.stop_browse(SERVICE_TYPE);
                    let _ = mdns.shutdown();
                    return Some(url);
                }
            }
            Ok(Ok(Ok(_))) => continue, // Other mDNS events
            _ => continue,             // Timeout or error
        }
    }

    let _ = mdns.stop_browse(SERVICE_TYPE);
    let _ = mdns.shutdown();
    info!("No server found via mDNS within timeout");
    None
}
