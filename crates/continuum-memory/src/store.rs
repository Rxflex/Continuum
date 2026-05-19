//! The SQLite writer actor and its async [`Memory`] handle.

use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use continuum_core::dto::{ArchitecturalDecision, IntentRecord, ScratchpadEntry};
use continuum_core::{ContinuumError, Result};
use rusqlite::{params, Connection};
use tokio::sync::{mpsc, oneshot};

use crate::schema::SCHEMA;

/// Commands sent to the writer actor. Each carries a `oneshot` reply channel.
enum Command {
    StoreDecision {
        topic: String,
        description: String,
        reply: oneshot::Sender<Result<i64>>,
    },
    ReadGuidelines {
        reply: oneshot::Sender<Result<Vec<ArchitecturalDecision>>>,
    },
    CommitIntent {
        agent_id: String,
        intent: String,
        files: Vec<String>,
        reply: oneshot::Sender<Result<i64>>,
    },
    RecentChanges {
        limit: i64,
        reply: oneshot::Sender<Result<Vec<IntentRecord>>>,
    },
    WriteScratchpad {
        agent_id: String,
        message: String,
        reply: oneshot::Sender<Result<i64>>,
    },
    ReadScratchpad {
        limit: i64,
        reply: oneshot::Sender<Result<Vec<ScratchpadEntry>>>,
    },
}

/// Cheap, clonable handle to the memory subsystem.
#[derive(Clone)]
pub struct Memory {
    tx: mpsc::UnboundedSender<Command>,
}

impl Memory {
    /// Open (or create) the database and start the writer actor.
    pub fn open(db_path: &Path) -> Result<Self> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Command>();
        let path = db_path.to_path_buf();
        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<()>>();

        std::thread::spawn(move || {
            let conn = match Connection::open(&path) {
                Ok(c) => c,
                Err(e) => {
                    let _ = init_tx.send(Err(ContinuumError::Storage(e.to_string())));
                    return;
                }
            };
            if let Err(e) = conn.execute_batch(SCHEMA) {
                let _ = init_tx.send(Err(ContinuumError::Storage(e.to_string())));
                return;
            }
            let counter = AtomicI64::new(current_max_seq(&conn));
            let _ = init_tx.send(Ok(()));

            while let Some(cmd) = rx.blocking_recv() {
                dispatch(&conn, &counter, cmd);
            }
        });

        init_rx
            .recv()
            .map_err(|e| ContinuumError::Storage(e.to_string()))??;
        Ok(Self { tx })
    }

    fn send<T>(&self, cmd: Command, rx: oneshot::Receiver<Result<T>>) -> SendFuture<T> {
        let queued = self.tx.send(cmd).is_ok();
        SendFuture { queued, rx }
    }

    pub async fn store_decision(&self, topic: String, description: String) -> Result<i64> {
        let (reply, rx) = oneshot::channel();
        self.send(
            Command::StoreDecision {
                topic,
                description,
                reply,
            },
            rx,
        )
        .await
    }

    pub async fn read_guidelines(&self) -> Result<Vec<ArchitecturalDecision>> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::ReadGuidelines { reply }, rx).await
    }

    pub async fn commit_intent(
        &self,
        agent_id: String,
        intent: String,
        files: Vec<String>,
    ) -> Result<i64> {
        let (reply, rx) = oneshot::channel();
        self.send(
            Command::CommitIntent {
                agent_id,
                intent,
                files,
                reply,
            },
            rx,
        )
        .await
    }

    pub async fn recent_changes(&self, limit: i64) -> Result<Vec<IntentRecord>> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::RecentChanges { limit, reply }, rx).await
    }

    pub async fn write_scratchpad(&self, agent_id: String, message: String) -> Result<i64> {
        let (reply, rx) = oneshot::channel();
        self.send(
            Command::WriteScratchpad {
                agent_id,
                message,
                reply,
            },
            rx,
        )
        .await
    }

    pub async fn read_scratchpad(&self, limit: i64) -> Result<Vec<ScratchpadEntry>> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::ReadScratchpad { limit, reply }, rx)
            .await
    }
}

/// Awaits a reply, mapping a dead actor to a storage error.
struct SendFuture<T> {
    queued: bool,
    rx: oneshot::Receiver<Result<T>>,
}

