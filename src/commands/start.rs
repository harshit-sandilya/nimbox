use crate::app::context::AppContext;
use crate::server::routes;
use anyhow::{Result, anyhow};
use axum::serve;
use std::fs;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use tokio::net::TcpListener;

const DAEMON_ENV: &str = "NIMBOX_DAEMON";

fn pid_file_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("nimbox")
        .join("nimbox.pid")
}

fn log_file_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("nimbox")
        .join("nimbox.log")
}

pub async fn run(ctx: &AppContext, port: u16) -> Result<()> {
    if std::env::var(DAEMON_ENV).is_ok() {
        run_server(ctx, port).await
    } else {
        daemonize(port)
    }
}

fn daemonize(port: u16) -> Result<()> {
    let pid_path = pid_file_path();
    let log_path = log_file_path();

    if let Some(parent) = pid_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if let Ok(pid_str) = fs::read_to_string(&pid_path) {
        let pid: u32 = pid_str.trim().parse().unwrap_or(0);
        if pid > 0 && process_alive(pid) {
            return Err(anyhow!(
                "Nimbox already running (PID {}). Stop with: nimbox stop",
                pid
            ));
        }
    }

    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let self_exe = std::env::current_exe()?;
    let args: Vec<String> = std::env::args().skip(1).collect();

    unsafe {
        Command::new(&self_exe)
            .args(&args)
            .envs(std::env::vars())
            .env(DAEMON_ENV, "1") // signal child to run server
            .stdin(std::process::Stdio::null())
            .stdout(log_file.try_clone()?)
            .stderr(log_file)
            .current_dir("/")
            .pre_exec(|| {
                libc::setsid();
                Ok(())
            })
            .spawn()?;
    }

    std::thread::sleep(std::time::Duration::from_millis(300));

    let pid = fs::read_to_string(&pid_path).unwrap_or_else(|_| "unknown".to_string());
    println!("Nimbox started on :{} (PID {})", port, pid.trim());
    println!("Logs: {}", log_path.display());

    Ok(())
}

async fn run_server(ctx: &AppContext, port: u16) -> Result<()> {
    let pid_path = pid_file_path();
    if let Some(parent) = pid_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::File::create(&pid_path)?;
    writeln!(f, "{}", std::process::id())?;

    let router = routes::router(ctx.clone());
    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    serve(listener, router).await?;
    Ok(())
}

fn process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}
