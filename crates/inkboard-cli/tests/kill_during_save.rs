//! Kills `ink` at random moments while it writes a large board. Afterwards the target must be
//! absent, the complete old version, or the complete new version: never a half-written file.

use inkboard_core::serial;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

const INK: &str = env!("CARGO_BIN_EXE_ink");
/// Big enough that writing the output takes tens of milliseconds even on a fast disk (~6 MB in, ~16 MB out).
const BIG: usize = 10_000;
const SMALL: usize = 50;
const TRIALS: usize = 10;
/// Upper bound of the random delay between the first change seen on the target and the kill.
const JITTER_US: u64 = 1500;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("kill-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the temporary folder");
        TempDir(dir)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ink() -> Command {
    let mut c = Command::new(INK);
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    c
}

fn run_ok(mut c: Command) -> Duration {
    let start = Instant::now();
    let status = c.status().expect("run ink");
    assert!(status.success(), "ink failed: {c:?}");
    start.elapsed()
}

/// Deterministic generator (xorshift64): the same kill instants on every run.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n.max(1)
    }
}

fn probe(p: &Path) -> Option<(u64, SystemTime)> {
    std::fs::metadata(p).ok().map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
}

#[derive(Default, Debug)]
struct Outcome {
    killed_running: usize,
    finished_first: usize,
    absent: usize,
    old: usize,
    new: usize,
}

/// `start`: command writing `target`. `old`: bytes of the previous version. `is_new`: is this the complete new version?
/// `full_time`: duration of an uninterrupted run. Even trials are killed at a random instant of the run, odd trials
/// shortly after the first change seen on the target (or its temporary file), which is where a bad save breaks.
fn kill_trials(target: &Path, start: &dyn Fn() -> Command, old: &[u8], is_new: &dyn Fn(&[u8]) -> bool, full_time: Duration) -> Outcome {
    let tmp = target.with_extension("tmp-save");
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut out = Outcome::default();
    for trial in 0..TRIALS {
        // two trials out of three start from an existing old version, the others from nothing
        let has_old = trial % 3 != 0;
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(target);
        if has_old {
            std::fs::write(target, old).unwrap();
        }
        let before = (probe(target), probe(&tmp));
        let mut child: Child = start().spawn().expect("spawn ink");
        let began = Instant::now();
        let mut finished = false;
        if trial % 2 == 0 {
            let delay = Duration::from_micros(rng.below(u64::try_from(full_time.as_micros()).unwrap_or(u64::MAX) / 10 * 12));
            while began.elapsed() < delay {
                if child.try_wait().unwrap().is_some() {
                    finished = true;
                    break;
                }
                std::thread::sleep(Duration::from_micros(100));
            }
        } else {
            loop {
                if child.try_wait().unwrap().is_some() {
                    finished = true;
                    break;
                }
                if (probe(target), probe(&tmp)) != before {
                    // busy-wait: a sleep lasts longer than the write itself
                    let jitter = Duration::from_micros(rng.below(JITTER_US));
                    let seen = Instant::now();
                    while seen.elapsed() < jitter {
                        std::hint::spin_loop();
                    }
                    break;
                }
                assert!(began.elapsed() < Duration::from_secs(30), "trial {trial}: ink neither wrote nor exited in 30 s");
            }
        }
        if finished {
            out.finished_first += 1;
        } else if child.try_wait().unwrap().is_none() {
            child.kill().expect("kill ink");
            out.killed_running += 1;
        } else {
            out.finished_first += 1;
            finished = true;
        }
        let status = child.wait().unwrap();
        if finished {
            assert!(status.success(), "trial {trial}: an uninterrupted run failed");
        }
        match std::fs::read(target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                assert!(!finished, "trial {trial}: ink finished successfully but wrote nothing");
                out.absent += 1;
            }
            Err(e) => panic!("trial {trial}: cannot read the target: {e}"),
            Ok(bytes) if has_old && bytes == old => {
                assert!(!finished, "trial {trial}: ink finished successfully but the old version is still there");
                out.old += 1;
            }
            Ok(bytes) if is_new(&bytes) => out.new += 1,
            Ok(bytes) => panic!(
                "trial {trial} (killed after {:?}, old version present: {has_old}): the target is not missing, not the old board, and not the complete new board \
                 ({} bytes, old {} bytes, valid board: {})",
                began.elapsed(),
                bytes.len(),
                old.len(),
                serial::from_json(&String::from_utf8_lossy(&bytes)).is_ok()
            ),
        }
    }
    eprintln!("kill trials: {out:?}");
    assert!(out.killed_running >= 3, "the writes were too short to be interrupted ({out:?}): enlarge the board");
    out
}

fn bench_args(count: usize, out: &Path) -> Command {
    let mut c = ink();
    c.arg("bench").arg(count.to_string()).arg(out);
    c
}

fn convert_args(input: &Path, out: &Path) -> Command {
    let mut c = ink();
    c.arg("convert").arg(input).arg(out);
    c
}

/// `ink bench` saves through `serial::save` (temporary file then rename), the path used by the application.
#[test]
fn killing_a_save_never_leaves_a_half_written_board() {
    let dir = TempDir::new("save");
    let target = dir.path("board.inkboard");
    run_ok(bench_args(SMALL, &target));
    let old = std::fs::read(&target).unwrap();
    let old_count = serial::load(&target).expect("the old version is valid").pages[0].objects.len();
    let full_time = run_ok(bench_args(BIG, &target));
    let new_count = serial::load(&target).expect("the new version is valid").pages[0].objects.len();
    assert!(new_count > 100 * old_count);
    // ids are random: the new version is recognised by its content, not its bytes
    let is_new = |bytes: &[u8]| match serial::from_json(&String::from_utf8_lossy(bytes)) {
        Ok(doc) => doc.name == "Bench" && doc.pages[0].objects.len() == new_count,
        Err(_) => false,
    };
    let out = kill_trials(&target, &|| bench_args(BIG, &target), &old, &is_new, full_time);
    assert_eq!(out.new + out.old + out.absent, TRIALS);
}

/// `ink convert` to a native file goes through `serial::write_atomic` (temporary file + rename), like `serial::save`: this was
/// BUG-7 (see `context/PLAN_TESTS_AUTO.md`). On Windows a kill never lands in the middle of a whole-file write, so only
/// Linux/macOS can show a regression.
#[test]
fn killing_a_conversion_never_leaves_a_half_written_board() {
    let dir = TempDir::new("convert");
    let big = dir.path("big.inkboard");
    let small = dir.path("small.inkboard");
    run_ok(bench_args(BIG, &big));
    run_ok(bench_args(SMALL, &small));
    let target = dir.path("out.inkboard");
    run_ok(convert_args(&small, &target));
    let old = std::fs::read(&target).unwrap();
    let full_time = run_ok(convert_args(&big, &target));
    let new = std::fs::read(&target).unwrap();
    run_ok(convert_args(&big, &target));
    assert_eq!(std::fs::read(&target).unwrap(), new, "a conversion must be deterministic for this test to compare bytes");
    assert!(serial::from_json(&String::from_utf8_lossy(&new)).is_ok() && serial::from_json(&String::from_utf8_lossy(&old)).is_ok());
    let out = kill_trials(&target, &|| convert_args(&big, &target), &old, &|bytes| bytes == new, full_time);
    assert_eq!(out.new + out.old + out.absent, TRIALS);
}