impl<T> std::future::Future for SendFuture<T> {
    type Output = Result<T>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();
        if !this.queued {
            return std::task::Poll::Ready(Err(ContinuumError::Storage(
                "memory actor stopped".into(),
            )));
        }
        match std::pin::Pin::new(&mut this.rx).poll(cx) {
            std::task::Poll::Ready(Ok(v)) => std::task::Poll::Ready(v),
            std::task::Poll::Ready(Err(_)) => std::task::Poll::Ready(Err(ContinuumError::Storage(
                "memory actor dropped reply".into(),
            ))),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

// ----- writer-thread side --------------------------------------------------

fn dispatch(conn: &Connection, counter: &AtomicI64, cmd: Command) {
    match cmd {
        Command::StoreDecision {
            topic,
            description,
            reply,
        } => {
            let _ = reply.send(db_store_decision(conn, &topic, &description));
        }
        Command::ReadGuidelines { reply } => {
            let _ = reply.send(db_read_guidelines(conn));
        }
        Command::CommitIntent {
            agent_id,
            intent,
            files,
            reply,
        } => {
            let _ = reply.send(db_commit_intent(conn, counter, &agent_id, &intent, &files));
        }
        Command::RecentChanges { limit, reply } => {
            let _ = reply.send(db_recent_changes(conn, limit));
        }
        Command::WriteScratchpad {
            agent_id,
            message,
            reply,
        } => {
            let _ = reply.send(db_write_scratchpad(conn, counter, &agent_id, &message));
        }
        Command::ReadScratchpad { limit, reply } => {
            let _ = reply.send(db_read_scratchpad(conn, limit));
        }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn next_seq(counter: &AtomicI64) -> i64 {
    counter.fetch_add(1, Ordering::SeqCst) + 1
}

fn se(e: rusqlite::Error) -> ContinuumError {
    ContinuumError::Storage(e.to_string())
}

fn current_max_seq(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COALESCE(MAX(s), 0) FROM \
         (SELECT seq AS s FROM action_history UNION ALL SELECT seq AS s FROM scratchpad)",
        [],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

fn db_store_decision(conn: &Connection, topic: &str, description: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO lore (topic, description, created_at) VALUES (?1, ?2, ?3)",
        params![topic, description, now()],
    )
    .map_err(se)?;
    Ok(conn.last_insert_rowid())
}

fn db_read_guidelines(conn: &Connection) -> Result<Vec<ArchitecturalDecision>> {
    let mut stmt = conn
        .prepare("SELECT id, topic, description, created_at FROM lore ORDER BY id")
        .map_err(se)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ArchitecturalDecision {
                id: r.get(0)?,
                topic: r.get(1)?,
                description: r.get(2)?,
                created_at: r.get(3)?,
            })
        })
        .map_err(se)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(se)?);
    }
    Ok(out)
}

fn db_commit_intent(
    conn: &Connection,
    counter: &AtomicI64,
    agent_id: &str,
    intent: &str,
    files: &[String],
) -> Result<i64> {
    let seq = next_seq(counter);
    let files_json = serde_json::to_string(files).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO action_history (agent_id, intent, files_touched, seq, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![agent_id, intent, files_json, seq, now()],
    )
    .map_err(se)?;
    Ok(conn.last_insert_rowid())
}

fn db_recent_changes(conn: &Connection, limit: i64) -> Result<Vec<IntentRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, agent_id, intent, files_touched, seq, created_at \
             FROM action_history ORDER BY seq DESC LIMIT ?1",
        )
        .map_err(se)?;
    let rows = stmt
        .query_map([limit], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })
        .map_err(se)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, agent_id, intent, files_json, seq, created_at) = row.map_err(se)?;
        out.push(IntentRecord {
            id,
            agent_id,
            intent,
            files_touched: serde_json::from_str(&files_json).unwrap_or_default(),
            seq,
            created_at,
        });
    }
    Ok(out)
}

fn db_write_scratchpad(
    conn: &Connection,
    counter: &AtomicI64,
    agent_id: &str,
    message: &str,
) -> Result<i64> {
    let seq = next_seq(counter);
    conn.execute(
        "INSERT INTO scratchpad (agent_id, message, seq, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![agent_id, message, seq, now()],
    )
    .map_err(se)?;
    Ok(conn.last_insert_rowid())
}

fn db_read_scratchpad(conn: &Connection, limit: i64) -> Result<Vec<ScratchpadEntry>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, agent_id, message, seq, created_at \
             FROM scratchpad ORDER BY seq DESC LIMIT ?1",
        )
        .map_err(se)?;
    let rows = stmt
        .query_map([limit], |r| {
            Ok(ScratchpadEntry {
                id: r.get(0)?,
                agent_id: r.get(1)?,
                message: r.get(2)?,
                seq: r.get(3)?,
                created_at: r.get(4)?,
            })
        })
        .map_err(se)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(se)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_db() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("continuum-memtest-{nanos}-{n}.db"));
        path
    }

    #[tokio::test]
    async fn decisions_round_trip() {
        let mem = Memory::open(&temp_db()).unwrap();
        let id = mem
            .store_decision("transport".into(), "tcp loopback".into())
            .await
            .unwrap();
        assert!(id >= 1);
        let all = mem.read_guidelines().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].topic, "transport");
        assert_eq!(all[0].description, "tcp loopback");
    }

    #[tokio::test]
    async fn intents_round_trip_with_files() {
        let mem = Memory::open(&temp_db()).unwrap();
        mem.commit_intent(
            "agentA".into(),
            "did the thing".into(),
            vec!["a.rs".into(), "b.rs".into()],
        )
        .await
        .unwrap();
        let recent = mem.recent_changes(10).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].agent_id, "agentA");
        assert_eq!(
            recent[0].files_touched,
            vec!["a.rs".to_string(), "b.rs".to_string()]
        );
    }

    #[tokio::test]
    async fn scratchpad_appends_newest_first_with_monotonic_seq() {
        let mem = Memory::open(&temp_db()).unwrap();
        mem.write_scratchpad("a1".into(), "first".into())
            .await
            .unwrap();
        mem.write_scratchpad("a2".into(), "second".into())
            .await
            .unwrap();
        let entries = mem.read_scratchpad(10).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message, "second");
        assert_eq!(entries[1].message, "first");
        assert!(entries[0].seq > entries[1].seq);
    }

    #[tokio::test]
    async fn sequence_continues_across_reopen() {
        let path = temp_db();
        let first_seq = {
            let mem = Memory::open(&path).unwrap();
            mem.write_scratchpad("a".into(), "one".into())
                .await
                .unwrap();
            mem.read_scratchpad(1).await.unwrap()[0].seq
        };
        let mem = Memory::open(&path).unwrap();
        mem.write_scratchpad("a".into(), "two".into())
            .await
            .unwrap();
        let seq = mem.read_scratchpad(1).await.unwrap()[0].seq;
        assert!(seq > first_seq, "seq {seq} must exceed {first_seq}");
    }
}
