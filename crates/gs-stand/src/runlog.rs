//! Per-run CSV: `logs/stand-<utc>.csv`, never overwritten. One row per load-cell sample and
//! per command / event, all with the same columns so a spreadsheet can plot and filter it.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use gs_protocol::StandMode;

pub struct RunLog {
    w: BufWriter<File>,
    path: PathBuf,
    rows_since_flush: u32,
}

/// Fields common to every row.
pub struct RowCtx<'a> {
    pub unix_s: f64,
    pub seq_t: Option<f64>,
    pub mode: StandMode,
    /// (name, latest value) for each configured load cell, in config order.
    pub cells: &'a [(String, Option<f32>)],
    pub mtv_percent: f32,
}

impl RunLog {
    pub fn open(dir: &Path, cell_names: &[String]) -> Result<Self> {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let mut path = dir.join(format!("stand-{stamp}.csv"));
        let mut n = 1;
        let file = loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(f) => break f,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    path = dir.join(format!("stand-{stamp}-{n}.csv"));
                    n += 1;
                }
                Err(e) => return Err(e).with_context(|| format!("creating {}", path.display())),
            }
        };
        let mut w = BufWriter::new(file);
        let mut header = vec!["unix_time_s".to_string(), "seq_t_s".into(), "mode".into()];
        header.extend(cell_names.iter().map(|n| format!("loadcell_{n}")));
        header.extend(["mtv_percent".to_string(), "kind".into(), "detail".into()]);
        writeln!(w, "{}", header.join(","))?;
        w.flush()?;
        Ok(Self {
            w,
            path,
            rows_since_flush: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn row(&mut self, ctx: &RowCtx, kind: &str, detail: &str) {
        let mut cols = vec![
            format!("{:.3}", ctx.unix_s),
            ctx.seq_t.map(|t| format!("{t:.3}")).unwrap_or_default(),
            format!("{:?}", ctx.mode),
        ];
        for (_, v) in ctx.cells {
            cols.push(v.map(|v| format!("{v:.3}")).unwrap_or_default());
        }
        cols.push(format!("{:.2}", ctx.mtv_percent));
        cols.push(kind.to_string());
        cols.push(csv_quote(detail));
        let _ = writeln!(self.w, "{}", cols.join(","));
        self.rows_since_flush += 1;
        // Samples arrive at ~10 Hz; flush often enough that a crash loses little.
        if kind != "sample" || self.rows_since_flush >= 5 {
            let _ = self.w.flush();
            self.rows_since_flush = 0;
        }
    }
}

fn csv_quote(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_header_and_rows_without_overwriting() {
        let dir = std::env::temp_dir().join(format!("gs-stand-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let names = vec!["engine".to_string(), "nitrous".to_string()];
        let mut a = RunLog::open(&dir, &names).unwrap();
        let b = RunLog::open(&dir, &names).unwrap();
        assert_ne!(a.path(), b.path(), "second run in the same second gets a new file");
        let cells = vec![("engine".to_string(), Some(1.5)), ("nitrous".to_string(), None)];
        a.row(
            &RowCtx {
                unix_s: 1.0,
                seq_t: Some(2.5),
                mode: StandMode::Sequence,
                cells: &cells,
                mtv_percent: 20.0,
            },
            "event",
            "T+2.5 OMV open, \"quoted\"",
        );
        let a_path = a.path().to_path_buf();
        drop(a);
        let text = std::fs::read_to_string(&a_path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "unix_time_s,seq_t_s,mode,loadcell_engine,loadcell_nitrous,mtv_percent,kind,detail"
        );
        assert_eq!(
            lines[1],
            "1.000,2.500,Sequence,1.500,,20.00,event,\"T+2.5 OMV open, \"\"quoted\"\"\""
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
