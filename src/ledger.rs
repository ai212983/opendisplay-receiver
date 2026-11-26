//! Generation-tracked transfer settlement and exit-redraw planning.
//!
//! A generation is minted per START; aborts (OFF, disconnect, close,
//! timeout, supersede) retire the current generation so a late display
//! outcome can never satisfy a later END. Pure state; the product wires it
//! to its tasks and outcome channel.

/// Generation-tracked refresh transaction policy shared by the radio loop,
/// the staging store and the display owner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransferLedger {
    epoch: u64,
    live: Option<u64>,
}

impl TransferLedger {
    pub const fn new() -> Self {
        Self {
            epoch: 0,
            live: None,
        }
    }

    /// Mint a generation for a new START, retiring any live one. Returns the
    /// new generation plus the retired live generation, if any, so the radio
    /// loop can report it before proceeding.
    pub fn supersede(&mut self) -> (u64, Option<u64>) {
        let retired = self.live.take();
        self.epoch = self.epoch.wrapping_add(1);
        (self.epoch, retired)
    }

    /// Whether `id` is the current generation.
    pub fn is_current(&self, id: u64) -> bool {
        id == self.epoch
    }

    /// Arm the radio loop to await `id`'s display outcome after END ACK.
    /// Returns false (leaving state untouched) for a retired generation.
    pub fn await_outcome(&mut self, id: u64) -> bool {
        if id != self.epoch {
            return false;
        }
        self.live = Some(id);
        true
    }

    /// True while a started generation awaits its outcome. Cleared by
    /// `resolve`, so it spans staging, display commit, and the scan.
    pub fn in_flight(&self) -> bool {
        self.live.is_some()
    }

    /// Settle an arrived outcome. Returns true only for the live generation;
    /// stale (late, superseded, aborted) outcomes report false and change
    /// nothing.
    pub fn resolve(&mut self, id: u64) -> bool {
        if self.live == Some(id) {
            self.live = None;
            true
        } else {
            false
        }
    }

    /// Retire the current generation and drop any awaited outcome. Returns
    /// the retired live generation, if any.
    pub fn abort(&mut self) -> Option<u64> {
        self.epoch = self.epoch.wrapping_add(1);
        self.live.take()
    }

    /// Live generation currently awaited, if any.
    pub fn live(&self) -> Option<u64> {
        self.live
    }
}

/// Exit-redraw step for one display cycle while an exit request is pending.
/// The display task peeks the request (never consuming it up front) and runs
/// at most one scan per cycle: a required recovery Cleans the retained frame
/// without rendering and keeps the request for a later local repaint; only a
/// successful local render+Clean completes the exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitStep {
    /// No request pending, or remote mode re-armed: do nothing.
    Idle,
    /// Recovery required but its retry delay has not elapsed: wait.
    WaitRetry,
    /// Recovery required and due: Clean the retained frame, keep the request.
    CleanRetained,
    /// No recovery barrier: render local UI and Clean; consume the request
    /// only if that scan succeeds.
    RenderLocal,
}

/// One-cycle exit plan from the display task's current state.
pub const fn plan_od_exit_step(
    request_pending: bool,
    remote_active: bool,
    recovery_required: bool,
    retry_due: bool,
) -> ExitStep {
    if !request_pending || remote_active {
        return ExitStep::Idle;
    }
    if recovery_required {
        if retry_due {
            return ExitStep::CleanRetained;
        }
        return ExitStep::WaitRetry;
    }
    ExitStep::RenderLocal
}

/// Display-owned exit-redraw request for the remote-mode OFF transition.
/// The serial task requests; the display task takes exactly once and runs a
/// full local repaint, so exit restoration never depends on the app-event
/// queue having room.
#[derive(Debug, Default)]
pub struct ExitRedraw {
    pending: bool,
}

impl ExitRedraw {
    pub const fn new() -> Self {
        Self { pending: false }
    }

    pub fn request(&mut self) {
        self.pending = true;
    }

    /// Take the request. Returns true exactly once per request, however many
    /// times a request is repeated while pending.
    pub fn take(&mut self) -> bool {
        core::mem::replace(&mut self.pending, false)
    }

    pub fn is_pending(&self) -> bool {
        self.pending
    }
}
