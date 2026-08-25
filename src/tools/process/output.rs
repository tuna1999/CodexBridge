#[derive(Default)]
pub(super) struct OutputBuffer {
    /// Bounded head+tail summary of the combined stdout/stderr byte stream.
    /// Once truncation begins, `retained[..head_len]` maps to logical bytes
    /// `[0, head_len)`, while `retained[head_len..]` maps to
    /// `[tail_start, total_bytes)`. Keeping those logical ranges explicit is
    /// required for truthful replay cursors.
    pub(super) retained: Vec<u8>,
    pub(super) total_bytes: usize,
    head_len: usize,
    tail_start: usize,
    /// Highest stream offset ever rendered into a tool response. Explicit
    /// cursors below this value replay buffered history instead of advancing.
    pub(super) delivered: usize,
    pub(super) truncated: bool,
}

impl OutputBuffer {
    pub(super) fn append(&mut self, bytes: &[u8], limit: usize) {
        if bytes.is_empty() {
            return;
        }
        self.total_bytes = self.total_bytes.saturating_add(bytes.len());
        if self.total_bytes <= limit {
            self.retained.extend_from_slice(bytes);
            self.head_len = 0;
            self.tail_start = 0;
        } else if limit > 0 {
            let head_len = limit / 2;
            let tail_len = limit.saturating_sub(head_len);
            let mut head = if self.truncated {
                self.retained[..self.head_len.min(self.retained.len())].to_vec()
            } else {
                self.retained[..head_len.min(self.retained.len())].to_vec()
            };
            if head.len() > head_len {
                head.truncate(head_len);
            }

            let mut tail = if self.truncated {
                self.retained[self.head_len.min(self.retained.len())..].to_vec()
            } else {
                self.retained[head_len.min(self.retained.len())..].to_vec()
            };
            tail.extend_from_slice(bytes);
            if tail.len() > tail_len {
                tail.drain(..tail.len() - tail_len);
            }

            self.head_len = head.len();
            self.tail_start = self.total_bytes.saturating_sub(tail.len());
            self.retained = head;
            self.retained.extend_from_slice(&tail);
        } else {
            self.retained.clear();
            self.head_len = 0;
            self.tail_start = self.total_bytes;
        }
        self.truncated |= self.total_bytes > limit;
    }

    /// Render the stream window beginning at `requested` (or just after the
    /// last delivered byte) as text. Returns
    /// `(text, start_offset, next_offset, truncated_ever)`.
    ///
    /// Rendering never consumes bytes: the caller decides whether to continue
    /// from `next_offset` or replay an older cursor after a lost response.
    /// Bytes that fell out of the bounded window are disclosed with an
    /// omission marker instead of being silently skipped.
    pub(super) fn render_window(
        &mut self,
        requested: Option<usize>,
    ) -> (String, usize, usize, bool) {
        let cursor = requested.unwrap_or(self.delivered).min(self.total_bytes);
        let (text, start) = if !self.truncated {
            (
                String::from_utf8_lossy(&self.retained[cursor.min(self.retained.len())..])
                    .into_owned(),
                cursor,
            )
        } else if cursor < self.head_len {
            let mut text =
                String::from_utf8_lossy(&self.retained[cursor..self.head_len]).into_owned();
            let omitted = self.tail_start.saturating_sub(self.head_len);
            if omitted > 0 {
                text.push_str(&format!(
                    "\n\n[... {omitted} buffered bytes omitted ...]\n\n"
                ));
            }
            text.push_str(&String::from_utf8_lossy(&self.retained[self.head_len..]));
            (text, cursor)
        } else if cursor < self.tail_start {
            let omitted = self.tail_start - cursor;
            let mut text = format!("[... {omitted} buffered bytes omitted ...]\n\n");
            text.push_str(&String::from_utf8_lossy(&self.retained[self.head_len..]));
            // The first actual retained byte in this response is tail_start.
            (text, self.tail_start)
        } else {
            let tail_offset = cursor.saturating_sub(self.tail_start);
            let tail = &self.retained[self.head_len..];
            (
                String::from_utf8_lossy(&tail[tail_offset.min(tail.len())..]).into_owned(),
                cursor,
            )
        };
        self.delivered = self.total_bytes;
        (text, start, self.total_bytes, self.truncated)
    }
}

pub(super) fn token_window(text: String, max_tokens: Option<usize>) -> (String, Option<usize>) {
    let Some(max_tokens) = max_tokens.filter(|value| *value > 0) else {
        return (text, None);
    };
    // Codex approximates tokens from JavaScript string length. UTF-16 code
    // units preserve that behavior for astral Unicode instead of undercounting
    // every non-BMP character as one Rust `char`.
    let max_units = max_tokens.saturating_mul(4);
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() <= max_units {
        return (text, None);
    }
    let original = units.len().div_ceil(4);
    let head = max_units / 2;
    let tail = max_units.saturating_sub(head);
    let value = format!(
        "{}\n\n[... {} UTF-16 code units omitted ...]\n\n{}",
        String::from_utf16_lossy(&units[..head]),
        units.len().saturating_sub(max_units),
        String::from_utf16_lossy(&units[units.len() - tail..])
    );
    (value, Some(original))
}
