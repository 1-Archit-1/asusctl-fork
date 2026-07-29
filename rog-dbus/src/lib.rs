pub use asusd::{DBUS_IFACE, DBUS_NAME, DBUS_PATH};

pub mod asus_armoury;
pub mod scsi_aura;
pub mod zbus_anime;
pub mod zbus_aura;
pub mod zbus_backlight;
pub mod zbus_fan_curves;
pub mod zbus_platform;
pub mod zbus_slash;
pub mod zbus_xgm_led;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Discover all DBus object paths that implement `iface_name` on the asusd
/// service, then build a blocking proxy for each one.
///
/// This is the blocking equivalent of `find_iface_async` and is the correct
/// way to connect to interfaces like `xyz.ljones.Aura` whose object paths are
/// generated dynamically at runtime (e.g. `/xyz/ljones/aura/19b6_2_3`).
pub fn find_iface_blocking<T>(
    iface_name: &str,
) -> Result<Vec<T>, Box<dyn std::error::Error>>
where
    T: zbus::blocking::proxy::ProxyImpl<'static> + From<zbus::Proxy<'static>>,
{
    let conn = zbus::blocking::Connection::system()?;
    let f = zbus::blocking::fdo::ObjectManagerProxy::new(&conn, "xyz.ljones.Asusd", "/")?;
    let interfaces = f.get_managed_objects()?;
    let mut paths = Vec::new();
    for (obj_path, ifaces) in interfaces.iter() {
        for k in ifaces.keys() {
            if k.as_str() == iface_name {
                paths.push(obj_path.clone());
            }
        }
    }
    if paths.len() > 1 {
        eprintln!("find_iface_blocking: multiple {iface_name} devices found");
    }
    if paths.is_empty() {
        return Err(format!("find_iface_blocking: did not find {iface_name}").into());
    }
    paths.sort_by(|a, b| a.cmp(b));
    let mut ctrl = Vec::new();
    for path in paths {
        ctrl.push(
            T::builder(&conn)
                .path(path.clone())?
                .destination("xyz.ljones.Asusd")?
                .build()?,
        );
    }
    Ok(ctrl)
}

pub fn list_iface_blocking() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let conn = zbus::blocking::Connection::system()?;
    let f = zbus::blocking::fdo::ObjectManagerProxy::new(&conn, "xyz.ljones.Asusd", "/")?;
    let interfaces = f.get_managed_objects()?;
    let mut ifaces = Vec::new();
    for v in interfaces.iter() {
        for k in v.1.keys() {
            ifaces.push(k.to_string());
        }
    }
    Ok(ifaces)
}

pub fn has_iface_blocking(iface: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let conn = zbus::blocking::Connection::system()?;
    let f = zbus::blocking::fdo::ObjectManagerProxy::new(&conn, "xyz.ljones.Asusd", "/")?;
    let interfaces = f.get_managed_objects()?;
    for v in interfaces.iter() {
        for k in v.1.keys() {
            if k.as_str() == iface {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub async fn has_iface(iface: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let conn = zbus::Connection::system().await?;
    let f = zbus::fdo::ObjectManagerProxy::new(&conn, "xyz.ljones.Asusd", "/").await?;
    let interfaces = f.get_managed_objects().await?;
    for v in interfaces.iter() {
        for k in v.1.keys() {
            if k.as_str() == iface {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub async fn find_iface_async<T>(iface_name: &str) -> Result<Vec<T>, Box<dyn std::error::Error>>
where
    T: zbus::proxy::ProxyImpl<'static> + From<zbus::Proxy<'static>>,
{
    let conn = zbus::Connection::system().await?;
    let f = zbus::fdo::ObjectManagerProxy::new(&conn, "xyz.ljones.Asusd", "/").await?;
    let interfaces = f.get_managed_objects().await?;
    let mut paths = Vec::new();
    for v in interfaces.iter() {
        for k in v.1.keys() {
            if k.as_str() == iface_name {
                paths.push(v.0.clone());
            }
        }
    }
    if paths.len() > 1 {
        println!("Multiple asusd interfaces devices found");
    }
    if !paths.is_empty() {
        let mut ctrl = Vec::new();
        paths.sort_by(|a, b| a.cmp(b));
        for path in paths {
            ctrl.push(
                T::builder(&conn)
                    .path(path.clone())?
                    .destination("xyz.ljones.Asusd")?
                    .build()
                    .await?,
            );
        }
        return Ok(ctrl);
    }

    Err(format!("Did not find {iface_name}").into())
}
