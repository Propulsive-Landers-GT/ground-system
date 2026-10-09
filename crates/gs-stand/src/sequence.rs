//! Sequence files (`stand/sequences/*.toml`), the action vocabulary and the MTV profile DSL.
//!
//! Vocabulary (one action per step, lower case, whitespace separated):
//!
//! ```text
//! valve <id> open|close        id: omv igv ofill oiso ovnt pumv pufill puiso puvnt lfvnt
//!                              (Arduino spellings ovent/puvent/lfvent accepted)
//! output igniter|daq_sync on|off
//! loadcell engine|nitrous|rcs begin|end
//! mtv <percent>                0..=100
//! ```
//!
//! MTV profile DSL (from legacy `get_angle_profile`): space separated `hold-<pct>-<dur>` and
//! `ramp-<pct1>-<pct2>-<dur>` segments, played back to back from `start_t`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use gs_protocol::{StandOutput, ValveId};
use serde::Deserialize;

/// Valves the actuation Arduino can drive, with the sketch's device name.
pub const STAND_VALVES: &[(ValveId, &str)] = &[
    (ValveId::Omv, "omv"),
    (ValveId::OVnt, "ovent"),
    (ValveId::PuIso, "puiso"),
    (ValveId::IgV, "igv"),
    (ValveId::OFill, "ofill"),
    (ValveId::LfVnt, "lfvent"),
    (ValveId::PuFill, "pufill"),
    (ValveId::OIso, "oiso"),
    (ValveId::PuMv, "pumv"),
    (ValveId::PuVnt, "puvent"),
];

/// Arduino device name for a valve, `None` for valves this stand does not have.
pub fn arduino_device(id: ValveId) -> Option<&'static str> {
    STAND_VALVES.iter().find(|(v, _)| *v == id).map(|(_, n)| *n)
}

/// Short tag used in sequence files and events.
pub fn valve_tag(id: ValveId) -> &'static str {
    match id {
        ValveId::Omv => "omv",
        ValveId::Mtv => "mtv",
        ValveId::IgV => "igv",
        ValveId::OFill => "ofill",
        ValveId::OIso => "oiso",
        ValveId::OVnt => "ovnt",
        ValveId::PuMv => "pumv",
        ValveId::PuFill => "pufill",
        ValveId::PuIso => "puiso",
        ValveId::PuVnt => "puvnt",
        ValveId::PuMvnt => "pumvnt",
        ValveId::LfVnt => "lfvnt",
        ValveId::TVnt => "tvnt",
        ValveId::Rcs1 => "rcs1",
        ValveId::Rcs2 => "rcs2",
    }
}

pub fn parse_valve(s: &str) -> Result<ValveId> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "omv" => ValveId::Omv,
        "igv" => ValveId::IgV,
        "ofill" => ValveId::OFill,
        "oiso" => ValveId::OIso,
        "ovnt" | "ovent" => ValveId::OVnt,
        "pumv" => ValveId::PuMv,
        "pufill" => ValveId::PuFill,
        "puiso" => ValveId::PuIso,
        "puvnt" | "puvent" => ValveId::PuVnt,
        "lfvnt" | "lfvent" => ValveId::LfVnt,
        // Valid protocol ids that this stand cannot drive: name them in the error.
        "mtv" => bail!("valve 'mtv' is throttled: use `mtv <percent>`"),
        "pumvnt" | "tvnt" | "rcs1" | "rcs2" => {
            bail!("valve '{s}' is not wired to the test-stand Arduino")
        }
        _ => bail!("unknown valve '{s}'"),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Valve { id: ValveId, open: bool },
    Output { id: StandOutput, on: bool },
    Loadcell { name: String, begin: bool },
    Mtv(f32),
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Valve { id, open } => write!(
                f,
                "{} {}",
                valve_tag(*id).to_uppercase(),
                if *open { "open" } else { "close" }
            ),
            Action::Output { id, on } => write!(
                f,
                "{} {}",
                match id {
                    StandOutput::Igniter => "igniter",
                    StandOutput::DaqSync => "daq_sync",
                },
                if *on { "on" } else { "off" }
            ),
            Action::Loadcell { name, begin } => write!(
                f,
                "{name} load cell {}",
                if *begin { "begin" } else { "end" }
            ),
            Action::Mtv(p) => write!(f, "MTV {p:.1} %"),
        }
    }
}

