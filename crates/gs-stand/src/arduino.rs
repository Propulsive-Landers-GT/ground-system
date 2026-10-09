//! The sketch's serial protocol (`jetson stuff/Arduino/test_stand/test_stand.ino`).
//!
//! Commands are ASCII `"<device> <action>"` with **no terminator**; the sketch frames by a
//! 10 ms silence, then drains anything else in its buffer. Replies are `println` lines:
//! `connected` at reset, `<name> loadcell ready` after setup, `<DEVICE> is <0|1>` on status,
//! and load-cell streams like `engine:1.23 nitrous:4.56`.

use std::io::{self, Read, Write};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Which sketch build a port turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Actuation,
    Loadcell,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Role::Actuation => "actuation",
            Role::Loadcell => "loadcell",
        })
    }
}

/// A line from a board, classified.
#[derive(Debug, Clone, PartialEq)]
pub enum BoardMsg {
    /// `connected` — the sketch just (re)started.
    Connected,
    /// `engine loadcell ready`
    LoadcellReady(String),
    /// `SYNC is 1`, `OMV is 0`, `OISO_OPEN is 1`, ...
    Status { device: String, value: i32 },
    /// `engine:1.23 nitrous:4.56` — at least one `name:number` token.
    Loadcells(Vec<(String, f32)>),
    /// Anything else (a bare number from `<name> read`, debug prints, garbage).
    Other(String),
}

pub fn classify(line: &str) -> BoardMsg {
    let line = line.trim();
    if line == "connected" {
        return BoardMsg::Connected;
    }
    if let Some(name) = line.strip_suffix(" loadcell ready") {
        return BoardMsg::LoadcellReady(name.to_string());
    }
    if let Some((dev, val)) = line.split_once(" is ") {
        if let Ok(v) = val.trim().parse::<i32>() {
            if !dev.is_empty() && !dev.contains(' ') {
                return BoardMsg::Status {
                    device: dev.to_string(),
                    value: v,
                };
            }
        }
    }
    let cells = parse_loadcell_line(line);
    if !cells.is_empty() {
        return BoardMsg::Loadcells(cells);
    }
    BoardMsg::Other(line.to_string())
}

/// Tokens of the form `name:number`; malformed tokens are skipped.
pub fn parse_loadcell_line(line: &str) -> Vec<(String, f32)> {
    line.split_whitespace()
        .filter_map(|tok| {
            let (name, val) = tok.split_once(':')?;
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            let v: f32 = val.parse().ok()?;
            if !v.is_finite() {
                return None;
            }
            Some((name.to_string(), v))
        })
        .collect()
}

/// Splits a byte stream into `\n`-terminated lines, tolerant of `\r`, partial reads and
/// non-UTF-8 garbage. Unterminated data is capped so a broken board cannot grow memory.
#[derive(Debug, Default)]
pub struct LineParser {
    buf: Vec<u8>,
    /// A runaway line is being discarded until the next newline.
    discarding: bool,
}

const MAX_LINE: usize = 512;

impl LineParser {
    pub fn push(&mut self, bytes: &[u8], out: &mut Vec<String>) {
        for &b in bytes {
            if b == b'\n' {
                if !self.discarding {
                    let s = String::from_utf8_lossy(&self.buf).trim().to_string();
                    if !s.is_empty() {
                        out.push(s);
                    }
                }
                self.buf.clear();
                self.discarding = false;
            } else if self.discarding {
                // skip
            } else if self.buf.len() < MAX_LINE {
                self.buf.push(b);
            } else {
                self.buf.clear();
                self.discarding = true;
            }
        }
    }
}

/// One serial (or fake) port.
pub trait Board: Send {
    fn path(&self) -> &str;
    /// Write one framed command. No terminator: the sketch frames by silence.
    fn write_cmd(&mut self, cmd: &str) -> io::Result<()>;
    /// Complete lines received since the last call. `Err` means the link is gone.
    fn poll_lines(&mut self) -> io::Result<Vec<String>>;
}

/// Opens a port and identifies it by role.
pub trait PortFactory: Send {
    /// Paths worth trying right now (already-open ones are filtered by the caller).
    fn candidates(&self) -> Vec<String>;
    fn open(&self, path: &str, baud: u32) -> io::Result<Box<dyn Board>>;
}

// ---------------------------------------------------------------------------
// Real serial port
// ---------------------------------------------------------------------------

pub struct SerialBoard {
    path: String,
    port: Box<dyn serialport::SerialPort>,
    rx: mpsc::Receiver<io::Result<String>>,
}

