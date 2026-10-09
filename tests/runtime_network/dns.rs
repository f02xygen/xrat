use super::{
    fixtures::{self, Events},
    process::{self, ChildGuard},
};
use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
use serde_json::json;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Read, Write},
    net::{IpAddr, TcpStream},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

fn https(mut stream: TcpStream, config: Arc<ServerConfig>, events: &Events) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut connection = ServerConnection::new(config).map_err(io::Error::other)?;
    while connection.is_handshaking() {
        connection.complete_io(&mut stream)?;
    }
    let h2 = connection.alpn_protocol() == Some(b"h2");
    let mut secure = StreamOwned::new(connection, stream);
    if !h2 {
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.extend(fixtures::exact(&mut secure, 1)?);
        }
        let text = String::from_utf8_lossy(&header);
        let size: usize = text
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.trim().parse().unwrap())
            })
            .ok_or(io::ErrorKind::InvalidData)?;
        let response =
            fixtures::dns_reply(&fixtures::exact(&mut secure, size)?, events, "2001:db8::2")?;
        write!(
            secure,
            "HTTP/1.1 200 OK\r\nContent-Type: application/dns-message\r\nContent-Length: {}\r\n\r\n",
            response.len()
        )?;
        secure.write_all(&response)?;
        return secure.flush();
    }
    assert_eq!(
        fixtures::exact(&mut secure, 24)?,
        b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"
    );
    frame(&mut secure, 4, 0, 0, &[])?;
    let mut packets = HashMap::<u32, Vec<u8>>::new();
    loop {
        let header = fixtures::exact(&mut secure, 9)?;
        let length = u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize;
        let kind = header[3];
        let flags = header[4];
        let identifier = u32::from_be_bytes(header[5..].try_into().unwrap());
        let payload = fixtures::exact(&mut secure, length)?;
        if kind == 4 && flags & 1 == 0 {
            frame(&mut secure, 4, 1, 0, &[])?;
        } else if kind == 6 && flags & 1 == 0 {
            frame(&mut secure, 6, 1, 0, &payload)?;
        } else if kind == 0 {
            packets.entry(identifier).or_default().extend(payload);
            if flags & 1 != 0 {
                let response = fixtures::dns_reply(
                    &packets.remove(&identifier).unwrap(),
                    events,
                    "2001:db8::2",
                )?;
                let mut headers = vec![0x88, 0x0f, 0x10, 23];
                headers.extend(b"application/dns-message");
                frame(&mut secure, 1, 4, identifier, &headers)?;
                frame(&mut secure, 0, 1, identifier, &response)?;
            }
        }
    }
}
fn frame(
    stream: &mut (impl Write + Read),
    kind: u8,
    flags: u8,
    identifier: u32,
    payload: &[u8],
) -> io::Result<()> {
    stream.write_all(&(payload.len() as u32).to_be_bytes()[1..])?;
    stream.write_all(&[kind, flags])?;
    stream.write_all(&identifier.to_be_bytes())?;
    stream.write_all(payload)?;
    stream.flush()
}
fn tls(directory: &Path, events: Events) -> std::path::PathBuf {
    let certificate = directory.join("resolver.crt");
    let key = directory.join("resolver.key");
    process::checked(
        Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-days",
                "1",
                "-subj",
                "/CN=resolver.test",
                "-addext",
                "subjectAltName=DNS:resolver.test",
                "-keyout",
            ])
            .arg(&key)
            .arg("-out")
            .arg(&certificate),
    );
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    let certificate_der = process::checked(
        Command::new("openssl")
            .args(["x509", "-in"])
            .arg(&certificate)
            .args(["-outform", "DER"]),
    )
    .stdout;
    let key_der = process::checked(
        Command::new("openssl")
            .args(["pkcs8", "-topk8", "-nocrypt", "-in"])
            .arg(&key)
            .args(["-outform", "DER"]),
    )
    .stdout;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(certificate_der)],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der)),
        )
        .unwrap();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let config = Arc::new(config);
    fixtures::tcp_server("203.0.113.2:5443".parse().unwrap(), move |stream| {
        let _ = https(stream, config.clone(), &events);
    });
    certificate
}
fn spawn(binary: &Path, file: &Path, log: &File, certificate: &Path) -> ChildGuard {
    ChildGuard(
        Command::new(binary)
            .args(["run", "-c"])
            .arg(file)
            .env("SSL_CERT_FILE", certificate)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log.try_clone().unwrap()))
            .spawn()
            .unwrap(),
    )
}
fn wait_core(process: &mut ChildGuard, log: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            process.0.try_wait().unwrap().is_none(),
            "{}",
            fs::read_to_string(log).unwrap()
        );
        if fixtures::connect("127.0.0.1", 1080).is_ok()
            && fixtures::connect("127.0.0.1", 1053).is_ok()
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "core startup timeout: {}",
            fs::read_to_string(log).unwrap()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn lookup(name: &str, tcp: bool, kind: u16) -> (u16, Option<IpAddr>) {
    fixtures::lookup(name, tcp, kind, "127.0.0.1", 1053).unwrap()
}
fn address(name: &str, kind: u16) -> IpAddr {
    let (code, address) = lookup(name, false, kind);
    assert_eq!(code, 0);
    address.unwrap()
}

