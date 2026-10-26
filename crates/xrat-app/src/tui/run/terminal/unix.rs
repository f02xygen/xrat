use std::io;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};

use super::{TuiEvent, input::TerminalInput};

pub(super) fn poll_event(
    input: &mut TerminalInput,
    timeout: Duration,
) -> io::Result<Option<TuiEvent>> {
    if let Some(event) = input.pop(Instant::now()) {
        return Ok(Some(event));
    }
    let stdin = io::stdin();
    let mut descriptors = [PollFd::new(&stdin, PollFlags::IN)];
    let mut wait = input.wait_duration(timeout, Instant::now());
    loop {
        let timeout = Timespec {
            tv_sec: wait.as_secs() as _,
            tv_nsec: wait.subsec_nanos() as _,
        };
        match poll(&mut descriptors, Some(&timeout)) {
            Ok(0) => break,
            Ok(_) if descriptors[0].revents().contains(PollFlags::IN) => {
                let mut bytes = [0; 512];
                match rustix::io::read(&stdin, &mut bytes) {
                    Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                    Ok(length) => input.feed(&bytes[..length]),
                    Err(rustix::io::Errno::INTR) => continue,
                    Err(error) => return Err(error.into()),
                }
                wait = Duration::ZERO;
            }
            Ok(_) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Err(rustix::io::Errno::INTR) => break,
            Err(error) => return Err(error.into()),
        }
    }
    let now = Instant::now();
    input.settle(now);
    Ok(input.pop(now))
}
