//! Log files for `ws run`, rotated by size.
//!
//! Every task gets a file under `logs.dir` — by default the `logs` directory
//! of the branch directory it runs in — named by the
//! `logs.file` template. A file that would grow past `logs.max_size` is moved
//! to `<file>.1` — the `.1` before it to `.2`, and so on, `logs.keep` of them
//! kept — and a fresh one started, so a dev server left running for a week
//! costs a bounded amount of disk.
//!
//! What goes in is what the terminal shows, minus the colours: escape
//! sequences are stripped, and each line can start with the local time.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::config::LogsConfig;

/// Where the logs of one run go, and how they are kept.
#[derive(Debug, Clone)]
pub struct Logs {
    pub dir: PathBuf,
    template: String,
    /// The branch, folded into a single path component.
    branch: String,
    max_size: u64,
    keep: u32,
    timestamps: bool,
}

impl Logs {
    pub fn new(config: &LogsConfig, dir: PathBuf, branch: &str) -> Logs {
        Logs {
            dir,
            template: config.file.clone(),
            branch: component(branch),
            max_size: config.max_size.0,
            keep: config.keep,
            timestamps: config.timestamps,
        }
    }

    /// The file a task's output goes to.
    pub fn path(&self, repo: &str, task: &str) -> PathBuf {
        let name = self
            .template
            .replace("{branch}", &self.branch)
            .replace("{repo}", &component(repo))
            .replace("{task}", &component(task));
        self.dir.join(name)
    }

    /// One sink per `(repo, task)`, in order. Tasks whose files coincide —
    /// a template without `{task}`, say — share one open file, and each of
    /// their lines is marked with the task it came from.
    pub fn sinks(&self, jobs: &[(&str, &str)]) -> Result<Vec<Sink>> {
        let paths: Vec<PathBuf> = jobs.iter().map(|(r, t)| self.path(r, t)).collect();
        let mut open: HashMap<&Path, Arc<Mutex<RotatingFile>>> = HashMap::new();
        let mut sinks = Vec::with_capacity(jobs.len());
        for ((repo, task), path) in jobs.iter().zip(&paths) {
            let file = match open.get(path.as_path()) {
                Some(file) => file.clone(),
                None => {
                    let file = Arc::new(Mutex::new(RotatingFile::open(
                        path,
                        self.max_size,
                        self.keep,
                    )?));
                    open.insert(path, file.clone());
                    file
                }
            };
            let shared = paths.iter().filter(|p| *p == path).count() > 1;
            sinks.push(Sink {
                file,
                tag: shared.then(|| format!("{repo}:{task}")),
                timestamps: self.timestamps,
            });
        }
        Ok(sinks)
    }
}

/// A path component made of whatever names it: `feature/x` is `feature-x`.
fn component(value: &str) -> String {
    value.replace(['/', '\\'], "-")
}

/// One task's way into its log file.
#[derive(Debug, Clone)]
pub struct Sink {
    file: Arc<Mutex<RotatingFile>>,
    tag: Option<String>,
    timestamps: bool,
}

impl Sink {
    /// Write one line of output; a failure to log is reported once per file
    /// rather than stopping the task.
    pub fn line(&self, line: &str) {
        let mut out = String::with_capacity(line.len() + 40);
        if self.timestamps {
            out.push_str(&timestamp());
            out.push(' ');
        }
        if let Some(tag) = &self.tag {
            out.push_str(tag);
            out.push_str(" | ");
        }
        out.push_str(&strip_ansi(line));
        out.push('\n');
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        file.write(out.as_bytes());
    }
}

/// A file that starts over, keeping what it had as `<file>.1`, once it would
/// grow past `max_size`.
#[derive(Debug)]
pub struct RotatingFile {
    path: PathBuf,
    file: Option<File>,
    size: u64,
    max_size: u64,
    keep: u32,
    failed: bool,
}

impl RotatingFile {
    pub fn open(path: &Path, max_size: u64, keep: u32) -> Result<RotatingFile> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let file = append(path)?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(RotatingFile {
            path: path.to_path_buf(),
            file: Some(file),
            size,
            max_size,
            keep,
            failed: false,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) {
        let len = bytes.len() as u64;
        // A line longer than the limit still goes in whole, into a file of
        // its own; splitting it would garble the one thing worth reading.
        if self.max_size > 0 && self.size > 0 && self.size + len > self.max_size {
            if let Err(e) = self.rotate() {
                self.warn(&e);
            }
        }
        let Some(file) = self.file.as_mut() else {
            return;
        };
        match file.write_all(bytes) {
            Ok(()) => self.size += len,
            Err(e) => self.warn(&e.into()),
        }
    }

    /// `<file>` → `<file>.1` → `<file>.2` …, the oldest beyond `keep` deleted.
    fn rotate(&mut self) -> Result<()> {
        self.file = None;
        if self.keep == 0 {
            std::fs::remove_file(&self.path).ok();
        } else {
            std::fs::remove_file(numbered(&self.path, self.keep)).ok();
            for n in (1..self.keep).rev() {
                let from = numbered(&self.path, n);
                if from.exists() {
                    std::fs::rename(&from, numbered(&self.path, n + 1))
                        .with_context(|| format!("cannot rotate {}", from.display()))?;
                }
            }
            std::fs::rename(&self.path, numbered(&self.path, 1))
                .with_context(|| format!("cannot rotate {}", self.path.display()))?;
        }
        self.file = Some(append(&self.path)?);
        self.size = 0;
        Ok(())
    }

    fn warn(&mut self, error: &anyhow::Error) {
        if !self.failed {
            eprintln!("warning: logging to {}: {error:#}", self.path.display());
            self.failed = true;
        }
    }
}

fn append(path: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("cannot open {}", path.display()))
}

