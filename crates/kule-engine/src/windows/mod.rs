//! What Kule asks of Windows beyond its filesystem (see [`crate::osfs`]):
//! the app container its proxy runs in, and the certificates it trusts.

pub mod appcontainer;
pub(crate) mod roots;
