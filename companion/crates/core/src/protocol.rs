//! Packets (spec/protocol.md sections 1 and 3). Bodies the companion builds are typed; anything
//! received is kept as JSON and inspected by type.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Packet {
    pub id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub re: Option<u32>,
    #[serde(rename = "type")]
    pub kind: String,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Send {
    Hid { modifiers: u8, usage: u8 },
    Event(bool),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutKey {
    pub key: u8,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speak: Option<String>,
    /// A Material Icons name from `spec/icons.txt`, drawn above a small label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirm: bool,
    pub send: Send,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutBody {
    /// Text the board shows in place of the middle row (e.g. what a permission request is for).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub layout: String,
    pub name: String,
    pub source: String,
    pub reviewed: bool,
    pub keys: Vec<LayoutKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusBody {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u32>,
}
