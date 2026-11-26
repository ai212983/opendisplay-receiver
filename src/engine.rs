//! Transfer state machine: every reassembly, validation, generation, and
//! outcome decision behind one interface.
//!
//! The product adapter owns radio IO, Embassy state, and all bulk bytes;
//! this engine owns the decisions. It stores small scalar descriptors only
//! (ids, counts, the PIPE receiver, slot occupancy); staging, reorder
//! slots, decode output, and inflater state arrive as caller slices on
//! every call, so placement stays entirely with the caller. Mis-sized
//! caller buffers fail as transfer NACKs, never panics.
//!
//! Call order mirrors the canonical handler flow: a new START supersedes
//! everything (legacy supersedes before the adapter reserves memory, PIPE
//! after, matching stock-client retry behavior); DATA appends stream or
//! buffer; END validates then commits exactly once; the display owner takes
//! the pending frame and reports its outcome by generation.

use super::geometry::DisplayGeometry;
use super::ledger::TransferLedger;
use super::pipe::{
    self, build_pipe_data_ack, build_pipe_data_nack, FrameClass, PipeParams, PipeStartReq, Receiver,
};
use super::protocol::{
    self, DecodeError, StartKind, COMPRESSED_STAGING_MAX, PIPE_REORDER_SLOTS,
    PIPE_REORDER_SLOT_SIZE,
};

/// Wire-visible handler NACK codes. No upstream namespace defines these
/// paths, so both products answer with these exact bytes toward the stock
/// client; defining them once keeps the two receivers identical.
pub const NACK_REMOTE_NOT_ARMED: u8 = 0x01;
pub const NACK_START_REJECTED: u8 = 0x02;
pub const NACK_NO_STAGING_MEMORY: u8 = 0x03;
pub const NACK_NO_SESSION: u8 = 0x01;
pub const NACK_OVERFLOW: u8 = 0x02;
pub const NACK_INCOMPLETE: u8 = 0x02;
pub const NACK_BAD_REFRESH_MODE: u8 = 0x03;
pub const NACK_DECODE_FAILED: u8 = 0x04;

/// One notify payload for a transfer response. All transfer responses fit
/// 8 bytes (SACK 7, fatal NACK 8, ACK/NACK 2/4); config-chunk streaming
/// stays with the adapter's 0x40 path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Response {
    len: u8,
    bytes: [u8; 8],
}

impl Response {
    pub const fn from2(bytes: [u8; 2]) -> Self {
        Self {
            len: 2,
            bytes: [bytes[0], bytes[1], 0, 0, 0, 0, 0, 0],
        }
    }

    pub const fn from4(bytes: [u8; 4]) -> Self {
        Self {
            len: 4,
            bytes: [bytes[0], bytes[1], bytes[2], bytes[3], 0, 0, 0, 0],
        }
    }

    pub const fn from7(bytes: [u8; 7]) -> Self {
        Self {
            len: 7,
            bytes: [
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], 0,
            ],
        }
    }

    pub const fn from8(bytes: [u8; 8]) -> Self {
        Self { len: 8, bytes }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

/// One staged direct-write transfer: generation, received stream count, and
/// compression mode. The bytes live in the caller's staging slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyTransfer {
    pub id: u64,
    pub received: usize,
    pub compressed: bool,
}

/// One PIPE_WRITE sliding-window transfer: scalar receiver policy plus
/// slot occupancy. Staging and slot bytes live in caller slices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipeTransfer {
    pub id: u64,
    pub receiver: Receiver,
    pub total: usize,
    pub compressed: bool,
    pub written: usize,
    pub slot_occupied: [bool; PIPE_REORDER_SLOTS],
    pub slot_seq: [u8; PIPE_REORDER_SLOTS],
    pub slot_len: [u16; PIPE_REORDER_SLOTS],
    pub error: bool,
}

