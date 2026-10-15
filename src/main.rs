//! Nodestarr command line entry point.
//!
//! `nodestarr` (or `nodestarr serve`) runs the web server. The other subcommands are
//! small maintenance tools meant to be run with `docker exec` when you are
//! locked out or need to recover, plus the internal self-update helper.

use std::process::ExitCode;

use clap::Parser;
use nodestarr::config::{Cli, Command};

fn main() -> ExitCode {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    tune_allocator();

    let cli = Cli::parse();
    let boot = cli.boot.clone();

    // Keep the runtime small: Nodestarr spends almost all of its time idle, so a
    // couple of worker threads is plenty. Blocking work (SQLite, archives)
    // goes to tokio's blocking pool, which only spawns threads on demand.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(boot.workers.max(1))
        .thread_name("nodestarr-worker")
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
            Command::Serve => nodestarr::app::serve(boot).await,
            Command::ResetPassword { username, password } => {
                nodestarr::cli::reset_password(&boot, &username, password.as_deref()).await
            }
            Command::Disable2fa { username } => nodestarr::cli::disable_2fa(&boot, &username).await,
            Command::Healthcheck => nodestarr::cli::healthcheck(&boot).await,
            Command::SelfUpdateHelper { container, image } => {
                nodestarr::selfupdate::run_helper(&boot, &container, &image).await
            }
            Command::Version => {
                println!("nodestarr {}", nodestarr::VERSION);
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

/// Makes glibc hand large freed buffers back to the system.
///
/// After a large buffer is freed, glibc raises its mmap threshold, so the
/// next ones come from per-thread arenas and stay resident once freed. Every
/// sign-in hashes with Argon2id in a 19 MiB buffer, so a few sign-ins used to
/// leave about 20 MiB parked in each thread's arena. A fixed threshold keeps
/// large buffers in their own mappings, which freeing returns, and two arenas
/// are plenty for a mostly idle server. (musl, used in the Docker image,
/// already returns large buffers.)
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn tune_allocator() {
    // SAFETY: mallopt only changes allocator parameters, and this runs before
    // any other thread starts.
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 256 * 1024);
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
}
