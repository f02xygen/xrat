use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::TuiEvent;
use crate::tui::keymap::TuiKeyEvent;

const ESCAPE_TIMEOUT: Duration = Duration::from_millis(25);

#[derive(Default)]
pub(super) struct TerminalInput {
    decoder: qwertty::SemanticDecoder,
    events: VecDeque<TuiEvent>,
    escape_since: Option<Instant>,
}

impl TerminalInput {
    pub(super) fn feed(&mut self, bytes: &[u8]) {
        let events = self.decoder.feed(bytes);
        self.extend(events);
    }

    pub(super) fn settle(&mut self, now: Instant) {
        if self.decoder.has_settled_text() {
            let events = self.decoder.finish();
            self.extend(events);
        }
        if self.decoder.has_pending_lone_escape() {
            self.escape_since.get_or_insert(now);
        } else {
            self.escape_since = None;
        }
    }

    pub(super) fn wait_duration(&self, timeout: Duration, now: Instant) -> Duration {
        self.escape_since.map_or(timeout, |since| {
            timeout.min(ESCAPE_TIMEOUT.saturating_sub(now.duration_since(since)))
        })
    }

    pub(super) fn pop(&mut self, now: Instant) -> Option<TuiEvent> {
        if self
            .escape_since
            .is_some_and(|since| now.duration_since(since) >= ESCAPE_TIMEOUT)
        {
            if let Some(event) = self.decoder.flush_pending_escape() {
                self.extend([event]);
            }
            self.escape_since = None;
        }
        self.events.pop_front()
    }

    fn extend(&mut self, events: impl IntoIterator<Item = qwertty::Event>) {
        self.events
            .extend(events.into_iter().filter_map(|event| match event {
                qwertty::Event::Key(key) if key.kind() != qwertty::event::KeyEventKind::Release => {
                    Some(TuiEvent::Key(TuiKeyEvent::from(&key)))
                }
                qwertty::Event::Resize(resize) => {
                    let cells = resize.cells();
                    Some(TuiEvent::Resize(cells.columns(), cells.rows()))
                }
                qwertty::Event::Paste(paste) => {
                    paste.as_str().map(|text| TuiEvent::Paste(text.to_string()))
                }
                _ => None,
            }));
    }
}

#[cfg(test)]
mod tests;
