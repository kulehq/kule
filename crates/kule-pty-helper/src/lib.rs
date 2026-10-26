//! What Kule and its terminal helper say to each other. The helper runs
//! inside a task's container on Windows, where the sandbox gives a command
//! pipes and no terminal: it opens a pseudo console there, starts the
//! person's shell on it, and passes the console's output out on its own
//! output as it comes. Kule passes in what the person types and how big
//! their terminal is, as [`Frame`]s on the helper's input.
//!
//! The helper trusts nothing on its input but well-made frames: one it
//! cannot read ends the shell, and nothing is guessed.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// The most a [`Frame::Data`] holds; more is sent as several.
pub const MAX_DATA: usize = 64 * 1024;

/// What Kule sends the helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// What the person typed or pasted, for the shell to read.
    Data(Vec<u8>),
    /// The person's terminal is now `cols` × `rows`.
    Resize { cols: u16, rows: u16 },
}

const DATA: u8 = 0;
const RESIZE: u8 = 1;

/// Writes `frame`: its kind, its length (four bytes, little-endian), what
/// it holds.
pub fn write_frame(w: &mut impl Write, frame: &Frame) -> io::Result<()> {
    let (kind, body) = match frame {
        Frame::Data(bytes) if bytes.len() > MAX_DATA => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("at most {MAX_DATA} bytes go in one frame"),
            ));
        }
        Frame::Data(bytes) => (DATA, bytes.clone()),
        Frame::Resize { cols, rows } => {
            let mut body = cols.to_le_bytes().to_vec();
            body.extend(rows.to_le_bytes());
            (RESIZE, body)
        }
    };
    let mut out = Vec::with_capacity(5 + body.len());
    out.push(kind);
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    w.write_all(&out)?;
    w.flush()
}

/// Reads the next frame, or `None` where the input ended between frames.
/// A frame of a kind or length it does not know, or cut short, is an
/// error.
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Frame>> {
    let mut head = [0u8; 5];
    let mut got = 0;
    while got < head.len() {
        match r.read(&mut head[got..])? {
            0 if got == 0 => return Ok(None),
            0 => return Err(cut_short()),
            n => got += n,
        }
    }
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_owned());
    match head[0] {
        DATA if len <= MAX_DATA => {
            let mut bytes = vec![0u8; len];
            read_all(r, &mut bytes)?;
            Ok(Some(Frame::Data(bytes)))
        }
        DATA => Err(bad("a frame longer than any Kule sends")),
        RESIZE if len == 4 => {
            let mut body = [0u8; 4];
            read_all(r, &mut body)?;
            Ok(Some(Frame::Resize {
                cols: u16::from_le_bytes([body[0], body[1]]),
                rows: u16::from_le_bytes([body[2], body[3]]),
            }))
        }
        RESIZE => Err(bad("a size frame of the wrong length")),
        _ => Err(bad("a frame of a kind Kule does not send")),
    }
}

fn read_all(r: &mut impl Read, buf: &mut [u8]) -> io::Result<()> {
    r.read_exact(buf).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => cut_short(),
        _ => e,
    })
}

fn cut_short() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "a frame was cut short")
}

/// A terminal's size as the helper gives it, whatever was asked: between 2
/// and 1000 columns and rows.
pub fn clamp(cols: u16, rows: u16) -> (u16, u16) {
    (cols.clamp(2, 1000), rows.clamp(2, 1000))
}

/// What the helper is started with: `--size COLSxROWS -- PROGRAM LINE`.
/// `program` is the shell by its absolute path (the helper looks for no
/// program anywhere), `line` the shell's whole command line, as Kule built
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub cols: u16,
    pub rows: u16,
    pub program: PathBuf,
    pub line: OsString,
}

impl Args {
    /// The arguments to start the helper with.
    pub fn to_args(&self) -> Vec<OsString> {
        vec![
            "--size".into(),
            format!("{}x{}", self.cols, self.rows).into(),
            "--".into(),
            self.program.clone().into_os_string(),
            self.line.clone(),
        ]
    }

