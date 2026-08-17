#[cfg(target_os = "linux")]
use zbus::{proxy, Connection};

#[cfg(target_os = "linux")]
#[proxy(
    interface = "org.freedesktop.portal.Settings",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
trait PortalSettings {
    fn read(&self, namespace: &str, key: &str) -> zbus::Result<zbus::zvariant::OwnedValue>;
}

#[cfg(target_os = "linux")]
pub async fn get_system_dark_mode() -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let connection = Connection::session().await?;
    let proxy = PortalSettingsProxy::new(&connection).await?;
    let value = proxy
        .read("org.freedesktop.appearance", "color-scheme")
        .await?;
    let scheme: u32 = value
        .downcast_ref::<zbus::zvariant::Value>()
        .ok()
        .and_then(|v| v.downcast_ref::<u32>().ok())
        .unwrap_or(0);
    Ok(scheme == 1)
}

#[cfg(not(target_os = "linux"))]
pub async fn get_system_dark_mode() -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    Ok(false)
}
