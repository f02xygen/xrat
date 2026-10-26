use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::tui::keymap::TuiKeyEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiEvent {
    Key(TuiKeyEvent),
    Resize(u16, u16),
    Paste(String),
}

pub struct TerminalSession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    decoder: qwertty::SemanticDecoder,
    buffered_events: VecDeque<qwertty::Event>,
    last_size: (u16, u16),
}

impl TerminalSession {
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableBracketedPaste,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
            )
        )?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        let initial_size = crossterm::terminal::size().unwrap_or((0, 0));

        Ok(Self {
            terminal,
            decoder: qwertty::SemanticDecoder::new(),
            buffered_events: VecDeque::new(),
            last_size: initial_size,
        })
    }

    pub fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame<'_>),
    {
        self.terminal.draw(render).map(|_| ())
    }

    pub fn clear(&mut self) -> io::Result<()> {
        self.terminal.clear()
    }

    pub fn poll_event(&mut self, timeout: Duration) -> io::Result<Option<TuiEvent>> {
        // First check for terminal geometry changes (window resize).
        if let Ok(current_size) = crossterm::terminal::size() {
            if current_size != self.last_size {
                self.last_size = current_size;
                return Ok(Some(TuiEvent::Resize(current_size.0, current_size.1)));
            }
        }

        // Return any already decoded and buffered event.
        while let Some(event) = self.buffered_events.pop_front() {
            if let Some(tui_event) = Self::convert_event(event) {
                return Ok(Some(tui_event));
            }
        }

        // Poll for new input bytes on stdin.
        #[cfg(unix)]
        {
            use rustix::event::{PollFd, PollFlags, Timespec, poll};
            use rustix::fd::BorrowedFd;

            let stdin_fd = unsafe { BorrowedFd::borrow_raw(0) };
            let mut poll_fds = [PollFd::new(&stdin_fd, PollFlags::IN)];
            let timeout_ts = Timespec {
                tv_sec: timeout.as_secs() as _,
                tv_nsec: timeout.subsec_nanos() as _,
            };

            match poll(&mut poll_fds, Some(&timeout_ts)) {
                Ok(n) if n > 0 => {
                    let mut buf = [0u8; 512];
                    match rustix::io::read(stdin_fd, &mut buf) {
                        Ok(len) if len > 0 => {
                            let events = self.decoder.feed(&buf[..len]);
                            self.buffered_events.extend(events);
                            while let Some(event) = self.buffered_events.pop_front() {
                                if let Some(tui_event) = Self::convert_event(event) {
                                    return Ok(Some(tui_event));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => {}
                Err(e) => return Err(io::Error::from(e)),
            }
        }

        #[cfg(not(unix))]
        {
            if crossterm::event::poll(timeout)? {
                match crossterm::event::read()? {
                    crossterm::event::Event::Key(k) => {
                        return Ok(Some(TuiEvent::Key(TuiKeyEvent::from(k))));
                    }
                    crossterm::event::Event::Resize(w, h) => {
                        return Ok(Some(TuiEvent::Resize(w, h)));
                    }
                    crossterm::event::Event::Paste(text) => {
                        return Ok(Some(TuiEvent::Paste(text)));
                    }
                    _ => {}
                }
            }
        }

        Ok(None)
    }

    fn convert_event(event: qwertty::Event) -> Option<TuiEvent> {
        match event {
            qwertty::Event::Key(k) => {
                if k.kind() == qwertty::event::KeyEventKind::Release {
                    None
                } else {
                    Some(TuiEvent::Key(TuiKeyEvent::from(&k)))
                }
            }
            qwertty::Event::Resize(r) => {
                let cells = r.cells();
                Some(TuiEvent::Resize(cells.columns(), cells.rows()))
            }
            qwertty::Event::Paste(p) => p.as_str().map(|s| TuiEvent::Paste(s.to_string())),
            _ => None,
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = execute!(
            self.terminal.backend_mut(),
            PopKeyboardEnhancementFlags,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
        let _ = self.terminal.show_cursor();
    }
}
