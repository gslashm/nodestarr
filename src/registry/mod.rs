//! Container registry access: reference parsing, version comparison and the
//! OCI distribution client used for update checks.

pub mod client;
pub mod reference;
pub mod version;

pub use client::RegistryClient;
pub use reference::ImageRef;
