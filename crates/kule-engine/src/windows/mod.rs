//! What Kule asks of Windows beyond its filesystem (see [`crate::osfs`]):
//! the app container its proxy runs in, the certificates it trusts, and
//! what the sandbox around a task's commands needs of this Windows.

pub mod appcontainer;
pub mod preconditions;
pub(crate) mod roots;
