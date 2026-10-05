//! Workspace-local shim shadowing crates.io `rig-fastembed 0.43.0`.
//!
//! The published crate depends on `fastembed 4.5.0`, which pins
//! `ort =2.0.0-rc.9`. That pin conflicts with our `ort ^2.0.0-rc.12`
//! , so plain cargo fails at version resolution.
compile_error!("rig-fastembed is shadowed by patches/rig-fastembed, which exists only to keep cargo from resolving the published crate's fastembed 4.5.0 -> ort =2.0.0-rc.9 pin against our ort ^2.0.0-rc.12. Do not enable rig's fastembed features; remove the [patch.crates-io] entry once rig/fastembed loosen the pin.");
