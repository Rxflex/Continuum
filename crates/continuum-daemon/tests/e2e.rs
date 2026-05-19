//! End-to-end tests: spawn the real daemon binary and drive it over TCP,
//! exactly as an adapter does.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_workspace() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut path = std::env::temp_dir();
    path.push(format!("continuum-e2e-{nanos}-{n}"));
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// Kills the spawned daemon when the test ends.
struct DaemonGuard(Child);

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Spawn the daemon for `workspace` and wait until it has published a lockfile.
fn start_daemon(workspace: &Path) -> (DaemonGuard, Value) {
    let child = Command::new(env!("CARGO_BIN_EXE_continuum-daemon"))
        .arg("--workspace")
        .arg(workspace)
        .arg("--idle-minutes")
        .arg("0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn daemon");
    // Own the child immediately so its `Drop` reaps it even if an assertion
    // below unwinds before the daemon is ready.
    let guard = DaemonGuard(child);

    let lockfile = workspace.join(".continuum").join("daemon.lock");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(text) = std::fs::read_to_string(&lockfile) {
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                return (guard, value);
            }
        }
        assert!(
            Instant::now() < deadline,
            "daemon never published a lockfile"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// A blocking MCP client over the daemon's TCP transport.
struct Client {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl Client {
    fn connect(lock: &Value) -> Client {
        let endpoint = lock["endpoint"].as_str().unwrap();
        let stream = TcpStream::connect(endpoint).expect("connect to daemon");
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut client = Client { stream, reader };

        client.send(&json!({
            "protocol_version": lock["protocol_version"],
            "token": lock["token"],
        }));
        assert_eq!(client.recv()["ok"], json!(true), "handshake should succeed");
        client
    }

    fn send(&mut self, value: &Value) {
        writeln!(self.stream, "{value}").unwrap();
        self.stream.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).unwrap();
        assert!(n > 0, "daemon closed the connection unexpectedly");
        serde_json::from_str(&line).unwrap()
    }

    fn request(&mut self, value: Value) -> Value {
        self.send(&value);
        self.recv()
    }
}

#[test]
fn initialize_and_list_all_tools() {
    let ws = temp_workspace();
    let (_daemon, lock) = start_daemon(&ws);
    let mut client = Client::connect(&lock);

    let init = client.request(json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
    }));
    assert_eq!(init["result"]["serverInfo"]["name"], "continuum");

    let tools = client.request(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 13, "expected 13 tools, got {names:?}");
    for expected in ["search_code", "find_text", "commit_intent", "get_stats"] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }
}

#[test]
fn memory_tools_round_trip_over_the_wire() {
    let ws = temp_workspace();
    let (_daemon, lock) = start_daemon(&ws);
    let mut client = Client::connect(&lock);
    client.request(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }));

    let stored = client.request(json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "commit_intent", "arguments": {
            "agent_id": "tester",
            "intent": "wired the transport",
            "files_touched": ["transport.rs"]
        }}
    }));
    assert_eq!(stored["result"]["isError"], json!(false));

    let recent = client.request(json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": { "name": "get_recent_changes", "arguments": { "limit": 5 } }
    }));
    let text = recent["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("wired the transport"),
        "intent not persisted: {text}"
    );
    assert!(text.contains("tester"));
}

#[test]
fn handshake_rejects_a_bad_token() {
    let ws = temp_workspace();
    let (_daemon, lock) = start_daemon(&ws);

    let mut stream = TcpStream::connect(lock["endpoint"].as_str().unwrap()).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    writeln!(
        stream,
        "{}",
        json!({
            "protocol_version": lock["protocol_version"],
            "token": "definitely-not-the-token"
        })
    )
    .unwrap();
    stream.flush().unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(reply["ok"], json!(false), "a bad token must be rejected");
}

#[test]
fn unknown_method_yields_a_json_rpc_error() {
    let ws = temp_workspace();
    let (_daemon, lock) = start_daemon(&ws);
    let mut client = Client::connect(&lock);

    let resp = client.request(json!({ "jsonrpc": "2.0", "id": 9, "method": "no/such/method" }));
    assert_eq!(resp["error"]["code"], json!(-32601));
}

#[test]
fn notifications_receive_no_response() {
    let ws = temp_workspace();
    let (_daemon, lock) = start_daemon(&ws);
    let mut client = Client::connect(&lock);

    // A notification (no `id`) must draw no reply; the next request's response
    // is what `recv` should see.
    client.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let ping = client.request(json!({ "jsonrpc": "2.0", "id": 7, "method": "ping" }));
    assert_eq!(
        ping["id"],
        json!(7),
        "notification must not consume a response slot"
    );
}

#[test]
fn find_text_locates_text_across_files() {
    let ws = temp_workspace();
    // A non-code file: find_text must search it even though the indexer ignores it.
    std::fs::write(
        ws.join("notes.md"),
        "intro\nthe special marker lives here\nend\n",
    )
    .unwrap();
    let (_daemon, lock) = start_daemon(&ws);
    let mut client = Client::connect(&lock);

    let resp = client.request(json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "find_text", "arguments": { "pattern": "special marker" } }
    }));
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("notes.md"),
        "find_text should locate the file: {text}"
    );
    assert!(text.contains("special marker"));
}