pub fn parse_action(s: &str) -> Result<Action> {
    let words: Vec<&str> = s.split_whitespace().collect();
    match words.as_slice() {
        ["valve", id, state] => {
            let id = parse_valve(id)?;
            let open = match *state {
                "open" => true,
                "close" | "closed" => false,
                other => bail!("valve state must be open|close, got '{other}'"),
            };
            Ok(Action::Valve { id, open })
        }
        ["output", id, state] => {
            let id = match *id {
                "igniter" => StandOutput::Igniter,
                "daq_sync" | "sync" => StandOutput::DaqSync,
                other => bail!("output must be igniter|daq_sync, got '{other}'"),
            };
            let on = match *state {
                "on" => true,
                "off" => false,
                other => bail!("output state must be on|off, got '{other}'"),
            };
            Ok(Action::Output { id, on })
        }
        ["loadcell", name, what] => {
            if !matches!(*name, "engine" | "nitrous" | "rcs") {
                bail!("loadcell must be engine|nitrous|rcs, got '{name}'");
            }
            let begin = match *what {
                "begin" => true,
                "end" => false,
                other => bail!("loadcell action must be begin|end, got '{other}'"),
            };
            Ok(Action::Loadcell {
                name: name.to_string(),
                begin,
            })
        }
        ["mtv", pct] => {
            let p: f32 = pct
                .parse()
                .map_err(|_| anyhow!("mtv percent '{pct}' is not a number"))?;
            if !(0.0..=100.0).contains(&p) {
                bail!("mtv percent {p} outside 0..=100");
            }
            Ok(Action::Mtv(p))
        }
        [] => bail!("empty action"),
        _ => bail!("unrecognised action '{s}'"),
    }
}

// ---------------------------------------------------------------------------
// MTV profile DSL
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Hold { percent: f32, duration: f32 },
    Ramp { from: f32, to: f32, duration: f32 },
}

impl Segment {
    fn duration(&self) -> f32 {
        match self {
            Segment::Hold { duration, .. } | Segment::Ramp { duration, .. } => *duration,
        }
    }
}

fn parse_pct(s: &str, what: &str) -> Result<f32> {
    let v: f32 = s
        .parse()
        .map_err(|_| anyhow!("{what} '{s}' is not a number"))?;
    if !(0.0..=100.0).contains(&v) {
        bail!("{what} {v} outside 0..=100");
    }
    Ok(v)
}

fn parse_dur(s: &str) -> Result<f32> {
    let v: f32 = s
        .parse()
        .map_err(|_| anyhow!("duration '{s}' is not a number"))?;
    if v < 0.0 {
        bail!("duration {v} is negative");
    }
    Ok(v)
}

pub fn parse_profile(dsl: &str) -> Result<Vec<Segment>> {
    let mut out = Vec::new();
    for word in dsl.split_whitespace() {
        let parts: Vec<&str> = word.split('-').collect();
        let seg = match parts.as_slice() {
            ["hold", pct, dur] => Segment::Hold {
                percent: parse_pct(pct, "hold percent")?,
                duration: parse_dur(dur)?,
            },
            ["ramp", p1, p2, dur] => Segment::Ramp {
                from: parse_pct(p1, "ramp start")?,
                to: parse_pct(p2, "ramp end")?,
                duration: parse_dur(dur)?,
            },
            _ => bail!(
                "bad profile segment '{word}' (want hold-<pct>-<dur> or ramp-<p1>-<p2>-<dur>)"
            ),
        };
        out.push(seg);
    }
    if out.is_empty() {
        bail!("empty MTV profile");
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq)]
pub struct MtvProfile {
    pub start_t: f32,
    pub segments: Vec<Segment>,
    /// Command the first segment's start percent at T-0 (legacy `mtv.command(START_ANGLE)`
    /// before the sequence) so the valve is positioned before the profile begins.
    pub preposition: bool,
}

impl MtvProfile {
    pub fn duration(&self) -> f32 {
        self.segments.iter().map(Segment::duration).sum()
    }

    pub fn end_t(&self) -> f32 {
        self.start_t + self.duration()
    }

    pub fn initial_percent(&self) -> f32 {
        match &self.segments[0] {
            Segment::Hold { percent, .. } => *percent,
            Segment::Ramp { from, .. } => *from,
        }
    }

    pub fn final_percent(&self) -> f32 {
        match self.segments.last().unwrap() {
            Segment::Hold { percent, .. } => *percent,
            Segment::Ramp { to, .. } => *to,
        }
    }

