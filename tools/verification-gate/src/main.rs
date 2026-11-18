//! File-based handoff gate. Signals authorize continued development, never a merge.
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Serialize, Deserialize)]
struct Request {
    id: String,
    stage: String,
    commit: String,
    smoke: bool,
}
#[derive(Debug, Serialize, Deserialize)]
struct Signal {
    request_id: String,
    commit: String,
    state: String,
    notes: String,
}

/// Release the cooperative protocol lock on normal return or unwinding.
struct Lock(PathBuf);
impl Drop for Lock {
    /// Remove only this process's lock directory.
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

/// Run Git without shell interpolation and require success.
fn git(at: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").current_dir(at).args(args).output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// Locate shared state outside all tracked worktrees.
fn state_dir(at: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(git(
        at,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?)
    .join("verification-signals"))
}

/// Fail rather than stash, reset, or overwrite tracked/untracked work.
fn clean(at: &Path) -> Result<()> {
    if !git(at, &["status", "--porcelain"])?.is_empty() {
        return Err("worktree must be clean (including untracked files)".into());
    }
    Ok(())
}

/// Read a typed TOML state file.
fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    Ok(toml::from_str(&fs::read_to_string(path)?)?)
}

/// Publish a new immutable file; never replace an existing request or decision.
fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(toml::to_string_pretty(value)?.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

/// Acquire a repository-wide cooperative lock for each operation.
fn lock(root: &Path) -> Result<Lock> {
    fs::create_dir_all(root)?;
    let path = root.join("lock");
    fs::create_dir(&path)
        .map_err(|e| format!("protocol busy or stale lock at {}: {e}", path.display()))?;
    Ok(Lock(path))
}

/// Restrict IDs to safe single path components.
fn valid_id(id: &str) -> Result<()> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err("id/stage must contain only ASCII letters, digits and hyphens".into());
    }
    Ok(())
}

/// Reject stale or mismatched signals before allowing development to continue.
fn matches(request: &Request, signal: &Signal) -> Result<()> {
    if request.id != signal.request_id || request.commit != signal.commit {
        return Err("signal does not match request ID and commit".into());
    }
    Ok(())
}

/// Load an immutable request under the shared repository directory.
fn request(root: &Path, id: &str) -> Result<Request> {
    valid_id(id)?;
    let value: Request = read(&root.join(id).join("request.toml"))?;
    if value.id != id {
        return Err("request ID mismatch".into());
    }
    Ok(value)
}

/// Record a terminal decision, preserving the checked commit identity.
fn signal(dir: &Path, req: &Request, state: &str, notes: String) -> Result<()> {
    write(
        &dir.join("decision.toml"),
        &Signal {
            request_id: req.id.clone(),
            commit: req.commit.clone(),
            state: state.into(),
            notes,
        },
    )
}

