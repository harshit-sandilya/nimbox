use crate::app::context::AppContext;
use anyhow::{Result, anyhow};
use std::fs;
use std::path::PathBuf;

fn pid_file_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("nimbox")
        .join("nimbox.pid")
}

pub fn run(_ctx: &AppContext) -> Result<()> {
    let pid_path = pid_file_path();

    let pid_str = fs::read_to_string(&pid_path)
        .map_err(|_| anyhow!("Nimbox not running (no PID file found)"))?;

    let pid: u32 = pid_str
        .trim()
        .parse()
        .map_err(|_| anyhow!("Corrupt PID file"))?;

    let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
    if !alive {
        fs::remove_file(&pid_path).ok();
        return Err(anyhow!("Process {} not found — already dead", pid));
    }

    unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    fs::remove_file(&pid_path)?;
    println!("Nimbox stopped (PID {})", pid);
    Ok(())
}
