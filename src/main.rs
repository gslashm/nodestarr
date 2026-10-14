//! Berth command line entry point.
//!
//! `berth` (or `berth serve`) runs the web server. The other subcommands are
//! small maintenance tools meant to be run with `docker exec` when you are
//! locked out or need to recover, plus the internal self-update helper.

use std::process::ExitCode;

use berth::config::{Cli, Command};
use clap::Parser;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let boot = cli.boot.clone();

    // Keep the runtime small: Berth spends almost all of its time idle, so a
    // couple of worker threads is plenty. Blocking work (SQLite, archives)
    // goes to tokio's blocking pool, which only spawns threads on demand.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(boot.workers.max(1))
        .thread_name("berth-worker")
        .max_blocking_threads(16)
        .thread_keep_alive(std::time::Duration::from_secs(10))
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("failed to start async runtime: {err}");
            return ExitCode::FAILURE;
        }
    };

    let result = runtime.block_on(async move {
        match cli.command.unwrap_or(Command::Serve) {
            Command::Serve => berth::app::serve(boot).await,
            Command::ResetPassword { username, password } => {
                berth::cli::reset_password(&boot, &username, password.as_deref()).await
            }
            Command::Disable2fa { username } => berth::cli::disable_2fa(&boot, &username).await,
            Command::Healthcheck => berth::cli::healthcheck(&boot).await,
            Command::SelfUpdateHelper { container, image } => {
                berth::selfupdate::run_helper(&boot, &container, &image).await
            }
            Command::Version => {
                println!("berth {}", berth::VERSION);
                Ok(())
            }
        }
    });

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