impl PipeTransfer {
    fn slot_has(&self, seq: u8) -> bool {
        let slot = Receiver::slot_index(seq);
        self.slot_occupied[slot] && self.slot_seq[slot] == seq
    }

    fn sack(&self) -> Response {
        let (highest_seen, ack_mask) = self.receiver.sack(|seq| self.slot_has(seq));
        Response::from7(build_pipe_data_ack(highest_seen, ack_mask))
    }

    fn fatal(&self, err: u8) -> Response {
        let (highest_seen, ack_mask) = self.receiver.sack(|seq| self.slot_has(seq));
        Response::from8(build_pipe_data_nack(err, highest_seen, ack_mask))
    }

    /// Move one queued reorder slot into the contiguous stream prefix.
    /// False when the bytes would overrun staging (caller NACKs and retires),
    /// the caller slots are short (never panics on mis-sized buffers), or —
    /// for raw sessions — the negotiated total: queued chunks must not
    /// bypass the total the immediate path enforces. Compressed sessions
    /// pass `None`: their stream stages opaquely past the frame length and
    /// only the END decode proves completion.
    fn drain_slot(
        &mut self,
        seq: u8,
        staging: &mut [u8],
        slots: &[u8],
        raw_total: Option<usize>,
    ) -> bool {
        let slot = Receiver::slot_index(seq);
        let len = self.slot_len[slot] as usize;
        let at = slot * PIPE_REORDER_SLOT_SIZE;
        let next = self.written.saturating_add(len);
        if next > staging.len() || at.saturating_add(len) > slots.len() {
            return false;
        }
        if matches!(raw_total, Some(total) if next > total) {
            return false;
        }
        staging[self.written..next].copy_from_slice(&slots[at..at + len]);
        self.written = next;
        self.slot_occupied[slot] = false;
        true
    }
}

/// Byte capacity of the reorder-slot area the caller lends per PIPE session.
pub const fn pipe_slot_bytes() -> usize {
    PIPE_REORDER_SLOTS * PIPE_REORDER_SLOT_SIZE
}

/// Staging capacity for one START kind: exact frame for raw, the shared
/// client-capped bound for compressed.
pub const fn staging_capacity(kind: StartKind, frame_bytes: usize) -> usize {
    match kind {
        StartKind::Raw => frame_bytes,
        StartKind::Compressed { .. } => COMPRESSED_STAGING_MAX,
    }
}

/// Outcome of one 0x81 DATA frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeDataOutcome {
    /// No reply: discarded, deduplicated-quiet, or no session.
    Silent,
    /// Cadence or gap SACK.
    Sack(Response),
    /// Fatal NACK; further DATA discards until next START/disconnect.
    Fatal(Response),
    /// Raw prefix reached the negotiated total: finalize as an END.
    Complete,
}

/// Gate result for one legacy 0x72 END after the staging was taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyEndGate {
    /// Short of the frame: drop the staging, client restarts from START.
    Incomplete,
    /// Complete but wrong refresh mode: restore the staging so a corrected
    /// END can still commit it.
    RefreshUnsupported,
    /// Malformed payload on a complete staging: restore it like the refresh
    /// case (the client retries with a well-formed END), NACKed as
    /// incomplete.
    Malformed,
    /// Ready to enqueue for the display owner.
    Ready,
}

/// Gate result for one PIPE 0x82 END after the session was taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeEndGate {
    /// A fatal DATA NACK already fired: drop, no scan.
    PriorError,
    /// Complete but wrong refresh mode: restore the session so a corrected
    /// END can still commit it.
    RefreshUnsupportedRestore,
    /// Malformed payload: drop the session.
    Malformed,
    /// Queued gap or short prefix: drop, client restarts from START.
    Incomplete,
    /// Ready to enqueue (raw) or decode-then-enqueue (compressed).
    Ready,
}

/// Result of enqueueing one accepted END for the display owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitOutcome {
    /// Scan pending; the radio loop arms its outcome deadline.
    Armed,
    /// Retired between take and arm (OFF won the race): drop the frame and
    /// answer timeout honestly.
    Retired,
}

