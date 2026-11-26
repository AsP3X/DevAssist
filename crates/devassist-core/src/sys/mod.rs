//! Atoms that talk to the operating system or the network.

pub mod clipboard;
pub mod encode;
pub mod http;
pub mod open;
pub mod paths;
pub mod process;

/// A way the core reaches outside itself. Each is noted where it
/// happens, under the `testing` feature only, so the tests can count
/// them and prove that something reached nothing. A normal build has
/// neither the notes nor the counts.
#[cfg(feature = "testing")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// A program made ready to run: git, gh, docker, the file manager
    /// or the browser that opens a folder or a page, the stopping of a
    /// program.
    Program,
    /// The clipboard written.
    Clipboard,
    /// A request sent over the network.
    Network,
    /// A folder deleted from disk.
    Disk,
}

#[cfg(feature = "testing")]
static REACHED: [std::sync::atomic::AtomicUsize; 4] =
    [const { std::sync::atomic::AtomicUsize::new(0) }; 4];

/// Notes that the core reaches outside itself in the way `reach`.
#[cfg(feature = "testing")]
pub(crate) fn note(reach: Reach) {
    REACHED[reach as usize].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// How often this process reached outside itself in the way `reach` so
/// far, in every thread. Only grows.
#[cfg(feature = "testing")]
pub fn reached(reach: Reach) -> usize {
    REACHED[reach as usize].load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(feature = "testing")]
thread_local! {
    /// How often the core looked at the disk on this thread (`note_read`).
    static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Notes that the core is about to look at the disk on the caller's
/// thread: a folder read, or a path asked whether it is a file, a folder
/// or there at all. Counted under the `testing` feature only
/// (`disk_reads_here`); a normal build does nothing here.
#[inline]
pub(crate) fn note_read() {
    #[cfg(feature = "testing")]
    READS.with(|reads| reads.set(reads.get() + 1));
}

/// How often the core looked at the disk on this thread so far
/// (`note_read`): what a window's dialogs ask it on the window's thread.
/// Other threads' reads, a worker's among them, are not counted here.
/// Only grows.
#[cfg(feature = "testing")]
pub fn disk_reads_here() -> usize {
    READS.with(std::cell::Cell::get)
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn what_reaches_out_is_counted() {
        // A program made through the core's own commands counts, run or
        // not (this one is not). Other tests run beside this one, so the
        // counts are only seen to grow.
        let before = reached(Reach::Program);
        let _ = crate::docker::cli::command(&[]);
        assert!(reached(Reach::Program) > before);
        let before = reached(Reach::Network);
        note(Reach::Network);
        assert!(reached(Reach::Network) > before);
    }
}
