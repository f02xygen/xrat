use super::{KernelPolicyRule, KernelRuleAttribute};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

unsafe extern "C" {
    fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
    fn send(fd: i32, bytes: *const u8, size: usize, flags: i32) -> isize;
    fn recv(fd: i32, bytes: *mut u8, size: usize, flags: i32) -> isize;
}

fn open() -> io::Result<OwnedFd> {
    let fd = unsafe { socket(16, 3 | 0x80000, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn request(fd: &OwnedFd, kind: u16, flags: u16, payload: &[u8]) -> io::Result<()> {
    let mut message = Vec::from(((16 + payload.len()) as u32).to_ne_bytes());
    message.extend(kind.to_ne_bytes());
    message.extend(flags.to_ne_bytes());
    message.extend(1u32.to_ne_bytes());
    message.extend(0u32.to_ne_bytes());
    message.extend(payload);
    if unsafe { send(fd.as_raw_fd(), message.as_ptr(), message.len(), 0) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn list() -> io::Result<Vec<KernelPolicyRule>> {
    let fd = open()?;
    request(&fd, 34, 1 | 0x300, &[0; 12])?;
    let mut result = Vec::new();
    loop {
        let mut buffer = [0; 65536];
        let received = unsafe { recv(fd.as_raw_fd(), buffer.as_mut_ptr(), buffer.len(), 0) };
        if received < 0 {
            return Err(io::Error::last_os_error());
        }
        if received == 0 {
            return Err(invalid("empty policy rule dump"));
        }
        let mut bytes = &buffer[..received as usize];
        while bytes.len() >= 16 {
            let length = word(&bytes[..4])? as usize;
            if length < 16 || length > bytes.len() {
                return Err(invalid("invalid rule dump length"));
            }
            let kind = u16::from_ne_bytes([bytes[4], bytes[5]]);
            if kind == 3 {
                return Ok(result);
            }
            if kind == 2 {
                return Err(netlink_error(&bytes[16..length])?);
            }
            if kind == 32 {
                result.push(parse(&bytes[16..length])?);
            }
            let aligned = length.div_ceil(4) * 4;
            if aligned > bytes.len() {
                return Err(invalid("truncated rule padding"));
            }
            bytes = &bytes[aligned..];
        }
        if !bytes.is_empty() {
            return Err(invalid("truncated rule header"));
        }
    }
}

fn parse(payload: &[u8]) -> io::Result<KernelPolicyRule> {
    if payload.len() < 12 {
        return Err(invalid("truncated policy rule"));
    }
    let mut attributes = Vec::new();
    let mut bytes = &payload[12..];
    while !bytes.is_empty() {
        if bytes.len() < 4 {
            return Err(invalid("truncated rule attribute"));
        }
        let length = u16::from_ne_bytes([bytes[0], bytes[1]]) as usize;
        if length < 4 || length > bytes.len() {
            return Err(invalid("invalid rule attribute length"));
        }
        attributes.push(KernelRuleAttribute {
            kind: u16::from_ne_bytes([bytes[2], bytes[3]]),
            value: bytes[4..length].to_vec(),
        });
        let aligned = length.div_ceil(4) * 4;
        if aligned > bytes.len() {
            return Err(invalid("truncated rule attribute padding"));
        }
        bytes = &bytes[aligned..];
    }
    Ok(KernelPolicyRule {
        family: payload[0],
        destination_prefix: payload[1],
        source_prefix: payload[2],
        tos: payload[3],
        table: payload[4],
        action: payload[7],
        // Detached-interface and unresolved-goto flags are kernel state, not selectors.
        flags: word(&payload[8..12])? & !0x1c,
        attributes,
    })
}

pub(super) fn delete(rule: &KernelPolicyRule) -> io::Result<()> {
    if !rule.is_singbox_capture_rule() {
        return Err(invalid(
            "refusing to delete a rule outside the owned sing-box capture range",
        ));
    }
    let mut payload = vec![
        rule.family,
        rule.destination_prefix,
        rule.source_prefix,
        rule.tos,
        rule.table,
        0,
        0,
        rule.action,
    ];
    payload.extend(rule.flags.to_ne_bytes());
    for attribute in &rule.attributes {
        let length = u16::try_from(attribute.value.len() + 4)
            .map_err(|_| invalid("oversized policy rule attribute"))?;
        payload.extend(length.to_ne_bytes());
        payload.extend(attribute.kind.to_ne_bytes());
        payload.extend(&attribute.value);
        payload.resize(payload.len().div_ceil(4) * 4, 0);
    }
    let fd = open()?;
    request(&fd, 33, 1 | 4, &payload)?;
    let mut buffer = [0; 4096];
    let received = unsafe { recv(fd.as_raw_fd(), buffer.as_mut_ptr(), buffer.len(), 0) };
    if received < 0 {
        return Err(io::Error::last_os_error());
    }
    if received < 20 || u16::from_ne_bytes([buffer[4], buffer[5]]) != 2 {
        return Err(invalid("missing policy rule deletion acknowledgement"));
    }
    let code = i32::from_ne_bytes(
        buffer[16..20]
            .try_into()
            .map_err(|_| invalid("invalid errno"))?,
    );
    if matches!(code, 0 | -2 | -3) {
        return Ok(());
    }
    Err(io::Error::from_raw_os_error(-code))
}

fn word(bytes: &[u8]) -> io::Result<u32> {
    Ok(u32::from_ne_bytes(
        bytes.try_into().map_err(|_| invalid("invalid rule word"))?,
    ))
}
fn netlink_error(bytes: &[u8]) -> io::Result<io::Error> {
    Ok(io::Error::from_raw_os_error(
        -(word(
            bytes
                .get(..4)
                .ok_or_else(|| invalid("truncated rule error"))?,
        )? as i32),
    ))
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_dump_preserves_exact_selectors_and_rejects_truncated_metadata() {
        let mut payload = vec![2, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
        payload.extend(8u16.to_ne_bytes());
        payload.extend(6u16.to_ne_bytes());
        payload.extend(9001u32.to_ne_bytes());
        let rule = parse(&payload).unwrap();
        assert!(rule.is_singbox_capture_rule());
        assert_eq!(rule.priority(), Some(9001));
        payload[8..12].copy_from_slice(&0x1cu32.to_ne_bytes());
        assert_eq!(parse(&payload).unwrap(), rule);
        payload[8..12].copy_from_slice(&2u32.to_ne_bytes());
        assert_ne!(parse(&payload).unwrap(), rule);
        assert!(parse(&payload[..payload.len() - 1]).is_err());
        let mut foreign = rule;
        foreign.attributes[0].value = 32766u32.to_ne_bytes().to_vec();
        assert!(delete(&foreign).is_err());
    }
}