fn numbered(path: &Path, n: u32) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{n}"));
    PathBuf::from(name)
}

/// The text without its terminal escape sequences: colours, cursor movement,
/// and the OSC ones that set titles and hyperlinks.
pub fn strip_ansi(text: &str) -> std::borrow::Cow<'_, str> {
    static ANSI: OnceLock<regex::Regex> = OnceLock::new();
    if !text.contains('\x1b') {
        return text.into();
    }
    ANSI.get_or_init(|| {
        regex::Regex::new(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)|[@-Z\\-_])")
            .expect("the pattern is valid")
    })
    .replace_all(text, "")
}

/// Local time to the millisecond: `2026-10-21 14:03:22.517`.
pub fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let millis = now.subsec_millis();
    let (y, mo, d, h, mi, s) = local_time(now.as_secs() as i64);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{millis:03}")
}

#[cfg(unix)]
fn local_time(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    // SAFETY: localtime_r only writes the struct it is handed.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        let t = secs as libc::time_t;
        if libc::localtime_r(&t, &mut tm).is_null() {
            return utc(secs);
        }
        tm
    };
    (
        tm.tm_year as i64 + 1900,
        tm.tm_mon as u32 + 1,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
        tm.tm_sec as u32,
    )
}

#[cfg(not(unix))]
fn local_time(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    utc(secs)
}

/// Civil date and time of a Unix timestamp, after Howard Hinnant's
/// `civil_from_days`.
fn utc(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
        (rem % 60) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;

    #[test]
    fn colours_and_titles_are_stripped() {
        assert_eq!(strip_ansi("\x1b[1;32mok\x1b[0m done"), "ok done");
        assert_eq!(strip_ansi("\x1b]0;title\x07text"), "text");
        assert_eq!(strip_ansi("\x1b[2K\x1b[1Gprogress"), "progress");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn utc_dates_are_civil() {
        assert_eq!(utc(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(utc(951_782_400), (2000, 2, 29, 0, 0, 0));
        assert_eq!(utc(1_792_576_922), (2026, 10, 21, 10, 2, 2));
    }

    #[test]
    fn the_template_names_the_file_and_folds_slashes() {
        let cfg = parse("logs:\n  file: \"{branch}/{repo}.{task}.log\"\n").unwrap();
        let logs = Logs::new(&cfg.logs, PathBuf::from("/w/logs"), "feature/x");
        assert_eq!(
            logs.path("api", "db/seed"),
            PathBuf::from("/w/logs/feature-x/api.db-seed.log")
        );
    }

    #[test]
    fn a_full_file_rotates_and_only_keep_copies_survive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/app.log");
        let mut file = RotatingFile::open(&path, 10, 2).unwrap();
        for line in ["aaaaaaaa\n", "bbbbbbbb\n", "cccccccc\n", "dddddddd\n"] {
            file.write(line.as_bytes());
        }
        let read = |p: PathBuf| std::fs::read_to_string(p).unwrap();
        assert_eq!(read(path.clone()), "dddddddd\n");
        assert_eq!(read(numbered(&path, 1)), "cccccccc\n");
        assert_eq!(read(numbered(&path, 2)), "bbbbbbbb\n");
        assert!(!numbered(&path, 3).exists());

        // A file reopened by the next run carries on from its size.
        drop(file);
        let mut file = RotatingFile::open(&path, 10, 2).unwrap();
        file.write(b"eeeeeeee\n");
        assert_eq!(read(path.clone()), "eeeeeeee\n");
        assert_eq!(read(numbered(&path, 1)), "dddddddd\n");
    }

    #[test]
    fn shared_files_mark_each_line_with_its_task() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = parse("logs:\n  file: \"{branch}.log\"\n  timestamps: false\n").unwrap();
        let logs = Logs::new(&cfg.logs, dir.path().to_path_buf(), "main");
        let sinks = logs.sinks(&[("api", "dev"), ("web", "dev")]).unwrap();
        sinks[0].line("\x1b[32mlistening\x1b[0m");
        sinks[1].line("ready");
        let text = std::fs::read_to_string(dir.path().join("main.log")).unwrap();
        assert_eq!(text, "api:dev | listening\nweb:dev | ready\n");
    }
}
