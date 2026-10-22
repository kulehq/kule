//! The first message of TLS, read before a tunnel Kule does not look
//! inside goes on: the name the client asks the host for (SNI), and
//! whether it hides that name (Encrypted Client Hello). A host behind a
//! shared front (a CDN) serves whichever name the hello asks for, so a
//! tunnel opened to an allowed host could otherwise reach any other behind
//! the same front.

/// The most of the hello Kule reads: one full TLS record and its header.
pub(crate) const MAX_HELLO: usize = (1 << 14) + 5;

/// What the hello said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hello {
    /// The host names it asks for, as written.
    pub names: Vec<String>,
    /// It carries an Encrypted Client Hello (or its draft, ESNI): the
    /// name it asks for is not the one above.
    pub hidden: bool,
}

/// How far the bytes read so far go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Read {
    /// A hello begun but not whole yet.
    More,
    /// Not TLS: the first byte is not a handshake record's.
    NotTls,
    /// TLS, but not a hello Kule can read.
    Bad(&'static str),
    Hello(Hello),
}

const HANDSHAKE: u8 = 0x16;
const CLIENT_HELLO: u8 = 1;
const SERVER_NAME: u16 = 0;
const ENCRYPTED_CLIENT_HELLO: u16 = 0xfe0d;
const ESNI: u16 = 0xffce;

/// Reads the hello at the start of `bytes`, a client's first bytes in a
/// tunnel.
pub(crate) fn read(bytes: &[u8]) -> Read {
    let Some(&first) = bytes.first() else {
        return Read::More;
    };
    if first != HANDSHAKE {
        return Read::NotTls;
    }
    // The handshake message, gathered from as many records as it takes.
    let mut message = Vec::new();
    let mut at = 0;
    loop {
        if bytes.len() < at + 5 {
            return Read::More;
        }
        let header = &bytes[at..at + 5];
        if header[0] != HANDSHAKE || header[1] != 3 {
            return Read::Bad("a record that is not the hello's");
        }
        let length = usize::from(u16::from_be_bytes([header[3], header[4]]));
        if length == 0 || length > 1 << 14 {
            return Read::Bad("a record of a length TLS does not allow");
        }
        if bytes.len() < at + 5 + length {
            return if at + 5 + length > MAX_HELLO {
                Read::Bad("a hello longer than Kule reads")
            } else {
                Read::More
            };
        }
        message.extend_from_slice(&bytes[at + 5..at + 5 + length]);
        at += 5 + length;
        if message.len() >= 4 {
            if message[0] != CLIENT_HELLO {
                return Read::Bad("a first message that is not a hello");
            }
            let size = usize::from(message[1]) << 16
                | usize::from(message[2]) << 8
                | usize::from(message[3]);
            if 4 + size > MAX_HELLO {
                return Read::Bad("a hello longer than Kule reads");
            }
            if message.len() >= 4 + size {
                return match hello(&message[4..4 + size]) {
                    Some(hello) => Read::Hello(hello),
                    None => Read::Bad("a hello that does not read as TLS says"),
                };
            }
        }
    }
}

/// Bytes read one field at a time; `None` once one runs past the end.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (taken, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(taken)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }

    /// A field led by its length in `width` bytes.
    fn field(&mut self, width: usize) -> Option<&'a [u8]> {
        let n = match width {
            1 => usize::from(self.u8()?),
            _ => usize::from(self.u16()?),
        };
        self.take(n)
    }
}

