use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::timeout;

use crate::stats::{Stats, StatsSnapshot};

const BUF_SIZE: usize = 8 * 1024;
const STATS_INTERVAL_SECS: u64 = 10;

/// Shared, append-only sink that records every forwarded chunk as a hex line.
///
/// One file for all connections and both directions; each line is self-describing
/// (`ts`, `peer`, `dir`, `len`, `data`) so concurrent connections can be told apart.
#[derive(Clone)]
pub struct DumpSink {
    file: Arc<Mutex<File>>,
}

impl DumpSink {
    /// Open (create if missing, always append) the dump file.
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("open dump file {}", path.display()))?;
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
        })
    }

    /// Append one chunk. Best-effort: a dump failure must not break forwarding.
    async fn record(&self, peer: SocketAddr, dir: Direction, len: usize, hex: &str) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut f = self.file.lock().await;
        let _ = writeln!(
            f,
            "ts={ts} peer={peer} dir={} len={len} data={hex}",
            dir.as_str()
        );
        let _ = f.flush();
    }
}

/// Per-connection traffic observation settings: console hex log and/or dump file.
#[derive(Clone, Default)]
struct DumpOpts {
    /// Emit hex lines through `tracing` (`--hex-dump`).
    console: bool,
    /// Append hex lines to a shared file (`--dump-file`).
    file: Option<DumpSink>,
}

/// Run a single forwarder: listen on `listen`, forward every accepted connection to `target`.
pub async fn run_forward(
    listen: SocketAddr,
    target: String,
    idle_timeout: u64,
    hex_dump: bool,
    dump_path: Option<PathBuf>,
) -> Result<()> {
    let listener = TcpListener::bind(&listen)
        .await
        .with_context(|| format!("failed to bind {}", listen))?;

    let stats = Arc::new(Stats::default());
    let target = Arc::new(target);
    let dump_opts = DumpOpts {
        console: hex_dump,
        file: match dump_path {
            Some(p) => {
                tracing::info!(file = ?p, "hex dump file enabled");
                Some(DumpSink::open(&p)?)
            }
            None => None,
        },
    };

    tracing::info!(%listen, target = %target, idle_timeout, "listening");

    // Background periodic stats reporter.
    let stats_for_log = stats.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(STATS_INTERVAL_SECS));
        interval.tick().await; // skip immediate tick
        loop {
            interval.tick().await;
            let s = stats_for_log.snapshot();
            tracing::info!(
                total = s.total_conns,
                active = s.active_conns,
                bytes_in = StatsSnapshot::human_bytes(s.bytes_in),
                bytes_out = StatsSnapshot::human_bytes(s.bytes_out),
                "stats"
            );
        }
    });

    loop {
        let (client, peer) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "accept failed");
                continue;
            }
        };

        let stats = stats.clone();
        let target = target.clone();
        let dump_opts = dump_opts.clone();

        tokio::spawn(async move {
            stats.total_conns.fetch_add(1, Ordering::Relaxed);
            stats.active_conns.fetch_add(1, Ordering::Relaxed);
            let res =
                handle_conn(client, peer, &target, idle_timeout, dump_opts, &stats).await;
            stats.active_conns.fetch_sub(1, Ordering::Relaxed);
            match &res {
                Ok(()) => tracing::info!(%peer, "connection closed"),
                Err(e) => tracing::warn!(%peer, error = %e, "connection ended"),
            }
        });
    }
}

async fn handle_conn(
    client: TcpStream,
    peer: SocketAddr,
    target: &str,
    idle_timeout: u64,
    dump_opts: DumpOpts,
    stats: &Arc<Stats>,
) -> Result<()> {
    let server = TcpStream::connect(target)
        .await
        .with_context(|| format!("connect {}", target))?;
    let _ = client.set_nodelay(true);
    let _ = server.set_nodelay(true);

    tracing::info!(%peer, target = %target, "connection established");

    let (mut cr, mut cw) = client.into_split();
    let (mut sr, mut sw) = server.into_split();

    let s1 = stats.clone();
    let s2 = stats.clone();
    let d1 = dump_opts.clone();
    let d2 = dump_opts;

    // direction: client -> server (in)
    let t1 = tokio::spawn(async move {
        let r = pipe(&mut cr, &mut sw, idle_timeout, d1, peer, &s1, Direction::In).await;
        let _ = sw.shutdown().await;
        r
    });

    // direction: server -> client (out)
    let t2 = tokio::spawn(async move {
        let r = pipe(&mut sr, &mut cw, idle_timeout, d2, peer, &s2, Direction::Out).await;
        let _ = cw.shutdown().await;
        r
    });

    let r1 = t1.await.context("join client->server")?;
    let r2 = t2.await.context("join server->client")?;
    r1.or(r2)?;
    Ok(())
}

#[derive(Clone, Copy)]
enum Direction {
    In,
    Out,
}

impl Direction {
    fn as_str(self) -> &'static str {
        match self {
            Direction::In => "in",
            Direction::Out => "out",
        }
    }
}

async fn pipe<R, W>(
    r: &mut R,
    w: &mut W,
    idle_timeout: u64,
    dump: DumpOpts,
    peer: SocketAddr,
    stats: &Arc<Stats>,
    dir: Direction,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; BUF_SIZE];
    let dur = (idle_timeout > 0).then(|| Duration::from_secs(idle_timeout));
    loop {
        let read_fut = r.read(&mut buf);
        let n = match dur {
            Some(d) => timeout(d, read_fut).await.map_err(|_| {
                anyhow::anyhow!("idle timeout ({}s)", idle_timeout)
            })??,
            None => read_fut.await?,
        };
        if n == 0 {
            return Ok(());
        }
        let counter = match dir {
            Direction::In => &stats.bytes_in,
            Direction::Out => &stats.bytes_out,
        };
        counter.fetch_add(n as u64, Ordering::Relaxed);
        w.write_all(&buf[..n]).await?;
        if dump.console || dump.file.is_some() {
            let hex = to_hex(&buf[..n]);
            if dump.console {
                tracing::info!(%peer, dir = dir.as_str(), len = n, data = %hex, "hex");
            }
            if let Some(sink) = &dump.file {
                sink.record(peer, dir, n, &hex).await;
            }
        }
    }
}

/// Format bytes as uppercase, space-separated hex: `[0xFF, 0xAC, 0x0F]` -> "FF AC 0F".
fn to_hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::with_capacity(data.len() * 3);
    for (i, b) in data.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0F) as usize] as char);
    }
    s
}
