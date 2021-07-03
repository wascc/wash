mod common;
use common::wash;
use std::io::Write;
use std::process::Stdio;

// Unfortunately, launching the REPL will corrupt a terminal session without being able to properly
// clean up the interactive mode. Until this can be fixed, we'll run these in certain situations
//TODO: Investigate possibility of "detatching" terminal _or_ starting a new session just for these tests.

#[actix_rt::test]
async fn integration_up_basic() {
    let mut up = wash()
        .args(&["up"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("failed to launch repl");

    let stdin = up.stdin.as_mut().expect("Failed to open stdin");
    actix_rt::time::sleep(std::time::Duration::from_millis(1000)).await;
    stdin
        .write_all("help\n".as_bytes())
        .expect("Failed to write to stdin");
    actix_rt::time::sleep(std::time::Duration::from_millis(1000)).await;
    stdin
        .write_all("q\n".as_bytes())
        .expect("Failed to write to stdin");

    assert!(up.wait().unwrap().success());
}

#[test]
#[ignore]
fn integration_up_all_flags() {
    const LOG_LEVEL: &str = "info";
    const RPC_HOST: &str = "0.0.0.0";
    const RPC_PORT: &str = "4222";

    let up = wash()
        .args(&[
            "up",
            "--log-level",
            LOG_LEVEL,
            "--host",
            RPC_HOST,
            "--port",
            RPC_PORT,
        ])
        .output()
        .expect("failed to launch repl");

    assert!(up.status.success());
}
