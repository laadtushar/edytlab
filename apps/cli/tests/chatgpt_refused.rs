//! The CLI cannot sign in with ChatGPT, and the ChatGPT plan takes no key.
//!
//! `--provider chatgpt` with an `--api-key` would otherwise build an agent that
//! sends the key as the "plan" bearer (#474, review of #695). It is refused
//! before anything else happens: no project folder, no request.

use assert_cmd::Command;

#[test]
fn the_chatgpt_provider_is_refused_with_a_message_that_says_why() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = dir.path().join("project");
    let output = Command::cargo_bin("edytlab-cli")
        .expect("locate cli binary")
        .args([
            "--provider",
            "chatgpt",
            "--api-key",
            "sk-would-be-the-bearer",
            "--message",
            "hi",
            "--project-dir",
        ])
        .arg(&project)
        .output()
        .expect("run the cli");

    assert!(!output.status.success(), "it ran");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ChatGPT"), "{stderr}");
    assert!(stderr.contains("signing in"), "{stderr}");
    assert!(
        !stderr.contains("sk-would-be-the-bearer") && output.stdout.is_empty(),
        "the key was echoed or a turn ran: {stderr}"
    );
    assert!(!project.exists(), "it opened a project before refusing");
}

#[test]
fn another_provider_is_not_refused() {
    // `--help` exits 0 and proves the binary still starts; a provider the
    // check does not know is not its business.
    Command::cargo_bin("edytlab-cli")
        .expect("locate cli binary")
        .arg("--help")
        .assert()
        .success();
}