    /// Reads them back, the program's own name first.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
        let usage = "usage: kule-pty-helper --size COLSxROWS -- PROGRAM LINE";
        let args: Vec<OsString> = args.into_iter().skip(1).collect();
        let [size_flag, size, dashes, program, line] = args.as_slice() else {
            return Err(usage.to_owned());
        };
        if size_flag != "--size" || dashes != "--" {
            return Err(usage.to_owned());
        }
        let (cols, rows) = size
            .to_str()
            .and_then(|s| s.split_once('x'))
            .and_then(|(c, r)| Some((c.parse::<u16>().ok()?, r.parse::<u16>().ok()?)))
            .ok_or_else(|| format!("{size:?} is no size; {usage}"))?;
        let program = PathBuf::from(program);
        if !program.is_absolute() {
            return Err(format!(
                "{} is no absolute path: the helper looks for no program",
                program.display()
            ));
        }
        let (cols, rows) = clamp(cols, rows);
        Ok(Args {
            cols,
            rows,
            program,
            line: line.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_read_back_as_written() {
        let mut wire = Vec::new();
        let frames = [
            Frame::Data(b"dir\r".to_vec()),
            Frame::Resize {
                cols: 120,
                rows: 40,
            },
            Frame::Data(Vec::new()),
            Frame::Data(vec![7; MAX_DATA]),
        ];
        for frame in &frames {
            write_frame(&mut wire, frame).unwrap();
        }
        let mut r = wire.as_slice();
        for frame in &frames {
            assert_eq!(read_frame(&mut r).unwrap().as_ref(), Some(frame));
        }
        assert_eq!(read_frame(&mut r).unwrap(), None);
    }

    /// Nothing the helper cannot read is taken for something else: an
    /// unknown kind, a length past the most Kule sends, a size of the
    /// wrong length and a frame cut short all fail.
    #[test]
    fn a_frame_it_cannot_read_is_refused() {
        let refused = |wire: &[u8]| read_frame(&mut &wire[..]).is_err();
        assert!(refused(&[9, 0, 0, 0, 0]));
        let too_long = (MAX_DATA as u32 + 1).to_le_bytes();
        assert!(refused(&[
            0,
            too_long[0],
            too_long[1],
            too_long[2],
            too_long[3]
        ]));
        assert!(refused(&[1, 3, 0, 0, 0, 1, 2, 3]));
        assert!(refused(&[0, 4, 0, 0, 0, b'a']));
        assert!(refused(&[0, 4]));
        assert!(write_frame(&mut Vec::new(), &Frame::Data(vec![0; MAX_DATA + 1])).is_err());
    }

    #[test]
    fn its_arguments_read_back_and_name_the_program_absolutely() {
        let program = if cfg!(windows) {
            PathBuf::from("C:\\Windows\\System32\\cmd.exe")
        } else {
            PathBuf::from("/bin/sh")
        };
        let args = Args {
            cols: 100,
            rows: 30,
            program: program.clone(),
            line: "\"cmd.exe\" /d".into(),
        };
        let mut all = vec![OsString::from("kule-pty-helper")];
        all.extend(args.to_args());
        assert_eq!(Args::parse(all.clone()), Ok(args));
        // A bare name would be looked for, so it is refused.
        all[4] = "cmd.exe".into();
        assert!(Args::parse(all).unwrap_err().contains("no absolute path"));
        let sized = Args::parse(
            ["kule-pty-helper", "--size", "0x5000", "--"]
                .map(OsString::from)
                .into_iter()
                .chain([program.into_os_string(), "x".into()]),
        )
        .unwrap();
        assert_eq!((sized.cols, sized.rows), (2, 1000));
        assert!(Args::parse(["kule-pty-helper", "--size"].map(OsString::from)).is_err());
    }
}
