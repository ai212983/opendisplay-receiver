//! OpenDisplay PIPE_WRITE (0x0080/0x0081/0x0082) sliding-window policy.
//!
//! The full-frame raw+compressed interop subset of the canonical wire:
//! negotiation with the min-rule, 8-bit sequence wrap, out-of-order
//! buffering, duplicate/idempotent retransmit, selective-ACK mask,
//! last-short-chunk, and explicit END validation. Partial, encrypted and
//! LAN PIPE uses are rejected per the canonical error namespaces.
//!
//! Field layouts mirror `opendisplay_protocol.h` (sections PIPE) and the
//! stock `protocol/commands.py` builders plus `protocol/responses.py`
//! parsers; the SACK mask rule mirrors the canonical firmware's
//! `pipeBuildAckPayload`/`pipeChunkReceived`, including the
//! `received_count`-bounded accepted prefix that keeps the first 32 chunks
//! of a transfer from setting phantom mask bits across the mod-256 wrap.
//!
//! The receiver holds small scalar state only. Bulk bytes (staging, reorder
//! slots) stay caller-owned; the transfer engine passes them per call.

use super::protocol::*;

/// Parsed 0x80 START request: full-frame panel byte total plus the
/// negotiated window/cadence/frame the client proposes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipeStartReq {
    pub compressed: bool,
    pub req_window: u8,
    pub req_ack_every: u8,
    pub client_max_frame: u16,
    pub total_size: u32,
}

/// Canonical 0x80 START rejection, carrying its namespaced error byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeStartReject {
    BadHeader,
    UnknownFlag,
    SizeMismatch,
    PartialUnsupported,
}

impl PipeStartReject {
    pub const fn code(self) -> u8 {
        match self {
            PipeStartReject::BadHeader => OD_ERR_PIPE_START_BAD_HEADER,
            PipeStartReject::UnknownFlag => OD_ERR_PIPE_START_UNKNOWN_FLAG,
            PipeStartReject::SizeMismatch => OD_ERR_PIPE_START_SIZE_MISMATCH,
            PipeStartReject::PartialUnsupported => OD_ERR_PIPE_START_PARTIAL_UNSUPPORTED,
        }
    }
}

/// Parse a 0x80 START payload (post-opcode bytes). Full-frame only:
/// `total_size` must name the product's decompressed frame, the version must
/// be `PIPE_VERSION`, unknown flag bits are rejected, and any PARTIAL
/// request is rejected with the canonical partial-unsupported code.
pub fn parse_pipe_start(
    payload: &[u8],
    frame_bytes: usize,
) -> Result<PipeStartReq, PipeStartReject> {
    if payload.len() < 10 {
        return Err(PipeStartReject::BadHeader);
    }
    let ver = payload[0];
    let flags = payload[1];
    if ver != PIPE_VERSION {
        return Err(PipeStartReject::BadHeader);
    }
    if flags & PIPE_FLAG_PARTIAL != 0 {
        if payload.len() < 22 {
            return Err(PipeStartReject::BadHeader);
        }
        return Err(PipeStartReject::PartialUnsupported);
    }
    if flags & !PIPE_KNOWN_FLAGS != 0 {
        return Err(PipeStartReject::UnknownFlag);
    }
    let req_window = payload[2];
    let req_ack_every = payload[3];
    let client_max_frame = u16::from_le_bytes([payload[4], payload[5]]);
    let total_size = u32::from_le_bytes([payload[6], payload[7], payload[8], payload[9]]);
    if (client_max_frame as usize) < PIPE_FRAME_OVERHEAD + 1 {
        return Err(PipeStartReject::BadHeader);
    }
    if total_size as usize != frame_bytes {
        return Err(PipeStartReject::SizeMismatch);
    }
    Ok(PipeStartReq {
        compressed: flags & PIPE_FLAG_COMPRESSED != 0,
        req_window,
        req_ack_every,
        client_max_frame,
        total_size,
    })
}

/// Effective transfer parameters after the canonical min-rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipeParams {
    pub window: u8,
    pub ack_every: u8,
    pub max_frame: usize,
    pub chunk_data: usize,
    pub compressed: bool,
}

/// Negotiate effective parameters: window/ack_every clamp into
/// `1..=DEV_MAX` with `ack_every <= window`; frame is the min of both
/// ceilings. A zero request clamps to 1, matching canonical firmware.
pub fn negotiate_pipe_params(req: &PipeStartReq) -> PipeParams {
    let window = req.req_window.clamp(1, PIPE_DEV_MAX_WINDOW);
    let ack_every = req
        .req_ack_every
        .clamp(1, PIPE_DEV_MAX_ACK_EVERY)
        .min(window);
    let max_frame = (req.client_max_frame as usize).min(PIPE_MAX_FRAME);
    PipeParams {
        window,
        ack_every,
        max_frame,
        chunk_data: max_frame - PIPE_FRAME_OVERHEAD,
        compressed: req.compressed,
    }
}

