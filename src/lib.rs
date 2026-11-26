//! OpenDisplay receiver engine for full-frame transfers.
//!
//! The engine parses, reassembles, and validates direct-write and PIPE
//! transfers. The application owns the transport and display and reports
//! when a frame has been displayed.
//!
//! [`engine::TransferEngine`] stores transfer state. The caller supplies
//! storage for frame staging, reorder slots, decode output, and the
//! inflater. The engine checks buffer lengths against the frame geometry.
//!
//! ```
//! use opendisplay::{engine::TransferEngine, geometry::DisplayGeometry};
//!
//! let geometry = DisplayGeometry::new(480, 800);
//! let receiver = TransferEngine::new(geometry).expect("valid monochrome geometry");
//! assert_eq!(receiver.frame_bytes(), 48_000);
//! ```

#![no_std]

pub mod engine;
pub mod geometry;
pub mod identity;
pub mod ledger;
pub mod pipe;
pub mod protocol;
