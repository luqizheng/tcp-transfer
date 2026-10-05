mod cli;
mod proxy;
mod stats;

use clap::Parser;
use cli::Cli;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(&cli.log_level, cli.json_log);

    let listen = cli.listen;
    let target = cli.target_addr();

    match proxy::run_forward(listen, target, cli.timeout, cli.hex_dump, cli.dump_file).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "forward exited with error");
            ExitCode::FAILURE
        }
    }
}

fn init_logging(level: &str, json: bool) {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    if json {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer())
            .init();
    }
}