/// Transfer state for a display geometry. The caller supplies frame buffers
/// with each operation that handles payload bytes.
#[derive(Clone, Debug)]
pub struct TransferEngine {
    frame_bytes: usize,
    ledger: TransferLedger,
    legacy: Option<LegacyTransfer>,
    pipe: Option<PipeTransfer>,
    pending: Option<u64>,
}

impl TransferEngine {
    /// None when geometry has zero or out-of-wire extents, or unaligned rows.
    pub fn new(geometry: DisplayGeometry) -> Option<Self> {
        Some(Self {
            frame_bytes: geometry.frame_bytes()?,
            ledger: TransferLedger::new(),
            legacy: None,
            pipe: None,
            pending: None,
        })
    }

    pub fn frame_bytes(&self) -> usize {
        self.frame_bytes
    }

    /// Mint a generation for a legacy START, retiring any live transfer and
    /// dropping all staged/pending state. Returns the new id plus the
    /// retired live generation, if any, so the caller reports it first.
    /// The caller reserves staging after this; a reservation failure must
    /// call [`abort_all`](Self::abort_all) since the epoch already moved.
    pub fn begin_legacy_start(&mut self) -> (u64, Option<u64>) {
        let minted = self.ledger.supersede();
        self.legacy = None;
        self.pipe = None;
        self.pending = None;
        minted
    }

    /// Mint a generation for a PIPE START after the caller reserved staging
    /// plus slots, so a reservation failure NACKs without retiring the live
    /// generation. Returns the new id plus the retired live one, if any.
    pub fn begin_pipe(&mut self, req: &PipeStartReq, params: &PipeParams) -> (u64, Option<u64>) {
        let minted = self.ledger.supersede();
        self.legacy = None;
        self.pipe = None;
        self.pending = None;
        let (id, retired) = minted;
        self.pipe = Some(PipeTransfer {
            id,
            receiver: Receiver::new(params.window, params.ack_every),
            total: req.total_size as usize,
            compressed: params.compressed,
            written: 0,
            slot_occupied: [false; PIPE_REORDER_SLOTS],
            slot_seq: [0; PIPE_REORDER_SLOTS],
            slot_len: [0; PIPE_REORDER_SLOTS],
            error: false,
        });
        (id, retired)
    }

    /// Retire the current generation and drop all staged/pending state.
    /// Returns the retired live generation, if any.
    pub fn abort_all(&mut self) -> Option<u64> {
        let retired = self.ledger.abort();
        self.legacy = None;
        self.pipe = None;
        self.pending = None;
        retired
    }

    /// Stage a START's initial bytes into caller `staging` (sized
    /// [`staging_capacity`] for the kind). False when the slice is short.
    pub fn stage_legacy_initial(
        &mut self,
        id: u64,
        kind: StartKind,
        initial: &[u8],
        staging: &mut [u8],
    ) -> bool {
        let capacity = staging_capacity(kind, self.frame_bytes);
        if staging.len() < capacity || initial.len() > capacity {
            return false;
        }
        staging[..initial.len()].copy_from_slice(initial);
        self.legacy = Some(LegacyTransfer {
            id,
            received: initial.len(),
            compressed: matches!(kind, StartKind::Compressed { .. }),
        });
        true
    }

