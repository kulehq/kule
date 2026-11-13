//! What a task's commands write to the person's terminal, with the
//! sequences a terminal acts on beyond the screen taken out: setting the
//! clipboard (OSC 52), the strings some terminals pass on or run (DCS,
//! APC, PM, SOS, tmux's passthrough among them), window operations and
//! the reports whose answer the terminal types back (`CSI … t`), and every
//! OSC but the few that only show (a title, the folder, a link, prompt
//! marks, colours). Colours, cursor moves and the rest pass as written, so
//! a program on a terminal looks as it would.
//!
//! The filter reads a stream in pieces as they come: a sequence split
//! across two reads is judged whole.

/// The OSC numbers that pass: titles (0, 1, 2), colours and their resets
/// (4, 10, 11, 12, 104, 110, 111, 112), the working folder (7), links (8),
/// prompt marks (133, and VS Code's 633).
const SHOWN_OSC: &[u32] = &[0, 1, 2, 4, 7, 8, 10, 11, 12, 104, 110, 111, 112, 133, 633];
/// The longest OSC that passes; a longer one is dropped.
const MAX_OSC: usize = 8 << 10;
/// The longest control sequence (`ESC [ …`) kept to judge.
const MAX_CSI: usize = 256;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Text,
    /// Just after ESC.
    Escape,
    /// In `ESC [`, its bytes kept until the final one says what it is.
    Csi,
    /// In `ESC ]`, its number not read yet.
    OscNumber,
    /// In an OSC that passes, kept until it ends.
    OscShown,
    /// In a string that does not pass, read to its end and dropped: an OSC
    /// ends at BEL too, the others (DCS, APC, PM, SOS) only at ST.
    Dropped {
        bel_ends: bool,
    },
    /// After ESC inside an OSC or a dropped string: `\` ends it.
    StringEscape {
        shown: bool,
        bel_ends: bool,
    },
}

/// The filter, fed one read at a time.
#[derive(Debug)]
pub struct Filter {
    state: State,
    held: Vec<u8>,
    number: Option<u32>,
}

impl Default for Filter {
    fn default() -> Self {
        Filter::new()
    }
}

impl Filter {
    pub fn new() -> Filter {
        Filter {
            state: State::Text,
            held: Vec::new(),
            number: Some(0),
        }
    }

