//! OpenPager companion core: layout libraries, app matching and protocol packets.
//!
//! Everything here is pure (no I/O beyond reading library files), so both the service and the
//! board simulator can use it. The contracts live in `spec/`; tests check this crate against the
//! examples there.

pub mod catalog;
pub mod focus;
pub mod keys;
pub mod layout;
pub mod library;
pub mod protocol;