pub fn exercise() {
    let directory = process::directory("XRAT_RUNTIME_FIXTURE_DIR");
    let xray = process::path("XRAT_RUNTIME_XRAY");
    let singbox = process::path("XRAT_RUNTIME_SINGBOX");
    process::ip(&["link", "set", "lo", "up"]);
    for address in ["192.0.2.2/32", "203.0.113.2/32", "2001:db8::2/128"] {
        process::ip(&["address", "add", address, "dev", "lo"]);
    }
    let events = Events::default();
    fixtures::start(
        events.clone(),
        &["203.0.113.2", "2001:db8::2"],
        "2001:db8::2",
        false,
    );
    let certificate = tls(&directory, events.clone());
    let mut receipts = Vec::new();
    for (label, binary) in [
        ("xray", xray.clone()),
        ("xray-new", xray.with_file_name("xray-new")),
        ("sing-box", singbox),
    ] {
        for mode in [
            "real",
            "fake",
            "encrypted",
            "rewrite",
            "drop",
            "reject",
            "forward",
            "hijack",
        ] {
            if label == "sing-box" && !["real", "fake", "encrypted"].contains(&mode) {
                continue;
            }
            let file = directory.join(format!("{label}-{mode}.json"));
            assert!(file.is_file(), "missing fixture: {}", file.display());
            let log_path = directory.join(format!("{label}-{mode}.log"));
            let log = File::create(&log_path).unwrap();
            let mut core = spawn(&binary, &file, &log, &certificate);
            wait_core(&mut core, &log_path);
            let start = events.all().len();
            match mode {
                "drop" => {
                    let result = fixtures::lookup("drop.test", false, 1, "127.0.0.1", 1053);
                    assert!(
                        result.is_err_and(|error| matches!(
                            error.kind(),
                            io::ErrorKind::TimedOut
                                | io::ErrorKind::WouldBlock
                                | io::ErrorKind::ConnectionReset
                                | io::ErrorKind::ConnectionRefused
                        )),
                        "drop returned a response"
                    );
                }
                "reject" => assert_eq!(lookup("reject.test", false, 1).0, 5),
                "fake" => {
                    let fake = address("fake.test", 1);
                    let fake6 = address("fake6.test", 28);
                    assert!(
                        matches!(fake, IpAddr::V4(value) if value.octets()[..2] == [198,18] || value.octets()[..2] == [198,19])
                    );
                    assert!(
                        matches!(fake6, IpAddr::V6(value) if value.segments()[..6] == [0xfd00,0x198,0x18,0,0,0])
                    );
                    fixtures::recover_fake(fake);
                    fixtures::recover_fake(fake6);
                    assert!(
                        events.all()[start..]
                            .iter()
                            .any(|event| event[0] == "proxy" && event[1] == "fake.test"),
                        "FakeIP domain was not recovered"
                    );
                    assert_eq!(address("excluded.test", 1).to_string(), "203.0.113.2");
                    assert_eq!(address("fixed.test", 1).to_string(), "203.0.113.2");
                    let direct_start = events.all().len();
                    fixtures::recover_fake(address("fake-direct.test", 1));
                    assert!(
                        !events.all()[direct_start..]
                            .iter()
                            .any(|event| event[0] == "proxy" && event[2] == 8080),
                        "direct recovered domain was proxied"
                    );
                    if label == "sing-box" {
                        core.stop();
                        let config: serde_json::Value =
                            serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
                        let cache = Path::new(
                            config["experimental"]["cache_file"]["path"]
                                .as_str()
                                .unwrap(),
                        );
                        assert_eq!(
                            fs::metadata(cache).unwrap().permissions().mode() & 0o077,
                            0,
                            "FakeIP cache is not private"
                        );
                        core = spawn(&binary, &file, &log, &certificate);
                        wait_core(&mut core, &log_path);
                        assert_eq!(address("fake.test", 1), fake);
                        assert_eq!(address("fake6.test", 28), fake6);
                        fixtures::recover_fake(fake);
                        fixtures::recover_fake(fake6);
                    }
                }
                _ => {
                    assert_eq!(address("remote.test", 1).to_string(), "203.0.113.2");
                    assert_eq!(
                        lookup("remote-tcp.test", true, 1).1.unwrap().to_string(),
                        "203.0.113.2"
                    );
                    if mode == "encrypted" {
                        assert!(
                            events.all()[start..]
                                .iter()
                                .any(|event| event[0] == "proxy" && event[2] == 5443),
                            "encrypted DNS bypassed proxy"
                        );
                    }
                    if mode == "real" {
                        assert!(
                            events.all()[start..].contains(&json!(["proxy", "192.0.2.2", 5353])),
                            "remote DNS bypassed proxy"
                        );
                        let direct_start = events.all().len();
                        assert_eq!(address("direct.test", 1).to_string(), "203.0.113.2");
                        assert!(
                            !events.all()[direct_start..]
                                .iter()
                                .any(|event| event[0] == "proxy"),
                            "direct DNS used proxy"
                        );
                    }
                }
            }
            receipts.push(json!({"engine": label, "mode": mode, "events": events.all()[start..]}));
            println!("PASS {label}/{mode}");
            core.stop();
        }
    }
    assert_eq!(receipts.len(), 19);
    process::write_json(
        &directory.join("dns-network-receipts.json"),
        &json!(receipts),
    );
}