/// `[0x00][0x80][ver][dev_max_window][dev_max_ack_every][max_frame LE2][flags]`.
/// Carries the device maxima (not the effective values); the client applies
/// the same min-rule. Flags bit0 reports selective-repeat support.
pub fn build_pipe_start_ack() -> [u8; 8] {
    [
        RESP_ACK,
        ECHO_PIPE_WRITE_START,
        PIPE_VERSION,
        PIPE_DEV_MAX_WINDOW,
        PIPE_DEV_MAX_ACK_EVERY,
        (PIPE_MAX_FRAME & 0xFF) as u8,
        ((PIPE_MAX_FRAME >> 8) & 0xFF) as u8,
        0x01,
    ]
}

/// Canonical START NACK: `[0xFF][0x80][err][0x00]`.
pub fn build_pipe_start_nack(err: u8) -> [u8; 4] {
    [RESP_NACK, ECHO_PIPE_WRITE_START, err, 0x00]
}

/// SACK ACK: `[0x00][0x81][highest_seen][ack_mask LE4]`.
pub fn build_pipe_data_ack(highest_seen: u8, ack_mask: u32) -> [u8; 7] {
    let mask = ack_mask.to_le_bytes();
    [
        RESP_ACK,
        ECHO_PIPE_WRITE_DATA,
        highest_seen,
        mask[0],
        mask[1],
        mask[2],
        mask[3],
    ]
}

/// Fatal DATA NACK: `[0xFF][0x81][err][highest_seen][ack_mask LE4]`.
pub fn build_pipe_data_nack(err: u8, highest_seen: u8, ack_mask: u32) -> [u8; 8] {
    let mask = ack_mask.to_le_bytes();
    [
        RESP_NACK,
        ECHO_PIPE_WRITE_DATA,
        err,
        highest_seen,
        mask[0],
        mask[1],
        mask[2],
        mask[3],
    ]
}

/// END ACK: `[0x00][0x82]`.
pub fn build_pipe_end_ack() -> [u8; 2] {
    [RESP_ACK, ECHO_PIPE_WRITE_END]
}

/// END NACK on incompleteness: `[0xFF][0x82]`.
pub fn build_pipe_end_nack() -> [u8; 2] {
    [RESP_NACK, ECHO_PIPE_WRITE_END]
}

/// Explicit-END payload gate. FULL only: exactly `[0x00]`. A lone nonzero
/// refresh byte is unsupported (the caller preserves staging so a corrected
/// END can still commit it); any other shape is malformed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeEndReject {
    UnsupportedRefreshMode,
    Malformed,
}

pub fn validate_pipe_end(payload: &[u8]) -> Result<(), PipeEndReject> {
    match payload {
        [0x00] => Ok(()),
        [_] => Err(PipeEndReject::UnsupportedRefreshMode),
        _ => Err(PipeEndReject::Malformed),
    }
}

/// Position of one 0x81 DATA frame against the receiver's window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameClass {
    /// `seq == expected`: streams immediately, then drains successors.
    InOrder,
    /// Within the window ahead of `expected`: buffered, panel stream pauses.
    Ahead,
    /// At or below `expected` within the window: already accepted, dropped.
    Duplicate,
    /// Outside the window on both sides: fatal protocol violation.
    OutOfWindow,
}

/// Small sliding-window receiver state. Bulk bytes stay caller-owned; this
/// struct only tracks sequence positions and ACK cadence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receiver {
    expected_seq: u8,
    highest_seen: u8,
    has_received: bool,
    received_count: u32,
    since_ack: u32,
    ooo_since_gap: u32,
    gap_open: bool,
    queued: u32,
    window: u8,
    ack_every: u8,
}

impl Receiver {
    pub fn new(window: u8, ack_every: u8) -> Self {
        Self {
            expected_seq: 0,
            highest_seen: 0,
            has_received: false,
            received_count: 0,
            since_ack: 0,
            ooo_since_gap: 0,
            gap_open: false,
            queued: 0,
            window: window.max(1),
            ack_every: ack_every.max(1),
        }
    }

    /// Rolling slot for `seq`; collision-free while live seqs < slots.
    pub fn slot_index(seq: u8) -> usize {
        (seq as usize) % PIPE_REORDER_SLOTS
    }

    pub fn expected_seq(&self) -> u8 {
        self.expected_seq
    }

    pub fn queued(&self) -> u32 {
        self.queued
    }

    /// Read-only view of the gap latch for status and tests; ACK and drain
    /// decisions remain the behavioral interface.
    pub fn gap_open(&self) -> bool {
        self.gap_open
    }

    fn forward_distance(&self, seq: u8) -> u8 {
        seq.wrapping_sub(self.expected_seq)
    }

    fn backward_distance(&self, seq: u8) -> u8 {
        self.expected_seq.wrapping_sub(seq)
    }

