/// Reassembles upstream SSE records that arrive fragmented across TCP chunks.
///
/// ResponsesViaChat has no codec decoder, so a record split across TCP frames
/// would otherwise be fed to [`convert_openai_sse_to_responses`] as several
/// half-records and silently dropped (mid-JSON fragments never parse). Only
/// complete records are returned here. This mirrors the `encode_responses_buffered`
/// reassembly in the StreamPumpCore path — tool names / call ids / argument
/// fragments are lost without it.
#[derive(Default)]
pub struct ResponsesSseAssembler {
    pending: Vec<u8>,
}

impl ResponsesSseAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one upstream chunk; returns every COMPLETE SSE record it contains.
    /// A record whose terminator hasn't arrived yet is buffered for the next call.
    ///
    /// Takes raw bytes, not `&str`: a TCP/HTTP chunk boundary may fall inside a
    /// UTF-8 codepoint (common with 3-byte CJK text), so callers must not gate
    /// on `str::from_utf8`.  Bytes are buffered and only COMPLETE records are
    /// decoded, so a mid-codepoint split is reassembled across calls.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.pending.extend_from_slice(chunk);
        let mut records = Vec::new();
        while let Some(end) = crate::protocol::codec::sse::record_end(&self.pending) {
            let record: Vec<u8> = self.pending.drain(..end).collect();
            records.push(String::from_utf8_lossy(&record).into_owned());
        }
        records
    }

    /// Flush any trailing bytes at EOF as a final record (a record that
    /// terminated exactly at EOF must not be lost).
    pub fn flush(&mut self) -> Vec<String> {
        if self.pending.is_empty() {
            return Vec::new();
        }
        let tail = std::mem::take(&mut self.pending);
        vec![String::from_utf8_lossy(&tail).into_owned()]
    }
}
