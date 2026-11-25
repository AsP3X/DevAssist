//! Says the window's log lines on standard error: warnings and errors always, info lines too when the
//! environment variable `DEVASSIST_LOG` is `info`.

use std::io::Write;

use log::{Level, LevelFilter, Log, Metadata, Record};

/// The environment variable that asks for info lines too.
pub const VARIABLE: &str = "DEVASSIST_LOG";

/// A logger that writes to standard error.
struct Stderr {
    /// Whether info lines are written too.
    info: bool,
}

impl Log for Stderr {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        shown(metadata.level(), self.info)
    }

    fn log(&self, record: &Record<'_>) {
        if let Some(line) = line(record.level(), self.info, &record.args().to_string()) {
            // Nothing is left to say a failed write to standard error on.
            let _ = writeln!(std::io::stderr(), "{line}");
        }
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
    }
}

/// The logger without info lines.
static QUIET: Stderr = Stderr { info: false };
/// The logger with info lines.
static TALKATIVE: Stderr = Stderr { info: true };

/// Whether a line of `level` is written, with info lines asked for or not.
fn shown(level: Level, info: bool) -> bool {
    level <= if info { Level::Info } else { Level::Warn }
}

/// The line written for `message` at `level`, or `None` when that level is not written.
fn line(level: Level, info: bool, message: &str) -> Option<String> {
    shown(level, info).then(|| format!("devassist-gui: {message}"))
}

/// Whether the value of [`VARIABLE`] asks for info lines.
fn wants_info(value: Option<&str>) -> bool {
    value.is_some_and(|value| value.trim().eq_ignore_ascii_case("info"))
}

/// Installs the logger, reading [`VARIABLE`]. A second call leaves the first logger in place.
pub fn install() {
    let info = wants_info(std::env::var(VARIABLE).ok().as_deref());
    let logger = if info { &TALKATIVE } else { &QUIET };
    if log::set_logger(logger).is_ok() {
        log::set_max_level(if info { LevelFilter::Info } else { LevelFilter::Warn });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warnings_and_errors_are_always_written_with_the_programs_name() {
        for info in [false, true] {
            assert_eq!(
                line(Level::Error, info, "the backdrop lake does not decode").as_deref(),
                Some("devassist-gui: the backdrop lake does not decode")
            );
            assert_eq!(line(Level::Warn, info, "drawn frosted instead").as_deref(), Some("devassist-gui: drawn frosted instead"));
        }
    }

    #[test]
    fn info_lines_are_written_only_when_asked_for() {
        assert_eq!(line(Level::Info, false, "baked 4 panes in 30 ms"), None);
        assert_eq!(line(Level::Info, true, "baked 4 panes in 30 ms").as_deref(), Some("devassist-gui: baked 4 panes in 30 ms"));
        for info in [false, true] {
            assert_eq!(line(Level::Debug, info, "x"), None);
            assert_eq!(line(Level::Trace, info, "x"), None);
        }
    }

    #[test]
    fn info_is_asked_for_by_the_word_info() {
        assert!(wants_info(Some("info")));
        assert!(wants_info(Some(" INFO ")));
        assert!(!wants_info(None));
        assert!(!wants_info(Some("")));
        assert!(!wants_info(Some("warn")));
        assert!(!wants_info(Some("1")));
    }
}
