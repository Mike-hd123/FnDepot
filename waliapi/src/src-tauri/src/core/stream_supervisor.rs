//! Streaming commit-barrier supervisor (T00 decision 6 / T05).
//!
//! Fixed state machine:
//!
//! ```text
//! Planned
//!   → Connecting
//!   → UpstreamHeadersReceived
//!   → FirstFrameBufferedAndValidated
//!   → DownstreamCommitted
//!   → Streaming
//!   → Completed | Aborted
//! ```
//!
//! Upstream may be swapped BEFORE `DownstreamCommitted`; once committed, no
//! upstream/codec retry is possible — only a protocol-representable error may
//! be sent downstream.  The supervisor is a pure state machine: it performs no
//! network I/O (that is T06's transport), so it is fully unit-testable.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum StreamState {
    Planned,
    Connecting,
    UpstreamHeadersReceived,
    FirstFrameBufferedAndValidated,
    DownstreamCommitted,
    Streaming,
    Completed,
    Aborted,
}

impl StreamState {
    pub fn as_str(&self) -> &'static str {
        match self {
            StreamState::Planned => "planned",
            StreamState::Connecting => "connecting",
            StreamState::UpstreamHeadersReceived => "upstream_headers_received",
            StreamState::FirstFrameBufferedAndValidated => "first_frame_buffered_and_validated",
            StreamState::DownstreamCommitted => "downstream_committed",
            StreamState::Streaming => "streaming",
            StreamState::Completed => "completed",
            StreamState::Aborted => "aborted",
        }
    }

    /// States from which the upstream may be swapped before commit.
    fn swapable(self) -> bool {
        matches!(
            self,
            StreamState::Connecting
                | StreamState::UpstreamHeadersReceived
                | StreamState::FirstFrameBufferedAndValidated
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamTransitionError {
    InvalidTransition {
        from: StreamState,
        to: StreamState,
    },
    /// The downstream has already committed: upstream may NOT be swapped.
    RetryAfterCommit,
    /// The stream already terminated (Completed/Aborted).
    AlreadyTerminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamTimeoutKind {
    /// Waiting for the upstream to respond with headers after connect.
    Connect,
    /// Waiting for the first complete validated SSE record.
    HeaderFirstFrame,
    /// Gap between consecutive stream events after commit.
    StreamIdle,
}

impl StreamTimeoutKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            StreamTimeoutKind::Connect => "connect",
            StreamTimeoutKind::HeaderFirstFrame => "header_first_frame",
            StreamTimeoutKind::StreamIdle => "stream_idle",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamSupervisor {
    state: StreamState,
    /// Whether the 200 + first downstream bytes have been committed.
    committed: bool,
    client_cancelled: bool,
    /// Number of pre-commit upstream swaps performed.
    upstream_swaps: usize,
    /// Exactly-once terminal marker guard (`[DONE]`/`message_stop`/`response.completed`).
    terminal_emitted: bool,
    /// The timeout that last aborted the stream, if any.
    last_timeout: Option<StreamTimeoutKind>,
    /// Failure class / message recorded at abort (committed errors etc.).
    abort_reason: Option<String>,
}

impl Default for StreamSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamSupervisor {
    pub fn new() -> Self {
        Self {
            state: StreamState::Planned,
            committed: false,
            client_cancelled: false,
            upstream_swaps: 0,
            terminal_emitted: false,
            last_timeout: None,
            abort_reason: None,
        }
    }

    pub fn state(&self) -> StreamState {
        self.state
    }

    pub fn committed(&self) -> bool {
        self.committed
    }

    pub fn client_cancelled(&self) -> bool {
        self.client_cancelled
    }

    pub fn upstream_swaps(&self) -> usize {
        self.upstream_swaps
    }

    pub fn terminal_emitted(&self) -> bool {
        self.terminal_emitted
    }

    pub fn last_timeout(&self) -> Option<StreamTimeoutKind> {
        self.last_timeout
    }

    pub fn abort_reason(&self) -> Option<&str> {
        self.abort_reason.as_deref()
    }

    // --- forward transitions ---

    pub fn begin_connect(&mut self) -> Result<(), StreamTransitionError> {
        self.transition(StreamState::Planned, StreamState::Connecting)
    }

    pub fn on_upstream_headers(&mut self) -> Result<(), StreamTransitionError> {
        self.transition(
            StreamState::Connecting,
            StreamState::UpstreamHeadersReceived,
        )
    }

    /// A complete first SSE record was buffered and validated (codec-decoded
    /// for conversion, original bytes for native) before committing.
    pub fn on_first_frame_validated(&mut self) -> Result<(), StreamTransitionError> {
        self.transition(
            StreamState::UpstreamHeadersReceived,
            StreamState::FirstFrameBufferedAndValidated,
        )
    }

    /// Commit the downstream 200 + first event bytes.  After this the upstream
    /// can never be swapped.
    pub fn commit_downstream(&mut self) -> Result<(), StreamTransitionError> {
        self.transition(
            StreamState::FirstFrameBufferedAndValidated,
            StreamState::DownstreamCommitted,
        )?;
        self.committed = true;
        Ok(())
    }

    pub fn begin_streaming(&mut self) -> Result<(), StreamTransitionError> {
        self.transition(StreamState::DownstreamCommitted, StreamState::Streaming)
    }

    /// Exactly-once completion (either from Committed→empty stream, or from
    /// Streaming after the protocol's terminal marker / EOF).
    pub fn complete(&mut self) -> Result<(), StreamTransitionError> {
        if self.state == StreamState::Completed || self.state == StreamState::Aborted {
            return Err(StreamTransitionError::AlreadyTerminated);
        }
        match self.state {
            StreamState::DownstreamCommitted | StreamState::Streaming => {
                self.state = StreamState::Completed;
                Ok(())
            }
            _ => Err(StreamTransitionError::InvalidTransition {
                from: self.state,
                to: StreamState::Completed,
            }),
        }
    }

    /// Abort from any non-terminal state (pre-commit failure / post-commit
    /// upstream error / timeout / cancellation).
    pub fn abort(&mut self, reason: impl Into<String>) -> Result<(), StreamTransitionError> {
        if self.state == StreamState::Completed || self.state == StreamState::Aborted {
            return Err(StreamTransitionError::AlreadyTerminated);
        }
        self.abort_reason = Some(reason.into());
        self.state = StreamState::Aborted;
        Ok(())
    }

    // --- upstream swap (the commit barrier) ---

    /// Swap to a new upstream.  ALLOWED only before commit.  Resets the state
    /// to Connecting so headers + first-frame validation run again for the new
    /// upstream.
    pub fn swap_upstream(&mut self) -> Result<(), StreamTransitionError> {
        if self.committed {
            return Err(StreamTransitionError::RetryAfterCommit);
        }
        if self.state.swapable() {
            self.upstream_swaps += 1;
            self.state = StreamState::Connecting;
            Ok(())
        } else {
            Err(StreamTransitionError::InvalidTransition {
                from: self.state,
                to: StreamState::Connecting,
            })
        }
    }

    // --- cancellation / timeouts ---

    /// Client disconnected: cancel the upstream and record `client_cancelled`
    /// exactly once (T00 decision 6).
    pub fn client_cancel(&mut self) -> Result<(), StreamTransitionError> {
        if self.client_cancelled {
            return Err(StreamTransitionError::AlreadyTerminated);
        }
        self.client_cancelled = true;
        self.abort("client_cancelled")
    }

    /// A timeout occurred while waiting in the given phase.
    pub fn on_timeout(&mut self, kind: StreamTimeoutKind) -> Result<(), StreamTransitionError> {
        self.last_timeout = Some(kind);
        self.abort(format!("timeout:{}", kind.as_str()))
    }

    /// Register a protocol terminal marker.  Returns `false` (and leaves the
    /// guard untouched) if a terminal was already emitted — each direction may
    /// terminate exactly once (T00 decision 6).
    pub fn register_terminal(&mut self) -> bool {
        if self.terminal_emitted {
            return false;
        }
        self.terminal_emitted = true;
        true
    }

    fn transition(
        &mut self,
        from: StreamState,
        to: StreamState,
    ) -> Result<(), StreamTransitionError> {
        if self.state != from {
            return Err(StreamTransitionError::InvalidTransition {
                from: self.state,
                to,
            });
        }
        self.state = to;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn through_first_frame(s: &mut StreamSupervisor) {
        s.begin_connect().unwrap();
        s.on_upstream_headers().unwrap();
        s.on_first_frame_validated().unwrap();
    }

    #[test]
    fn happy_path_state_machine() {
        let mut s = StreamSupervisor::new();
        assert_eq!(s.state(), StreamState::Planned);
        s.begin_connect().unwrap();
        assert_eq!(s.state(), StreamState::Connecting);
        s.on_upstream_headers().unwrap();
        assert_eq!(s.state(), StreamState::UpstreamHeadersReceived);
        s.on_first_frame_validated().unwrap();
        assert_eq!(s.state(), StreamState::FirstFrameBufferedAndValidated);
        assert!(!s.committed());
        s.commit_downstream().unwrap();
        assert!(s.committed());
        assert_eq!(s.state(), StreamState::DownstreamCommitted);
        s.begin_streaming().unwrap();
        assert_eq!(s.state(), StreamState::Streaming);
        assert!(s.register_terminal());
        assert!(
            !s.register_terminal(),
            "terminal marker must be exactly-once"
        );
        s.complete().unwrap();
        assert_eq!(s.state(), StreamState::Completed);
    }

    #[test]
    fn commit_barrier_blocks_swap_after_commit() {
        let mut s = StreamSupervisor::new();
        through_first_frame(&mut s);
        assert!(s.swap_upstream().is_ok(), "swap allowed before commit");
        // Swapping reset us to Connecting; re-walk to commit.
        s.on_upstream_headers().unwrap();
        s.on_first_frame_validated().unwrap();
        s.commit_downstream().unwrap();
        // After commit, swapping (i.e. retrying a second upstream) is impossible.
        let err = s.swap_upstream().unwrap_err();
        assert_eq!(err, StreamTransitionError::RetryAfterCommit);
        assert_eq!(s.upstream_swaps(), 1);
    }

    #[test]
    fn invalid_first_frame_allows_swap_without_commit() {
        let mut s = StreamSupervisor::new();
        s.begin_connect().unwrap();
        s.on_upstream_headers().unwrap();
        // First frame failed validation → swap to the next candidate.
        assert!(s.swap_upstream().is_ok());
        assert_eq!(s.state(), StreamState::Connecting);
        assert_eq!(s.upstream_swaps(), 1);
        assert!(!s.committed());
    }

    #[test]
    fn complete_only_after_commit_or_streaming() {
        let mut s = StreamSupervisor::new();
        assert!(s.complete().is_err(), "cannot complete from Planned");
        through_first_frame(&mut s);
        s.commit_downstream().unwrap();
        assert!(
            s.complete().is_ok(),
            "empty stream completes from Committed"
        );
    }

    #[test]
    fn abort_and_cancel_terminate() {
        let mut s = StreamSupervisor::new();
        s.begin_connect().unwrap();
        s.client_cancel().unwrap();
        assert!(s.client_cancelled());
        assert_eq!(s.state(), StreamState::Aborted);
        assert_eq!(s.abort_reason(), Some("client_cancelled"));
        // Exactly-once finalizer: a second cancel is rejected.
        assert!(s.client_cancel().is_err());
    }

    #[test]
    fn timeout_classifies_phase() {
        let mut s = StreamSupervisor::new();
        s.begin_connect().unwrap();
        s.on_timeout(StreamTimeoutKind::Connect).unwrap();
        assert_eq!(s.last_timeout(), Some(StreamTimeoutKind::Connect));
        assert_eq!(s.state(), StreamState::Aborted);

        let mut s = StreamSupervisor::new();
        through_first_frame(&mut s);
        s.commit_downstream().unwrap();
        s.begin_streaming().unwrap();
        s.on_timeout(StreamTimeoutKind::StreamIdle).unwrap();
        assert_eq!(s.state(), StreamState::Aborted);
    }

    #[test]
    fn invalid_transitions_rejected() {
        let mut s = StreamSupervisor::new();
        assert!(
            s.on_upstream_headers().is_err(),
            "cannot receive headers before connecting"
        );
        assert!(
            s.commit_downstream().is_err(),
            "cannot commit before first frame"
        );
        assert!(
            s.on_first_frame_validated().is_err(),
            "cannot validate first frame before headers"
        );
    }

    #[test]
    fn state_names_are_stable() {
        let names: Vec<&str> = [
            StreamState::Planned,
            StreamState::Connecting,
            StreamState::UpstreamHeadersReceived,
            StreamState::FirstFrameBufferedAndValidated,
            StreamState::DownstreamCommitted,
            StreamState::Streaming,
            StreamState::Completed,
            StreamState::Aborted,
        ]
        .iter()
        .map(|s| s.as_str())
        .collect();
        assert_eq!(
            names,
            vec![
                "planned",
                "connecting",
                "upstream_headers_received",
                "first_frame_buffered_and_validated",
                "downstream_committed",
                "streaming",
                "completed",
                "aborted",
            ]
        );
    }
}