    /// Append one 0x71 chunk. Err carries the wire NACK code
    /// (`NACK_NO_SESSION` / `NACK_OVERFLOW`, empty included).
    pub fn append_legacy(&mut self, chunk: &[u8], staging: &mut [u8]) -> Result<(usize, bool), u8> {
        if chunk.is_empty() || chunk.len() > protocol::DATA_CHUNK_MAX {
            return Err(NACK_OVERFLOW);
        }
        let Some(transfer) = self.legacy.as_mut() else {
            return Err(NACK_NO_SESSION);
        };
        let next = if transfer.compressed {
            protocol::validate_compressed_append(transfer.received, chunk.len())
        } else {
            protocol::validate_data_append(transfer.received, chunk.len(), self.frame_bytes)
        };
        let next = match next {
            Ok(next) => next,
            Err(_) => return Err(NACK_OVERFLOW),
        };
        if next > staging.len() {
            return Err(NACK_OVERFLOW);
        }
        staging[transfer.received..next].copy_from_slice(chunk);
        transfer.received = next;
        Ok((next, transfer.compressed))
    }

    /// Take the legacy staging for END processing.
    pub fn take_legacy(&mut self) -> Option<LegacyTransfer> {
        self.legacy.take()
    }

    /// Restore a complete staging after a wrong-refresh-mode END so a
    /// corrected END can still commit it.
    pub fn restore_legacy(&mut self, transfer: LegacyTransfer) {
        self.legacy = Some(transfer);
    }

    /// Gate one legacy END after staging was taken and (if compressed)
    /// decoded to the exact frame.
    pub fn legacy_end_gate(&self, received: usize, payload: &[u8]) -> LegacyEndGate {
        if received != self.frame_bytes {
            return LegacyEndGate::Incomplete;
        }
        match protocol::validate_end_payload(payload) {
            Ok(()) => LegacyEndGate::Ready,
            Err(protocol::EndReject::UnsupportedRefreshMode) => LegacyEndGate::RefreshUnsupported,
            Err(protocol::EndReject::Malformed) => LegacyEndGate::Malformed,
        }
    }

