//! Shared OpenDisplay receiver engine.
//!
//! One transfer implementation for every product: the stock client speaks
//! the same wire to all of them, so parsing, reassembly, ZIP/PIPE policy,
//! generation tracking, and outcome settlement live here, tested once.
//! Radio wiring (GATT tables, advertising, window deadlines, notify),
//! supervisor handoff, buffer placement, and panel commit stay with the
//! product adapter, which lends this engine its bytes on every call.
//!
//! Ownership rule: this crate never allocates and never stores a frame
//! byte. [`engine::TransferEngine`] holds small scalar descriptors only;
//! staging, reorder slots, decode output, and inflater state arrive as
//! caller slices. A Meditamer adapter lends PSRAM boxes; a target without
//! a global external allocator lends carved byte ranges. Neither path can
//! spill frame bytes onto the CPU stack or the internal heap by accident:
//! every slice length is validated against the geometry before use.

#![no_std]

pub mod engine;
pub mod geometry;
pub mod identity;
pub mod ledger;
pub mod pipe;
pub mod protocol;