impl SerialBoard {
    pub fn open(path: &str, baud: u32) -> io::Result<Self> {
        let mut port = serialport::new(path, baud)
            .timeout(Duration::from_millis(20))
            .open()
            .map_err(io::Error::other)?;
        // Legacy pulses DTR to force a clean reset so the `connected` banner is seen.
        let _ = port.write_data_terminal_ready(false);
        thread::sleep(Duration::from_millis(100));
        let _ = port.write_data_terminal_ready(true);
        let _ = port.clear(serialport::ClearBuffer::All);

        let mut reader = port.try_clone().map_err(io::Error::other)?;
        let (tx, rx) = mpsc::channel();
        let name = path.to_string();
        thread::Builder::new()
            .name(format!("serial-rx {name}"))
            .spawn(move || {
                let mut parser = LineParser::default();
                let mut buf = [0u8; 256];
                let mut lines = Vec::new();
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => thread::sleep(Duration::from_millis(5)),
                        Ok(n) => {
                            parser.push(&buf[..n], &mut lines);
                            for l in lines.drain(..) {
                                if tx.send(Ok(l)).is_err() {
                                    return;
                                }
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            return;
                        }
                    }
                }
            })
            .map_err(io::Error::other)?;
        Ok(Self {
            path: path.to_string(),
            port,
            rx,
        })
    }
}

impl Board for SerialBoard {
    fn path(&self) -> &str {
        &self.path
    }

    fn write_cmd(&mut self, cmd: &str) -> io::Result<()> {
        self.port.write_all(cmd.as_bytes())?;
        self.port.flush()
    }

    fn poll_lines(&mut self) -> io::Result<Vec<String>> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(Ok(line)) => out.push(line),
                Ok(Err(e)) => return Err(e),
                Err(mpsc::TryRecvError::Empty) => return Ok(out),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(io::Error::other("reader thread exited"))
                }
            }
        }
    }
}

/// Real ports: explicit paths or an `auto` scan of the usual USB CDC names.
pub struct SerialFactory {
    pub explicit: Vec<String>,
}

impl PortFactory for SerialFactory {
    fn candidates(&self) -> Vec<String> {
        let mut out: Vec<String> = self.explicit.clone();
        if let Ok(rd) = std::fs::read_dir("/dev") {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let usb_cdc = name.starts_with("ttyACM")
                    || name.starts_with("stand-")
                    || name.starts_with("cu.usbmodem");
                if usb_cdc {
                    out.push(format!("/dev/{name}"));
                }
            }
        }
        if let Ok(ports) = serialport::available_ports() {
            for p in ports {
                if matches!(p.port_type, serialport::SerialPortType::UsbPort(_)) {
                    out.push(p.port_name);
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn open(&self, path: &str, baud: u32) -> io::Result<Box<dyn Board>> {
        Ok(Box::new(SerialBoard::open(path, baud)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_loadcell_stream() {
        assert_eq!(
            parse_loadcell_line("engine:1.23 nitrous:4.56"),
            vec![("engine".to_string(), 1.23), ("nitrous".to_string(), 4.56)]
        );
        assert_eq!(
            parse_loadcell_line("rcs:-0.50"),
            vec![("rcs".to_string(), -0.5)]
        );
        // Garbage tokens skipped, good ones kept.
        assert_eq!(
            parse_loadcell_line("engine:abc nitrous:2 :3 x:nan"),
            vec![("nitrous".to_string(), 2.0)]
        );
        assert!(parse_loadcell_line("connected").is_empty());
        assert!(parse_loadcell_line("").is_empty());
    }

    #[test]
    fn classifies_lines() {
        assert_eq!(classify("connected\r"), BoardMsg::Connected);
        assert_eq!(
            classify("engine loadcell ready"),
            BoardMsg::LoadcellReady("engine".into())
        );
        assert_eq!(
            classify("SYNC is 1"),
            BoardMsg::Status {
                device: "SYNC".into(),
                value: 1
            }
        );
        assert_eq!(
            classify("OISO_CLOSE is 0"),
            BoardMsg::Status {
                device: "OISO_CLOSE".into(),
                value: 0
            }
        );
        assert!(matches!(classify("engine:1.00"), BoardMsg::Loadcells(_)));
        assert_eq!(classify("12.34"), BoardMsg::Other("12.34".into()));
        assert_eq!(classify("\u{0}\u{fffd} garbage"), BoardMsg::Other("\u{0}\u{fffd} garbage".into()));
    }

    #[test]
    fn line_parser_handles_partial_and_garbage() {
        let mut p = LineParser::default();
        let mut out = Vec::new();
        p.push(b"engine:1.2", &mut out);
        assert!(out.is_empty());
        p.push(b"3 nitrous:4.56\r\nconn", &mut out);
        assert_eq!(out, vec!["engine:1.23 nitrous:4.56".to_string()]);
        out.clear();
        p.push(b"ected\n\n\r\n", &mut out);
        assert_eq!(out, vec!["connected".to_string()]);
        out.clear();
        p.push(&[0xff, 0xfe, b'\n'], &mut out);
        assert_eq!(out.len(), 1);
        assert!(matches!(classify(&out[0]), BoardMsg::Other(_)));
        out.clear();
        // Runaway line is dropped, the next proper line still arrives.
        p.push(&vec![b'a'; MAX_LINE + 100], &mut out);
        p.push(b"\nok\n", &mut out);
        assert_eq!(out, vec!["ok".to_string()]);
    }
}
