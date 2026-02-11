use tracing::info;

const SERVICE_TYPE: &str = "_shell-sync._tcp.local.";

/// Start mDNS broadcasting so clients can discover this server.
/// Returns a handle that keeps the service registered until dropped.
pub fn start_broadcast(port: u16) -> anyhow::Result<mdns_sd::ServiceDaemon> {
    let mdns = mdns_sd::ServiceDaemon::new()
        .map_err(|e| anyhow::anyhow!("Failed to create mDNS daemon: {}", e))?;

    let hostname = gethostname::gethostname()
        .to_string_lossy()
        .into_owned();

    let service_name = format!("shell-sync-{}", hostname);

    let service_info = mdns_sd::ServiceInfo::new(
        SERVICE_TYPE,
        &service_name,
        &format!("{}.local.", hostname),
        "",
        port,
        None,
    )
    .map_err(|e| anyhow::anyhow!("Failed to create service info: {}", e))?;

    mdns.register(service_info)
        .map_err(|e| anyhow::anyhow!("Failed to register mDNS service: {}", e))?;

    info!(port, service_type = SERVICE_TYPE, "mDNS broadcast started");

    Ok(mdns)
}