    /// Serve one PIPE_WRITE DATA frame. ACKs are synchronous: a cadence ACK
    /// every N in-order accepts, an immediate SACK when a gap opens,
    /// rate-limited SACKs while it stays open. No timer involved; loss
    /// recovery is clocked by the client's tail-flush probe plus SACKs.
    pub fn pipe_data(
        &mut self,
        seq: u8,
        data: &[u8],
        staging: &mut [u8],
        slots: &mut [u8],
    ) -> PipeDataOutcome {
        if data.is_empty() {
            return PipeDataOutcome::Silent;
        }
        let Some(transfer) = self.pipe.as_mut() else {
            return PipeDataOutcome::Silent;
        };
        if transfer.error {
            return PipeDataOutcome::Silent;
        }
        if data.len() > PIPE_REORDER_SLOT_SIZE {
            let nack = transfer.fatal(protocol::PIPE_DATA_NACK_WRITE);
            transfer.error = true;
            return PipeDataOutcome::Fatal(nack);
        }
        match transfer.receiver.classify(seq) {
            FrameClass::InOrder => {
                let next = transfer.written.saturating_add(data.len());
                if next > staging.len() {
                    let nack = transfer.fatal(protocol::PIPE_DATA_NACK_WRITE);
                    transfer.error = true;
                    return PipeDataOutcome::Fatal(nack);
                }
                // A caller may reuse a larger compressed-transfer buffer.
                // Raw transfers still stop at the negotiated frame size.
                if !transfer.compressed && next > transfer.total {
                    let nack = transfer.fatal(protocol::PIPE_DATA_NACK_WRITE);
                    transfer.error = true;
                    return PipeDataOutcome::Fatal(nack);
                }
                staging[transfer.written..next].copy_from_slice(data);
                transfer.written = next;
                let mut ack_due = transfer.receiver.accept_in_order(seq);
                let mut fatal: Option<Response> = None;
                let raw_total = (!transfer.compressed).then_some(transfer.total);
                while transfer.slot_has(transfer.receiver.expected_seq()) {
                    let next_seq = transfer.receiver.expected_seq();
                    if !transfer.drain_slot(next_seq, staging, slots, raw_total) {
                        let nack = transfer.fatal(protocol::PIPE_DATA_NACK_WRITE);
                        transfer.error = true;
                        fatal = Some(nack);
                        break;
                    }
                    ack_due |= transfer.receiver.drain_queued();
                }
                if let Some(nack) = fatal {
                    return PipeDataOutcome::Fatal(nack);
                }
                if transfer.receiver.queued() == 0 {
                    transfer.receiver.clear_gap();
                }
                // Uncompressed full-frame auto-complete: the total is known,
                // so reaching it finalizes without an END, mirroring the
                // canonical handler. Compressed transfers never
                // auto-complete: only the explicit END carries their
                // completion proof.
                if !transfer.compressed && transfer.written >= transfer.total {
                    PipeDataOutcome::Complete
                } else if ack_due {
                    PipeDataOutcome::Sack(transfer.sack())
                } else {
                    PipeDataOutcome::Silent
                }
            }
            FrameClass::Ahead => {
                let slot = Receiver::slot_index(seq);
                let fresh = !(transfer.slot_occupied[slot] && transfer.slot_seq[slot] == seq);
                if fresh && transfer.receiver.queued() as usize >= PIPE_REORDER_SLOTS {
                    let nack = transfer.fatal(protocol::PIPE_DATA_NACK_WRITE);
                    transfer.error = true;
                    return PipeDataOutcome::Fatal(nack);
                }
                let at = slot * PIPE_REORDER_SLOT_SIZE;
                if at.saturating_add(data.len()) > slots.len() {
                    let nack = transfer.fatal(protocol::PIPE_DATA_NACK_WRITE);
                    transfer.error = true;
                    return PipeDataOutcome::Fatal(nack);
                }
                slots[at..at + data.len()].copy_from_slice(data);
                transfer.slot_occupied[slot] = true;
                transfer.slot_seq[slot] = seq;
                transfer.slot_len[slot] = data.len() as u16;
                if transfer.receiver.buffer_ahead(seq, fresh) {
                    PipeDataOutcome::Sack(transfer.sack())
                } else {
                    PipeDataOutcome::Silent
                }
            }
            FrameClass::Duplicate => {
                if transfer.receiver.note_duplicate() {
                    PipeDataOutcome::Sack(transfer.sack())
                } else {
                    PipeDataOutcome::Silent
                }
            }
            FrameClass::OutOfWindow => {
                let nack = transfer.fatal(protocol::PIPE_DATA_NACK_STATE);
                transfer.error = true;
                PipeDataOutcome::Fatal(nack)
            }
        }
    }

    /// Current SACK snapshot for a live PIPE session, if any. Used for the
    /// tail-flush SACK before the END result and the auto-complete SACK.
    pub fn pipe_sack(&self) -> Option<Response> {
        self.pipe.as_ref().map(PipeTransfer::sack)
    }

    /// Read-only PIPE receiver position for status/tests:
    /// `(expected_seq, queued, gap_open)`. None when no session is live.
    pub fn pipe_status(&self) -> Option<(u8, u32, bool)> {
        let transfer = self.pipe.as_ref()?;
        Some((
            transfer.receiver.expected_seq(),
            transfer.receiver.queued(),
            transfer.receiver.gap_open(),
        ))
    }

    /// Abnormal-end snapshot for the UART trace: `(id, written, expected,
    /// highest_seen, ack_mask, queued)`. None when no session is live.
    pub fn pipe_stall_snapshot(&self) -> Option<(u64, usize, u8, u8, u32, u32)> {
        let transfer = self.pipe.as_ref()?;
        let (highest_seen, ack_mask) = transfer.receiver.sack(|seq| transfer.slot_has(seq));
        Some((
            transfer.id,
            transfer.written,
            transfer.receiver.expected_seq(),
            highest_seen,
            ack_mask,
            transfer.receiver.queued(),
        ))
    }

    /// Take the PIPE session for END processing.
    pub fn take_pipe(&mut self) -> Option<PipeTransfer> {
        self.pipe.take()
    }

