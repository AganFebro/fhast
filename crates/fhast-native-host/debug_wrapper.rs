use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::fs::OpenOptions;

fn main() {
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/fhast-debug.log")
        .unwrap();

    let mut stdout = io::stdout();
    let mut stdin = io::stdin();

    let mut len_buf = [0u8; 4];
    if stdin.read_exact(&mut len_buf).is_err() {
        return;
    }
    let len = u32::from_ne_bytes(len_buf) as usize;
    writeln!(log, "LEN={len} bytes={len_buf:02x?}").unwrap();

    let mut msg = vec![0u8; len];
    stdin.read_exact(&mut msg).unwrap();
    writeln!(log, "MSG={}", String::from_utf8_lossy(&msg)).unwrap();

    let mut child = Command::new("/home/febrian/fhast-branch-cmd/target/debug/fhast-native-host")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut child_stdin = child.stdin.take().unwrap();
    child_stdin.write_all(&len_buf).unwrap();
    child_stdin.write_all(&msg).unwrap();
    drop(child_stdin);

    let output = child.wait_with_output().unwrap();
    writeln!(log, "EXIT={}", output.status).unwrap();
    writeln!(log, "STDERR={}", String::from_utf8_lossy(&output.stderr)).unwrap();
    writeln!(log, "STDOUT_LEN={}", output.stdout.len()).unwrap();
    log.flush().unwrap();

    stdout.write_all(&output.stdout).unwrap();
}
