//! Importers for vendor-specific register-definition file formats.
//!
//! Each submodule turns a file format from a real product family (in
//! Varmeco's case: a semicolon-separated CSV with German labels) into a
//! list of [`modsim_core::model::RegisterPoint`]s the simulator can put
//! straight onto a [`modsim_core::model::DeviceType`].

pub mod varmeco;
