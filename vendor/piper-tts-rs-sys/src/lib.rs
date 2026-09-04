#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

// ! Main CPU feature
#[cfg(not(feature = "cuda"))]
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bindings.rs"));

#[cfg(feature = "cuda")]
pub mod cuda;
