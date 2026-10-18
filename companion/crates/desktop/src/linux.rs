//! Notifications on the session D-Bus: the service becomes a monitor for `Notify` calls (what
//! `dbus-monitor` does); it only reads them, the desktop still shows them as usual.

use std::collections::HashMap;

use zbus::blocking::{fdo::MonitoringProxy, Connection, MessageIterator};
use zbus::zvariant::OwnedValue;
use zbus::MatchRule;

use crate::Notification;

pub fn watch_notifications(mut each: impl FnMut(Notification) -> bool) -> Result<(), String> {
    let connection = Connection::session().map_err(|e| format!("D-Bus session: {e}"))?;
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::MethodCall)
        .interface("org.freedesktop.Notifications").map_err(|e| e.to_string())?
        .member("Notify").map_err(|e| e.to_string())?
        .build();
    MonitoringProxy::new(&connection).map_err(|e| e.to_string())?
        .become_monitor(&[rule], 0).map_err(|e| format!("cannot watch notifications: {e}"))?;
    for message in MessageIterator::from(&connection) {
        let Ok(message) = message else { continue };
        let header = message.header();
        if header.member().map(|m| m.as_str()) != Some("Notify") {
            continue;
        }
        type Body = (String, u32, String, String, String, Vec<String>, HashMap<String, OwnedValue>, i32);
        let Ok((app, _, _, summary, body, _, hints, _)) = message.body().deserialize::<Body>() else { continue };
        let desktop_entry = hints.get("desktop-entry").and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_default();
        if !each(Notification { app, desktop_entry, summary, body }) {
            break;
        }
    }
    Ok(())
}
