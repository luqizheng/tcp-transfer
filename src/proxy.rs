use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

use crate::stats::{Stats, StatsSnapshot};

const BUF_SIZE: usize = 8 * 1024;
const STATS_INTERVAL_SECS: u64 = 10;

/// Run a single forwarder: listen on `listen`, forward every accepted connection to `target`.
pub async fn run_forward(listen: SocketAddr, target: String, idle_timeout: u64) -> Result<()> {
    let listener = TcpListener::bind(&listen)
        .await
        .with_context(|| format!("failed to bind {}", listen))?;

    let stats = Arc::new(Stats::default());
    let target = Arc::new(target);

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

        tokio::spawn(async move {
            stats.total_conns.fetch_add(1, Ordering::Relaxed);
            stats.active_conns.fetch_add(1, Ordering::Relaxed);
            let res = handle_conn(client, peer, &target, idle_timeout, &stats).await;
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

    // direction: client -> server (in)
    let t1 = tokio::spawn(async move {
        let r = pipe(&mut cr, &mut sw, idle_timeout, &s1, Direction::In).await;
        let _ = sw.shutdown().await;
        r
    });

    // direction: server -> client (out)
    let t2 = tokio::spawn(async move {
        let r = pipe(&mut sr, &mut cw, idle_timeout, &s2, Direction::Out).await;
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

async fn pipe<R, W>(
    r: &mut R,
    w: &mut W,
    idle_timeout: u64,
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
    }
}
