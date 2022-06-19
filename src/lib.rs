#![feature(generic_associated_types)]
#![feature(type_alias_impl_trait)]

mod firmware;

pub use firmware::*;

#[cfg(feature = "ble")]
mod gatt;

#[cfg(feature = "ble")]
pub use gatt::*;