/// 분리된 검증 워크트리에서 Rust 검사와 Godot import/실행 검사를 수행하고 전체 출력을 보존한다.
fn verify(root: &Path, req: &Request, test: &Path) -> Result<()> {
    clean(test)?;
    if state_dir(test)? != root {
        return Err("test worktree belongs to a different repository".into());
    }
    if !git(test, &["branch", "--show-current"])?.is_empty() {
        return Err("test worktree must already have detached HEAD".into());
    }
    let dir = root.join(&req.id);
    if dir.join("checks.toml").exists() || dir.join("decision.toml").exists() {
        return Err("already verified/decided; submit a fresh request".into());
    }
    write(&dir.join("started.toml"), req)?;
    git(test, &["switch", "--detach", &req.commit])?;
    let mut commands: Vec<(&str, Vec<&str>)> = vec![
        ("fmt", vec!["fmt", "--all", "--", "--check"]),
        (
            "clippy",
            vec![
                "clippy",
                "--locked",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("test", vec!["test", "--locked", "--all-features"]),
    ];
    if req.smoke {
        commands.push((
            "extension-build",
            vec!["build", "--locked", "-p", "sim-godot"],
        ));
    }
    let cargo = if cfg!(windows) { "cargo.exe" } else { "cargo" };
    for (name, args) in commands {
        println!("running {name} for {}", req.commit);
        let output = Command::new(cargo).current_dir(test).args(args).output()?;
        let mut log = output.stdout;
        log.extend_from_slice(&output.stderr);
        fs::write(dir.join(format!("{name}.log")), log)?;
        if !output.status.success() {
            return signal(
                &dir,
                req,
                "changes-required",
                format!("{name} failed: {}; see {name}.log", output.status),
            );
        }
    }
    if req.smoke {
        let engine = env::var_os("GODOT_BIN").unwrap_or_else(|| "godot".into());
        for (name, args) in [
            (
                "godot-import",
                vec!["--headless", "--editor", "--path", "godot", "--import"],
            ),
            (
                "smoke",
                vec![
                    "--headless",
                    "--fixed-fps",
                    "64",
                    "--path",
                    "godot",
                    "--script",
                    "res://tests/smoke_test.gd",
                ],
            ),
        ] {
            println!("running {name} for {}", req.commit);
            let output = Command::new(&engine)
                .current_dir(test)
                .args(args)
                .output()?;
            let mut log = output.stdout;
            log.extend_from_slice(&output.stderr);
            fs::write(dir.join(format!("{name}.log")), log)?;
            if !output.status.success() {
                return signal(
                    &dir,
                    req,
                    "changes-required",
                    format!("{name} failed: {}; see {name}.log", output.status),
                );
            }
        }
    }
    clean(test)?;
    if git(test, &["rev-parse", "HEAD"])? != req.commit {
        return Err("test HEAD changed during verification".into());
    }
    write(
        &dir.join("checks.toml"),
        &Signal {
            request_id: req.id.clone(),
            commit: req.commit.clone(),
            state: "awaiting-review".into(),
            notes: "commands passed; code/requirements/manual review still required".into(),
        },
    )
}

/// Read a decision and return a machine-readable continuation status.
fn status(root: &Path, req: &Request, at: &Path) -> Result<u8> {
    clean(at)?;
    if git(at, &["rev-parse", "HEAD"])? != req.commit {
        return Err("HEAD differs from requested commit; submit a fresh request".into());
    }
    let path = root.join(&req.id).join("decision.toml");
    if !path.exists() {
        println!("pending {} {}", req.id, req.commit);
        return Ok(3);
    }
    let value: Signal = read(&path)?;
    matches(req, &value)?;
    println!("{} {} {}\n{}", value.state, req.id, req.commit, value.notes);
    match value.state.as_str() {
        "continue" => Ok(0),
        "changes-required" => Ok(2),
        _ => Err("unknown decision state".into()),
    }
}

/// Dispatch explicit protocol operations; no automatic merge or unattended approval.
fn run() -> Result<u8> {
    let args: Vec<String> = env::args().skip(1).collect();
    let at = env::current_dir()?;
    if args.is_empty() {
        return Err("usage: verification-gate list | request <stage> [--no-smoke] | verify <id> <detached-test-path> | approve|reject <id> <review-notes> | status <id> | wait <id> <timeout-seconds>".into());
    }
    let root = state_dir(&at)?;
    if args[0] == "wait" && args.len() == 3 {
        let seconds: u64 = args[2].parse()?;
        let start = Instant::now();
        loop {
            let code = match lock(&root) {
                Ok(_guard) => {
                    let req = request(&root, &args[1])?;
                    status(&root, &req, &at)?
                }
                Err(_) if root.join("lock").exists() => 3,
                Err(error) => return Err(error),
            };
            if code != 3 || start.elapsed() >= Duration::from_secs(seconds) {
                return Ok(code);
            }
            thread::sleep(Duration::from_secs(2));
        }
    }
    let _guard = lock(&root)?;
    match args[0].as_str() {
        "list" if args.len() == 1 => {
            let mut entries = fs::read_dir(&root)?.collect::<std::io::Result<Vec<_>>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                if !path.join("request.toml").exists() {
                    continue;
                }
                let req: Request = read(&path.join("request.toml"))?;
                let state = if path.join("decision.toml").exists() {
                    let value: Signal = read(&path.join("decision.toml"))?;
                    matches(&req, &value)?;
                    value.state
                } else if path.join("checks.toml").exists() {
                    "awaiting-review".into()
                } else if path.join("started.toml").exists() {
                    "started-or-interrupted".into()
                } else {
                    "pending".into()
                };
                println!("{} {} {} {}", req.id, req.stage, req.commit, state);
            }
        }
        "request" if args.len() == 2 || (args.len() == 3 && args[2] == "--no-smoke") => {
            valid_id(&args[1])?;
            clean(&at)?;
            let commit = git(&at, &["rev-parse", "HEAD"])?;
            let id = format!(
                "{}-{}",
                args[1],
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            );
            let dir = root.join(&id);
            fs::create_dir(&dir)?;
            write(
                &dir.join("request.toml"),
                &Request {
                    id: id.clone(),
                    stage: args[1].clone(),
                    commit,
                    smoke: args.len() == 2,
                },
            )?;
            println!("{id}");
        }
        "verify" if args.len() == 3 => {
            let req = request(&root, &args[1])?;
            verify(&root, &req, Path::new(&args[2]))?;
            if root.join(&req.id).join("decision.toml").exists() {
                return Ok(2);
            }
        }
        "approve" | "reject" if args.len() == 3 && !args[2].trim().is_empty() => {
            let req = request(&root, &args[1])?;
            let dir = root.join(&req.id);
            if args[0] == "approve" {
                let checks: Signal = read(&dir.join("checks.toml"))?;
                matches(&req, &checks)?;
                if checks.state != "awaiting-review" {
                    return Err("successful checks are required".into());
                }
            }
            signal(
                &dir,
                &req,
                if args[0] == "approve" {
                    "continue"
                } else {
                    "changes-required"
                },
                args[2].clone(),
            )?;
        }
        "status" if args.len() == 2 => {
            let req = request(&root, &args[1])?;
            return status(&root, &req, &at);
        }
        _ => return Err("invalid command or arguments; run without arguments for usage".into()),
    }
    Ok(0)
}

/// Report protocol/tool failures separately from pending and rejected decisions.
fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("verification-gate: {error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Old approvals must not unlock a different request or revised commit.
    #[test]
    fn stale_decisions_are_rejected() {
        let req = Request {
            id: "3-2-a".into(),
            stage: "3-2".into(),
            commit: "abc".into(),
            smoke: true,
        };
        let mut value = Signal {
            request_id: req.id.clone(),
            commit: req.commit.clone(),
            state: "continue".into(),
            notes: "reviewed".into(),
        };
        assert!(matches(&req, &value).is_ok());
        value.commit = "def".into();
        assert!(matches(&req, &value).is_err());
        value.commit = req.commit.clone();
        value.request_id = "3-2-old".into();
        assert!(matches(&req, &value).is_err());
    }
    /// Request IDs must not escape the shared state directory.
    #[test]
    fn traversal_ids_are_rejected() {
        for id in ["", "..", "../other", "C:\\other", "a/b"] {
            assert!(valid_id(id).is_err());
        }
        assert!(valid_id("3-2-123").is_ok());
    }
    /// Keep published decisions immutable and serialize concurrent protocol writers.
    #[test]
    fn publication_and_locking_are_exclusive() {
        let root = env::temp_dir().join(format!(
            "verification-gate-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let guard = lock(&root).unwrap();
        assert!(lock(&root).is_err());
        drop(guard);
        let guard = lock(&root).unwrap();
        let req = Request {
            id: "3-2-new".into(),
            stage: "3-2".into(),
            commit: "abc".into(),
            smoke: true,
        };
        write(&root.join("request.toml"), &req).unwrap();
        let loaded: Request = read(&root.join("request.toml")).unwrap();
        assert_eq!(loaded.commit, req.commit);
        assert!(write(&root.join("request.toml"), &req).is_err());
        signal(&root, &req, "changes-required", "test finding".into()).unwrap();
        assert!(signal(&root, &req, "continue", "cannot replace rejection".into()).is_err());
        fs::remove_file(root.join("request.toml")).unwrap();
        fs::remove_file(root.join("decision.toml")).unwrap();
        drop(guard);
        fs::remove_dir(root).unwrap();
    }
}
