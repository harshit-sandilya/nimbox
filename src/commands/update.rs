use anyhow::Result;

pub async fn run() -> Result<()> {
    let repo = "harshit-sandilya/nimbox";

    let current_tag = match option_env!("NIMBOX_BUILD_VERSION") {
        Some(v) if v.starts_with('v') => v.to_string(),
        Some(v) => format!("v{}", v),
        None => format!("v{}", env!("CARGO_PKG_VERSION")),
    };

    println!("Current version: {}", current_tag);
    print!("Checking latest release...");

    let client = reqwest::Client::builder()
        .user_agent("nimbox-updater")
        .build()?;

    let release: serde_json::Value = client
        .get(format!(
            "https://api.github.com/repos/{}/releases/latest",
            repo
        ))
        .send()
        .await?
        .json()
        .await?;

    let latest = release["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Could not read latest version"))?;

    println!(" latest: {}", latest);

    if latest == current_tag {
        println!("Already up to date.");
        return Ok(());
    }

    // Detect platform
    let artifact = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "nimbox-linux-x86_64",
        ("linux", "aarch64") => "nimbox-linux-aarch64",
        ("macos", "x86_64") => "nimbox-macos-x86_64",
        ("macos", "aarch64") => "nimbox-macos-aarch64",
        (os, arch) => anyhow::bail!("Unsupported platform: {}/{}", os, arch),
    };

    let url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        repo, latest, artifact
    );

    println!("Downloading {}...", url);

    let bytes = client.get(&url).send().await?.bytes().await?;

    // Write next to current binary, then replace
    let current_exe = std::env::current_exe()?;
    let tmp = current_exe.with_extension("tmp");

    std::fs::write(&tmp, &bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }

    // Atomic replace — fails if no write permission
    if std::fs::rename(&tmp, &current_exe).is_err() {
        // Try sudo cp as fallback hint
        std::fs::remove_file(&tmp).ok();
        anyhow::bail!(
            "Permission denied replacing {}. Run with sudo or:\n  sudo nimbox update",
            current_exe.display()
        );
    }

    println!(
        "Updated to {} — restart any running instance with: nimbox stop && nimbox start",
        latest
    );
    Ok(())
}