    /// Commanded percent at sequence time `t`, or `None` outside `[start_t, end_t)`.
    pub fn percent_at(&self, t: f32) -> Option<f32> {
        let mut seg_start = self.start_t;
        for seg in &self.segments {
            let seg_end = seg_start + seg.duration();
            if t >= seg_start && t < seg_end {
                return Some(match seg {
                    Segment::Hold { percent, .. } => *percent,
                    Segment::Ramp { from, to, duration } => {
                        from + (to - from) * (t - seg_start) / duration
                    }
                });
            }
            seg_start = seg_end;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Sequence files
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub t: f32,
    pub action: Action,
    /// The action text as written in the file.
    pub raw: String,
}

impl Step {
    pub fn describe(&self) -> String {
        format!("T+{:.1} {}", self.t, self.action)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    pub name: String,
    pub description: String,
    /// Sorted by time; equal times keep file order.
    pub steps: Vec<Step>,
    pub mtv_profile: Option<MtvProfile>,
    /// Last step time, or the profile end if later.
    pub duration_s: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SequenceFile {
    name: Option<String>,
    #[serde(default)]
    description: String,
    mtv_profile: Option<ProfileFile>,
    #[serde(default)]
    step: Vec<StepFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileFile {
    start_t: f32,
    profile: String,
    #[serde(default = "default_true")]
    preposition: bool,
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepFile {
    t: f32,
    action: String,
}

impl Sequence {
    pub fn parse(default_name: &str, text: &str) -> Result<Self> {
        let file: SequenceFile = toml::from_str(text)?;
        let name = file.name.unwrap_or_else(|| default_name.to_string());

        let mut errors = Vec::new();
        let mut steps = Vec::new();
        for (i, s) in file.step.iter().enumerate() {
            if s.t < 0.0 || !s.t.is_finite() {
                errors.push(format!("step {} (t={}): time must be >= 0", i + 1, s.t));
                continue;
            }
            match parse_action(&s.action) {
                Ok(action) => steps.push(Step {
                    t: s.t,
                    action,
                    raw: s.action.clone(),
                }),
                Err(e) => errors.push(format!("step {} (t={}): {e}", i + 1, s.t)),
            }
        }
        let mtv_profile = match file.mtv_profile {
            Some(p) => match parse_profile(&p.profile) {
                Ok(segments) => Some(MtvProfile {
                    start_t: p.start_t,
                    segments,
                    preposition: p.preposition,
                }),
                Err(e) => {
                    errors.push(format!("mtv_profile: {e}"));
                    None
                }
            },
            None => None,
        };
        if !errors.is_empty() {
            bail!("sequence '{name}':\n  {}", errors.join("\n  "));
        }
        if steps.is_empty() && mtv_profile.is_none() {
            bail!("sequence '{name}' has no steps");
        }
        // Stable sort keeps file order for equal times.
        steps.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
        let last_step = steps.last().map(|s| s.t).unwrap_or(0.0);
        let duration_s = mtv_profile
            .as_ref()
            .map(|p| p.end_t().max(last_step))
            .unwrap_or(last_step);
        Ok(Sequence {
            name,
            description: file.description,
            steps,
            mtv_profile,
            duration_s,
        })
    }

    pub fn load_file(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("sequence");
        Self::parse(stem, &text).with_context(|| format!("in {}", path.display()))
    }
}

/// Load every `*.toml` in `dir`, keyed by name. Any bad file fails the whole load.
pub fn load_dir(dir: &Path) -> Result<BTreeMap<String, Sequence>> {
    let mut out = BTreeMap::new();
    let mut errors = Vec::new();
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading sequence dir {}", dir.display()))?;
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    for p in paths {
        match Sequence::load_file(&p) {
            Ok(seq) => {
                if out.contains_key(&seq.name) {
                    errors.push(format!("duplicate sequence name '{}'", seq.name));
                }
                out.insert(seq.name.clone(), seq);
            }
            Err(e) => errors.push(format!("{e:#}")),
        }
    }
    if !errors.is_empty() {
        bail!("sequence load failed:\n{}", errors.join("\n"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_action_form() {
        assert_eq!(
            parse_action("valve omv open").unwrap(),
            Action::Valve {
                id: ValveId::Omv,
                open: true
            }
        );
        assert_eq!(
            parse_action("valve ovent close").unwrap(),
            Action::Valve {
                id: ValveId::OVnt,
                open: false
            }
        );
        assert_eq!(
            parse_action("output igniter on").unwrap(),
            Action::Output {
                id: StandOutput::Igniter,
                on: true
            }
        );
        assert_eq!(
            parse_action("output daq_sync off").unwrap(),
            Action::Output {
                id: StandOutput::DaqSync,
                on: false
            }
        );
        assert_eq!(
            parse_action("loadcell nitrous begin").unwrap(),
            Action::Loadcell {
                name: "nitrous".into(),
                begin: true
            }
        );
        assert_eq!(parse_action("  mtv   20 ").unwrap(), Action::Mtv(20.0));
    }

    #[test]
    fn rejects_bad_actions() {
        for bad in [
            "",
            "valve",
            "valve omv",
            "valve omv ajar",
            "valve nope open",
            "valve mtv open",
            "valve rcs1 open",
            "output kaboom on",
            "output igniter maybe",
            "loadcell tank begin",
            "loadcell engine start",
            "mtv",
            "mtv abc",
            "mtv 101",
            "mtv -1",
            "frobnicate",
        ] {
            assert!(parse_action(bad).is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn unknown_valve_error_names_it() {
        let e = parse_action("valve foo open").unwrap_err().to_string();
        assert!(e.contains("foo"), "{e}");
    }

    #[test]
    fn dsl_hold_and_ramp() {
        let segs = parse_profile("hold-15-0.25 ramp-15-100-1.25 hold-100-4.5").unwrap();
        assert_eq!(segs.len(), 3);
        let p = MtvProfile {
            start_t: 10.0,
            segments: segs,
            preposition: true,
        };
        assert!((p.duration() - 6.0).abs() < 1e-5);
        assert!((p.end_t() - 16.0).abs() < 1e-5);
        assert_eq!(p.initial_percent(), 15.0);
        assert_eq!(p.final_percent(), 100.0);
        assert_eq!(p.percent_at(9.99), None);
        assert_eq!(p.percent_at(10.0), Some(15.0));
        assert_eq!(p.percent_at(10.2), Some(15.0));
        // Midway through the ramp: 15 + 85 * 0.5
        let mid = p.percent_at(10.25 + 0.625).unwrap();
        assert!((mid - 57.5).abs() < 1e-3, "{mid}");
        assert_eq!(p.percent_at(12.0), Some(100.0));
        assert_eq!(p.percent_at(16.0), None);
    }

    #[test]
    fn dsl_rejects_garbage() {
        for bad in ["", "100-8", "hold-20", "ramp-0-100", "hold-x-1", "hold-120-1", "hold-20--1"] {
            assert!(parse_profile(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn sequence_parse_sorts_and_measures() {
        let text = r#"
description = "test"
[mtv_profile]
start_t = 5.0
profile = "hold-20-8"
[[step]]
t = 1.0
action = "valve omv open"
[[step]]
t = 0.5
action = "output igniter on"
[[step]]
t = 1.0
action = "output igniter off"
"#;
        let s = Sequence::parse("x", text).unwrap();
        assert_eq!(s.name, "x");
        assert_eq!(s.steps[0].t, 0.5);
        assert_eq!(s.steps[1].raw, "valve omv open");
        assert_eq!(s.steps[2].raw, "output igniter off");
        assert!((s.duration_s - 13.0).abs() < 1e-5);
        assert_eq!(s.steps[0].describe(), "T+0.5 igniter on");
    }

    #[test]
    fn sequence_rejects_unknown_valves_listing_all() {
        let text = r#"
[[step]]
t = 0
action = "valve foo open"
[[step]]
t = 1
action = "valve bar close"
[[step]]
t = 2
action = "valve omv open"
"#;
        let e = Sequence::parse("bad", text).unwrap_err().to_string();
        assert!(e.contains("foo") && e.contains("bar"), "{e}");
    }

    #[test]
    fn sequence_rejects_unknown_keys() {
        assert!(Sequence::parse("x", "[[step]]\nt = 0\naction = \"mtv 1\"\nfoo = 1").is_err());
    }

    #[test]
    fn shipped_sequences_load() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stand/sequences");
        let seqs = load_dir(&dir).unwrap();
        for name in ["hotfire", "rcs", "coldflow", "igniter_check"] {
            assert!(seqs.contains_key(name), "missing {name}");
        }
        let hf = &seqs["hotfire"];
        assert!((hf.duration_s - 63.8).abs() < 1e-3, "{}", hf.duration_s);
        assert_eq!(hf.mtv_profile.as_ref().unwrap().start_t, 53.6);
    }
}