    /// Restore a complete session after a wrong-refresh-mode END so a
    /// corrected END can still commit it.
    pub fn restore_pipe(&mut self, transfer: PipeTransfer) {
        self.pipe = Some(transfer);
    }

    /// Gate one PIPE END after the session was taken. Raw sessions must
    /// cover the negotiated total exactly; compressed sessions decode first
    /// (the adapter reports decode failure itself) and then gate as Ready.
    pub fn pipe_end_gate(&self, transfer: &PipeTransfer, payload: &[u8]) -> PipeEndGate {
        if transfer.error {
            return PipeEndGate::PriorError;
        }
        match pipe::validate_pipe_end(payload) {
            Ok(()) => {}
            Err(pipe::PipeEndReject::UnsupportedRefreshMode) => {
                return PipeEndGate::RefreshUnsupportedRestore;
            }
            Err(pipe::PipeEndReject::Malformed) => return PipeEndGate::Malformed,
        }
        if transfer.receiver.queued() > 0 {
            return PipeEndGate::Incomplete;
        }
        if !transfer.compressed && transfer.written != transfer.total {
            return PipeEndGate::Incomplete;
        }
        PipeEndGate::Ready
    }

    /// Shared commit for one accepted END (legacy 0x72, PIPE 0x82 explicit,
    /// or PIPE raw auto-complete): arm the generation for the display
    /// owner's scan. The caller enqueues its own pending frame before this
    /// (adapter-owned bytes) and notifies the END ACK after `Armed`, so an
    /// accepted END always has a scan pending. A duplicate END after commit
    /// finds no session and can never scan the panel twice.
    pub fn commit(&mut self, id: u64) -> CommitOutcome {
        if !self.ledger.await_outcome(id) {
            // Retired between take and arm (OFF won the race): report
            // honestly instead of arming a dead generation.
            self.pending = None;
            return CommitOutcome::Retired;
        }
        self.pending = Some(id);
        CommitOutcome::Armed
    }

    /// Display-owner side: take the scan-pending generation, if any. The
    /// caller takes its own frame bytes alongside this id.
    pub fn take_pending(&mut self) -> Option<u64> {
        self.pending.take()
    }

    pub fn pending_present(&self) -> bool {
        self.pending.is_some()
    }

    /// True while a transfer is staged, decoding, scan-pending, or awaiting
    /// its outcome. The app screen reports Receiving while this holds.
    pub fn transfer_in_flight(&self) -> bool {
        self.pending.is_some()
            || self.pipe.is_some()
            || self.legacy.is_some()
            || self.ledger.in_flight()
    }

    /// Staged stream bytes across both paths (display/status diagnostics).
    pub fn staged_bytes(&self) -> usize {
        let legacy = self.legacy.as_ref().map_or(0, |t| t.received);
        let pipe = self.pipe.as_ref().map_or(0, |t| t.written);
        legacy + pipe
    }

    /// Live generation currently awaited, if any.
    pub fn live_generation(&self) -> Option<u64> {
        self.ledger.live()
    }

    pub fn is_current(&self, id: u64) -> bool {
        self.ledger.is_current(id)
    }

    /// Settle an arrived display outcome. True only for the live
    /// generation; stale outcomes change nothing. The outcome is reported
    /// only for a presented scan, so no false success reaches the client
    /// for an unseen image — the engine never resolves by itself.
    pub fn resolve_outcome(&mut self, id: u64) -> bool {
        self.ledger.resolve(id)
    }

    /// Decode one compressed staging into the exact frame. Shared with the
    /// legacy path: truncated, corrupt, checksum-invalid, over-expanding,
    /// and trailing streams all fail with no publish and no 0x73.
    pub fn decode_staging(
        staged: &[u8],
        out: &mut [u8],
        state: &mut miniz_oxide::inflate::stream::InflateState,
        frame_bytes: usize,
    ) -> Result<(), DecodeError> {
        protocol::decode_compressed_frame(staged, out, state, frame_bytes)
    }
}
