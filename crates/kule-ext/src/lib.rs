//! Kule's extensions: what an extension says it is ([`Manifest`]), the file
//! it travels in ([`Package`], a `.kulext`), and the person's installed and
//! approved extensions ([`Store`]).
//!
//! An extension is as untrusted as an agent. Its manifest asks for
//! permissions from a closed list, the person approves exactly what they saw
//! (pinned by digest), and any change holds the extension until they look
//! again. Nothing here runs an extension's code; the desktop app does that in
//! a sandboxed frame and asks the [`Store`] which extensions it may.

mod error;
mod manifest;
mod package;
mod store;
mod text;

pub use error::{Error, Result};
pub use manifest::{
    Command, Contributes, Field, FieldKind, MANIFEST, Manifest, McpServer, Panel, Permission,
};
pub use package::{MAX_ENTRY, MAX_PACKAGE, Package, Source, media_type, pack, plain_path};
pub use store::{Approval, Listed, Store};
pub use text::host_ok;
