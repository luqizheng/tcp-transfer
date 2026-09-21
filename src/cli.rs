use clap::Parser;
use std::net::SocketAddr;

/// Lightweight TCP port forwarder for testing.
#[derive(Parser, Debug)]
#[command(name = "tcp-transfer", version, about, long_about = None)]
pub struct Cli {
    /// Local address to listen on, as host:port, e.g. 0.0.0.0:444
    #[arg(short = 'l', long = "listen")]
    pub listen: SocketAddr,

    /// Remote target, as host:port, e.g. 192.2.3.3:333 or example.com:80
    #[arg(short = 't', long = "target")]
    pub target: String,

    /// Idle read timeout in seconds (0 = disabled).
    #[arg(short = 'T', long = "timeout", default_value = "0")]
    pub timeout: u64,

    /// Log level: error, warn, info, debug, trace
    #[arg(short = 'v', long = "log-level", default_value = "info")]
    pub log_level: String,

    /// Emit structured JSON logs (useful for log shippers)
    #[arg(long = "json-log")]
    pub json_log: bool,
}

impl Cli {
    /// Remote target as a `host:port` string, passed straight to `TcpStream::connect`.
    ///
    /// Kept as a `String` rather than `SocketAddr` so DNS names (`example.com:80`)
    /// work — `SocketAddr` would require the host to already be an IP literal.
    pub fn target_addr(&self) -> String {
        self.target.clone()
    }
}
