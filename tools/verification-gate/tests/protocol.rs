//! Exercise the handoff using a tiny independent Git/Cargo repository, never real phase approvals.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Drop for Fixture {
    /// Remove only the unique fixture directory created by this test.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Run fixture setup commands and fail with their captured diagnostics.
fn command(at: &Path, program: &str, args: &[&str]) -> Output {
    let out = Command::new(program)
        .current_dir(at)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{program} {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// Invoke the actual gate executable and verify its public exit-code contract.
fn gate(at: &Path, args: &[&str], expected: i32) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_verification-gate"))
        .current_dir(at)
        .args(args)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{args:?}\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// Check pending, missing-review evidence, real command checks, approval, stale SHA, rejection and dirty-tree refusal.
#[test]
fn protocol_lifecycle() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("gate-integration-{}-{nonce}", std::process::id());
    let fixture = Fixture(std::env::temp_dir().join(name));
    let dev = fixture.0.join("dev");
    let test = fixture.0.join("test");
    fs::create_dir_all(dev.join("src")).unwrap();
    fs::write(
        dev.join("Cargo.toml"),
        "[package]\nname = \"self_driving_sim\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        dev.join("src/main.rs"),
        "/// Exit successfully for the protocol smoke fixture.\nfn main() {}\n",
    )
    .unwrap();
    fs::write(dev.join(".gitignore"), "/target/\n").unwrap();
    let cargo = if cfg!(windows) { "cargo.exe" } else { "cargo" };
    command(&dev, cargo, &["generate-lockfile", "--offline"]);
    command(&dev, "git", &["init"]);
    command(&dev, "git", &["config", "user.name", "Protocol Test"]);
    command(
        &dev,
        "git",
        &["config", "user.email", "protocol@example.invalid"],
    );
    command(&dev, "git", &["add", "."]);
    command(&dev, "git", &["commit", "-m", "fixture"]);
    command(
        &dev,
        "git",
        &["worktree", "add", "--detach", test.to_str().unwrap()],
    );
    let out = gate(&dev, &["request", "protocol-test"], 0);
    let id = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    gate(&dev, &["wait", &id, "0"], 3);
    gate(&dev, &["approve", &id, "must fail before checks"], 1);
    gate(&dev, &["verify", &id, test.to_str().unwrap()], 0);
    gate(&dev, &["status", &id], 3);
    gate(
        &dev,
        &["approve", &id, "fixture commands and lifecycle reviewed"],
        0,
    );
    gate(&dev, &["status", &id], 0);
    gate(&dev, &["reject", &id, "must not overwrite approval"], 1);
    fs::write(dev.join("revision.txt"), "new SHA").unwrap();
    command(&dev, "git", &["add", "."]);
    command(&dev, "git", &["commit", "-m", "revision"]);
    gate(&dev, &["status", &id], 1);
    let out = gate(&dev, &["request", "protocol-test"], 0);
    let next = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    gate(&dev, &["reject", &next, "fixture rejection"], 0);
    gate(&dev, &["wait", &next, "0"], 2);
    gate(&dev, &["list"], 0);
    fs::write(dev.join("dirty.txt"), "do not touch").unwrap();
    gate(&dev, &["request", "protocol-test"], 1);
    assert!(dev.join("dirty.txt").exists());
}