fn hello(body: &[u8]) -> Option<Hello> {
    let mut c = Cursor(body);
    c.take(2)?; // legacy version
    c.take(32)?; // random
    if c.field(1)?.len() > 32 {
        return None; // session id
    }
    c.field(2)?; // cipher suites
    c.field(1)?; // compression methods
    let mut hello = Hello {
        names: Vec::new(),
        hidden: false,
    };
    // A hello may have no extensions at all (TLS 1.2 and before).
    if c.0.is_empty() {
        return Some(hello);
    }
    let mut extensions = Cursor(c.field(2)?);
    if !c.0.is_empty() {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    while !extensions.0.is_empty() {
        let kind = extensions.u16()?;
        let data = extensions.field(2)?;
        // A host takes one of two of the same; which is not Kule's to guess.
        if !seen.insert(kind) {
            return None;
        }
        match kind {
            SERVER_NAME => {
                let mut list = Cursor(data);
                let mut names = Cursor(list.field(2)?);
                if !list.0.is_empty() {
                    return None;
                }
                while !names.0.is_empty() {
                    let kind = names.u8()?;
                    let name = names.field(2)?;
                    // Only host names are defined; another kind is read
                    // by nobody Kule knows, so it is refused.
                    if kind != 0 {
                        return None;
                    }
                    hello.names.push(String::from_utf8(name.to_vec()).ok()?);
                }
            }
            ENCRYPTED_CLIENT_HELLO | ESNI => hello.hidden = true,
            _ => {}
        }
    }
    Some(hello)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A client hello as a TLS 1.3 client writes it: `names` in SNI, ECH
    /// when `hidden`, split over records of at most `record` bytes.
    pub(crate) fn hello_bytes(names: &[&str], hidden: bool, record: usize) -> Vec<u8> {
        let mut extensions = Vec::new();
        if !names.is_empty() {
            let mut list = Vec::new();
            for name in names {
                list.push(0);
                list.extend_from_slice(&(name.len() as u16).to_be_bytes());
                list.extend_from_slice(name.as_bytes());
            }
            let mut data = (list.len() as u16).to_be_bytes().to_vec();
            data.extend_from_slice(&list);
            extensions.extend_from_slice(&SERVER_NAME.to_be_bytes());
            extensions.extend_from_slice(&(data.len() as u16).to_be_bytes());
            extensions.extend_from_slice(&data);
        }
        // supported_versions: TLS 1.3.
        extensions.extend_from_slice(&[0x00, 0x2b, 0x00, 0x03, 0x02, 0x03, 0x04]);
        if hidden {
            extensions.extend_from_slice(&ENCRYPTED_CLIENT_HELLO.to_be_bytes());
            extensions.extend_from_slice(&[0x00, 0x04, 0, 1, 2, 3]);
        }
        let mut body = vec![0x03, 0x03];
        body.extend_from_slice(&[7; 32]);
        body.push(0);
        body.extend_from_slice(&[0x00, 0x02, 0x13, 0x01]);
        body.extend_from_slice(&[0x01, 0x00]);
        body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
        body.extend_from_slice(&extensions);
        let mut message = vec![CLIENT_HELLO];
        message.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        message.extend_from_slice(&body);
        let mut out = Vec::new();
        for piece in message.chunks(record) {
            out.extend_from_slice(&[HANDSHAKE, 0x03, 0x01]);
            out.extend_from_slice(&(piece.len() as u16).to_be_bytes());
            out.extend_from_slice(piece);
        }
        out
    }

    #[test]
    fn the_name_a_hello_asks_for_is_read_however_it_comes() {
        let bytes = hello_bytes(&["registry.npmjs.org"], false, 1 << 14);
        let wanted = Read::Hello(Hello {
            names: vec!["registry.npmjs.org".into()],
            hidden: false,
        });
        assert_eq!(read(&bytes), wanted);
        // Split over records, and read as far as it has come.
        let split = hello_bytes(&["registry.npmjs.org"], false, 20);
        assert_eq!(read(&split), wanted);
        for cut in [0, 1, 4, 5, 30, split.len() - 1] {
            assert_eq!(read(&split[..cut]), Read::More, "{cut}");
        }
        // No name: an address, or a client that names none.
        assert_eq!(
            read(&hello_bytes(&[], false, 1 << 14)),
            Read::Hello(Hello {
                names: Vec::new(),
                hidden: false
            })
        );
    }

    #[test]
    fn a_hidden_name_or_a_hello_that_does_not_read_is_told() {
        let Read::Hello(hello) = read(&hello_bytes(&["a.example"], true, 1 << 14)) else {
            panic!("not read");
        };
        assert!(hello.hidden);
        assert_eq!(read(b"SSH-2.0-OpenSSH_9.8\r\n"), Read::NotTls);
        assert_eq!(read(b"GET / HTTP/1.1\r\n"), Read::NotTls);
        // An alert, not a handshake, after the first record.
        let mut odd = hello_bytes(&["a.example"], false, 10);
        odd[15] = 0x15;
        assert!(matches!(read(&odd), Read::Bad(_)), "{:?}", read(&odd));
        // Two names extensions, or a name of another kind.
        let mut twice = hello_bytes(&["a.example"], false, 1 << 14);
        let at = twice.windows(2).position(|w| w == [0x00, 0x2b]).unwrap();
        twice[at + 1] = 0x00; // supported_versions becomes a second SNI
        assert!(matches!(read(&twice), Read::Bad(_)));
        // A field longer than the hello: the cipher suites', after the
        // record and message headers, the version, the random and an
        // empty session id.
        let mut long = hello_bytes(&["a.example"], false, 1 << 14);
        long[5 + 4 + 2 + 32 + 1] = 0xff;
        assert!(matches!(read(&long), Read::Bad(_)));
    }
}