    /// Classify one DATA seq. `fwd == 0` is in order; `0 < fwd < W` is
    /// ahead; at/below expected within W is a duplicate; anything else is
    /// a protocol violation.
    pub fn classify(&self, seq: u8) -> FrameClass {
        let fwd = self.forward_distance(seq);
        if fwd == 0 {
            return FrameClass::InOrder;
        }
        if fwd < self.window {
            return FrameClass::Ahead;
        }
        if self.backward_distance(seq) <= self.window {
            return FrameClass::Duplicate;
        }
        FrameClass::OutOfWindow
    }

    fn note_received(&mut self, seq: u8) {
        if !self.has_received {
            self.has_received = true;
            self.highest_seen = seq;
        } else {
            let fwd = seq.wrapping_sub(self.highest_seen);
            if fwd != 0 && fwd <= PIPE_ACK_MASK_BITS as u8 {
                self.highest_seen = seq;
            }
        }
        self.received_count = self.received_count.saturating_add(1);
    }

    /// Record one in-order accept. Returns true when the ACK cadence fires.
    pub fn accept_in_order(&mut self, seq: u8) -> bool {
        self.note_received(seq);
        self.expected_seq = self.expected_seq.wrapping_add(1);
        self.since_ack = self.since_ack.saturating_add(1);
        if self.since_ack >= u32::from(self.ack_every) {
            self.since_ack = 0;
            true
        } else {
            false
        }
    }

    /// Record draining one queued successor. Returns true when the ACK
    /// cadence fires; every drained frame counts so a post-gap drain
    /// refunds tokens promptly instead of waiting for fresh frames.
    pub fn drain_queued(&mut self) -> bool {
        self.note_received(self.expected_seq);
        self.expected_seq = self.expected_seq.wrapping_add(1);
        self.queued = self.queued.saturating_sub(1);
        self.since_ack = self.since_ack.saturating_add(1);
        if self.since_ack >= u32::from(self.ack_every) {
            self.since_ack = 0;
            true
        } else {
            false
        }
    }

    /// Record buffering one ahead-of-hole frame. Returns true when a SACK
    /// is due: immediately when the gap first opens, then rate-limited to
    /// one per ACK cadence while it stays open. The highest-seen marker
    /// advances for genuinely newer seqs (never for duplicates), mirroring
    /// the canonical highest-seen update in the ahead branch.
    pub fn buffer_ahead(&mut self, seq: u8, fresh: bool) -> bool {
        if fresh {
            self.queued = self.queued.saturating_add(1);
        }
        if !self.has_received {
            self.has_received = true;
            self.highest_seen = seq;
        } else {
            let fwd = seq.wrapping_sub(self.highest_seen);
            if fwd != 0 && fwd <= PIPE_ACK_MASK_BITS as u8 {
                self.highest_seen = seq;
            }
        }
        if !self.gap_open {
            self.gap_open = true;
            self.ooo_since_gap = 0;
            return true;
        }
        self.ooo_since_gap = self.ooo_since_gap.saturating_add(1);
        if self.ooo_since_gap >= u32::from(self.ack_every) {
            self.ooo_since_gap = 0;
            true
        } else {
            false
        }
    }

    /// Record discarding one duplicate. SACKs immediately while no gap is
    /// open (so the sender re-learns the position), else rate-limited like
    /// out-of-order arrivals.
    pub fn note_duplicate(&mut self) -> bool {
        if !self.gap_open {
            return true;
        }
        self.ooo_since_gap = self.ooo_since_gap.saturating_add(1);
        if self.ooo_since_gap >= u32::from(self.ack_every) {
            self.ooo_since_gap = 0;
            true
        } else {
            false
        }
    }

    /// Close the gap once the queue drains: duplicates go quiet again.
    pub fn clear_gap(&mut self) {
        self.gap_open = false;
        self.ooo_since_gap = 0;
    }

    /// Current SACK fields: highest seen seq plus the 32-bit mask where
    /// bit `i` marks chunk `(highest_seen - 1 - i)` received. `slot_has`
    /// reports the caller's reorder slots; the contiguous accepted prefix
    /// below `expected_seq` is bounded by `received_count` so the first 32
    /// chunks of a transfer never set phantom bits across the mod-256 wrap.
    pub fn sack(&self, slot_has: impl Fn(u8) -> bool) -> (u8, u32) {
        let highest_seen = if self.has_received {
            self.highest_seen
        } else {
            self.expected_seq.wrapping_sub(1)
        };
        let mut mask = 0u32;
        for i in 0..PIPE_ACK_MASK_BITS {
            let chunk = highest_seen.wrapping_sub(1 + i as u8);
            if chunk_received(self, chunk, &slot_has) {
                mask |= 1 << i;
            }
        }
        (highest_seen, mask)
    }
}

fn chunk_received(receiver: &Receiver, chunk: u8, slot_has: &impl Fn(u8) -> bool) -> bool {
    let below = receiver.expected_seq.wrapping_sub(1).wrapping_sub(chunk);
    let accepted_depth = receiver.received_count.min(PIPE_ACK_MASK_BITS);
    if (below as u32) < accepted_depth {
        return true;
    }
    slot_has(chunk)
}
