use super::KernelInterfaceInfo;
use std::io;

unsafe extern "C" {
    fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
    fn send(fd: i32, bytes: *const u8, size: usize, flags: i32) -> isize;
    fn recv(fd: i32, bytes: *mut u8, size: usize, flags: i32) -> isize;
    fn close(fd: i32) -> i32;
}

pub(super) fn inspect(name: &str) -> io::Result<Option<KernelInterfaceInfo>> {
    if name.len() > 15 || name.contains('\0') {
        return Ok(None);
    }
    let fd = unsafe { socket(16, 3 | 0x80000, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    struct Socket(i32);
    impl Drop for Socket {
        fn drop(&mut self) {
            unsafe {
                close(self.0);
            }
        }
    }
    let _socket = Socket(fd);
    let attribute_size = 4 + name.len() + 1;
    let mut request = vec![0; 32 + attribute_size.div_ceil(4) * 4];
    let length = request.len() as u32;
    request[..4].copy_from_slice(&length.to_ne_bytes());
    request[4..6].copy_from_slice(&18u16.to_ne_bytes());
    request[6..8].copy_from_slice(&1u16.to_ne_bytes());
    request[8..12].copy_from_slice(&1u32.to_ne_bytes());
    request[32..34].copy_from_slice(&(attribute_size as u16).to_ne_bytes());
    request[34..36].copy_from_slice(&3u16.to_ne_bytes());
    request[36..36 + name.len()].copy_from_slice(name.as_bytes());
    if unsafe { send(fd, request.as_ptr(), request.len(), 0) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut response = [0; 65536];
    let received = unsafe { recv(fd, response.as_mut_ptr(), response.len(), 0) };
    if received < 0 {
        return Err(io::Error::last_os_error());
    }
    parse(name, &response[..received as usize])
}

fn parse(name: &str, packet: &[u8]) -> io::Result<Option<KernelInterfaceInfo>> {
    if packet.len() < 20 {
        return Err(invalid("truncated netlink interface response"));
    }
    let length = u32::from_ne_bytes(
        packet[..4]
            .try_into()
            .map_err(|_| invalid("invalid header"))?,
    ) as usize;
    if length > packet.len() || length < 20 {
        return Err(invalid("invalid netlink response length"));
    }
    let kind = u16::from_ne_bytes([packet[4], packet[5]]);
    if kind == 2 {
        let error = i32::from_ne_bytes(
            packet[16..20]
                .try_into()
                .map_err(|_| invalid("invalid errno"))?,
        );
        if matches!(error, -19 | -2) {
            return Ok(None);
        }
        return Err(if error < 0 {
            io::Error::from_raw_os_error(-error)
        } else {
            invalid("unexpected netlink acknowledgement")
        });
    }
    if kind != 16 || length < 32 {
        return Err(invalid("expected a netlink link response"));
    }
    let index = i32::from_ne_bytes(
        packet[20..24]
            .try_into()
            .map_err(|_| invalid("invalid interface index"))?,
    );
    if index <= 0 {
        return Err(invalid("invalid kernel interface index"));
    }
    let attributes = attributes(&packet[32..length])?;
    let returned_name = attributes
        .iter()
        .find(|(kind, _)| *kind == 3)
        .map(|(_, value)| value.strip_suffix(&[0]).unwrap_or(value));
    if returned_name != Some(name.as_bytes()) {
        return Err(invalid("netlink interface name mismatch"));
    }
    let mut is_tun = false;
    if let Some((_, link)) = attributes.iter().find(|(kind, _)| *kind == 18) {
        let link = self::attributes(link)?;
        if link
            .iter()
            .any(|(kind, value)| *kind == 1 && *value == b"tun\0")
            && let Some((_, data)) = link.iter().find(|(kind, _)| *kind == 2)
        {
            is_tun = self::attributes(data)?
                .iter()
                .any(|(kind, value)| *kind == 3 && *value == [1]);
        }
    }
    Ok(Some(KernelInterfaceInfo {
        name: name.into(),
        ifindex: index as u32,
        is_tun,
    }))
}

fn attributes(mut bytes: &[u8]) -> io::Result<Vec<(u16, &[u8])>> {
    let mut result = Vec::new();
    while !bytes.is_empty() {
        if bytes.len() < 4 {
            return Err(invalid("truncated netlink attribute"));
        }
        let size = u16::from_ne_bytes([bytes[0], bytes[1]]) as usize;
        let kind = u16::from_ne_bytes([bytes[2], bytes[3]]) & 0x3fff;
        if size < 4 || size > bytes.len() {
            return Err(invalid("invalid netlink attribute length"));
        }
        result.push((kind, &bytes[4..size]));
        let aligned = size.div_ceil(4) * 4;
        if aligned > bytes.len() {
            return Err(invalid("missing netlink attribute padding"));
        }
        bytes = &bytes[aligned..];
    }
    Ok(result)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attribute(kind: u16, value: &[u8]) -> Vec<u8> {
        let mut result = Vec::from(((value.len() + 4) as u16).to_ne_bytes());
        result.extend(kind.to_ne_bytes());
        result.extend(value);
        result.resize(result.len().div_ceil(4) * 4, 0);
        result
    }

    #[test]
    fn inspects_tun_and_tap_metadata_from_the_current_namespace() {
        for (device_type, expected) in [(1, true), (2, false)] {
            let mut packet = vec![0; 32];
            packet[4..6].copy_from_slice(&16u16.to_ne_bytes());
            packet[20..24].copy_from_slice(&42i32.to_ne_bytes());
            packet.extend(attribute(3, b"xrat0\0"));
            let mut link = attribute(1, b"tun\0");
            link.extend(attribute(2, &attribute(3, &[device_type])));
            packet.extend(attribute(18, &link));
            let length = packet.len() as u32;
            packet[..4].copy_from_slice(&length.to_ne_bytes());
            let info = parse("xrat0", &packet).unwrap().unwrap();
            assert_eq!(info.ifindex, 42);
            assert_eq!(info.is_tun, expected);
            assert!(parse("foreign0", &packet).is_err());
            assert!(parse("xrat0", &packet[..packet.len() - 1]).is_err());
        }
    }
}