    /// What of `bytes` goes on to the terminal, after what came before.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len());
        for &b in bytes {
            self.step(b, &mut out);
        }
        out
    }

    fn step(&mut self, b: u8, out: &mut Vec<u8>) {
        match self.state {
            State::Text => {
                if b == ESC {
                    self.state = State::Escape;
                } else {
                    out.push(b);
                }
            }
            State::Escape => match b {
                b'[' => {
                    self.held.clear();
                    self.held.extend_from_slice(&[ESC, b'[']);
                    self.state = State::Csi;
                }
                b']' => {
                    self.held.clear();
                    self.held.extend_from_slice(&[ESC, b']']);
                    self.number = Some(0);
                    self.state = State::OscNumber;
                }
                b'P' | b'_' | b'^' | b'X' => self.state = State::Dropped { bel_ends: false },
                ESC => out.push(ESC),
                _ => {
                    out.extend_from_slice(&[ESC, b]);
                    self.state = State::Text;
                }
            },
            State::Csi => {
                self.held.push(b);
                match b {
                    0x40..=0x7e => {
                        // Window operations, and the reports a terminal
                        // answers by typing them back.
                        if b != b't' {
                            out.extend_from_slice(&self.held);
                        }
                        self.held.clear();
                        self.state = State::Text;
                    }
                    0x20..=0x3f if self.held.len() <= MAX_CSI => {}
                    // Cut short, or not a sequence a terminal would read:
                    // not passed on.
                    _ => {
                        self.held.clear();
                        self.state = if b == ESC { State::Escape } else { State::Text };
                    }
                }
            }
            State::OscNumber => match b {
                b'0'..=b'9' => {
                    self.held.push(b);
                    self.number = self
                        .number
                        .and_then(|n| n.checked_mul(10))
                        .and_then(|n| n.checked_add(u32::from(b - b'0')));
                }
                b';' | BEL | ESC => {
                    // A number read, and one that only shows.
                    let shown =
                        self.held.len() > 2 && self.number.is_some_and(|n| SHOWN_OSC.contains(&n));
                    match (b, shown) {
                        (b';', true) => {
                            self.held.push(b';');
                            self.state = State::OscShown;
                        }
                        // Ended right after its number (`ESC ] 104 BEL`).
                        (BEL, true) => {
                            self.held.push(BEL);
                            out.extend_from_slice(&self.held);
                            self.held.clear();
                            self.state = State::Text;
                        }
                        (ESC, _) => {
                            self.state = State::StringEscape {
                                shown,
                                bel_ends: true,
                            }
                        }
                        (BEL, false) => {
                            self.held.clear();
                            self.state = State::Text;
                        }
                        _ => {
                            self.held.clear();
                            self.state = State::Dropped { bel_ends: true };
                        }
                    }
                }
                CAN | SUB => {
                    self.held.clear();
                    self.state = State::Text;
                }
                _ => {
                    self.held.clear();
                    self.state = State::Dropped { bel_ends: true };
                }
            },
            State::OscShown => match b {
                BEL => {
                    self.held.push(BEL);
                    out.extend_from_slice(&self.held);
                    self.held.clear();
                    self.state = State::Text;
                }
                ESC => {
                    self.state = State::StringEscape {
                        shown: true,
                        bel_ends: true,
                    }
                }
                CAN | SUB => {
                    self.held.clear();
                    self.state = State::Text;
                }
                _ => {
                    self.held.push(b);
                    if self.held.len() > MAX_OSC {
                        self.held.clear();
                        self.state = State::Dropped { bel_ends: true };
                    }
                }
            },
            State::Dropped { bel_ends } => match b {
                CAN | SUB => self.state = State::Text,
                BEL if bel_ends => self.state = State::Text,
                ESC => {
                    self.state = State::StringEscape {
                        shown: false,
                        bel_ends,
                    }
                }
                _ => {}
            },
            State::StringEscape { shown, bel_ends } => {
                if b == b'\\' {
                    if shown {
                        self.held.extend_from_slice(&[ESC, b'\\']);
                        out.extend_from_slice(&self.held);
                    }
                    self.held.clear();
                    self.state = State::Text;
                } else if shown {
                    // ESC inside the text: it ends the string, as terminals
                    // read it; what it starts is read afresh.
                    self.held.clear();
                    self.state = State::Escape;
                    self.step(b, out);
                } else if b == ESC {
                    self.state = State::StringEscape {
                        shown: false,
                        bel_ends,
                    };
                } else {
                    self.state = State::Dropped { bel_ends };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Filter;

    fn through(pieces: &[&[u8]]) -> Vec<u8> {
        let mut filter = Filter::new();
        pieces.iter().flat_map(|p| filter.feed(p)).collect()
    }

    #[test]
    fn the_clipboard_sequence_never_passes_however_it_comes() {
        let set = b"\x1b]52;c;a3VsZS1wcm9iZQ==\x07";
        assert_eq!(through(&[b"a", set, b"b"]), b"ab");
        assert_eq!(through(&[b"a\x1b]52;c;eA==\x1b\\b"]), b"ab");
        // Split across reads, at every point.
        let whole = [b"x".as_slice(), set, b"y"].concat();
        for at in 0..whole.len() {
            let (first, second) = whole.split_at(at);
            assert_eq!(through(&[first, second]), b"xy", "split at {at}");
        }
        // Wrapped for tmux to pass on, or in any string a terminal reads.
        assert_eq!(
            through(&[b"a\x1bPtmux;\x1b\x1b]52;c;eA==\x07\x1b\\b"]),
            b"ab"
        );
        for start in *b"P_^X" {
            let s = [b"a\x1b".as_slice(), &[start], b"payload\x1b\\b"].concat();
            assert_eq!(through(&[&s]), b"ab");
        }
        // An OSC whose number is not plain, or not one that only shows.
        assert_eq!(through(&[b"a\x1b]1337;File=x\x07b"]), b"ab");
        assert_eq!(through(&[b"a\x1b]5x2;c;eA==\x07b"]), b"ab");
        assert_eq!(through(&[b"a\x1b]99999999999;x\x07b"]), b"ab");
    }

    #[test]
    fn window_operations_and_reports_are_dropped() {
        // The title reported back as typed text, a resize, an iconify.
        assert_eq!(through(&[b"a\x1b[21tb\x1b[8;1;1tc\x1b[2td"]), b"abcd");
    }

    #[test]
    fn what_only_shows_passes_as_written() {
        for kept in [
            b"\x1b[1;31mred\x1b[0m".as_slice(),
            b"\x1b[2J\x1b[H\x1b[?1049h\x1b[?2004l",
            b"\x1b]0;a title\x07",
            b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\",
            b"\x1b]7;file:///tmp\x07",
            b"\x1b]133;A\x07",
            b"\x1b(B\x1b=\x1b>",
            "plain text, ünïcode\r\n".as_bytes(),
        ] {
            assert_eq!(
                through(&[kept]),
                kept,
                "{:?}",
                String::from_utf8_lossy(kept)
            );
            // In pieces too.
            let pieces: Vec<&[u8]> = kept.chunks(3).collect();
            assert_eq!(through(&pieces), kept);
        }
    }
}
