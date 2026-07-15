//! `herald doctor` — one command that answers "why did/didn't I get a
//! notification?": config state, detected context, presenter authorization,
//! backend availability, log writability.

use crate::config;
use crate::context;
use crate::platform;
use crate::sinks::system::{SystemSink, which};
use anyhow::{Context as _, Result, bail};
use std::path::Path;
use std::process::Command;

/// Build Herald.app from the embedded sources into ~/Applications.
/// Deliberately installs alongside any existing presenter: on macOS 26+
/// Notification Center registration of new bundles can fail, so a working
/// presenter must never be replaced — only superseded after verification.
pub fn install_app() -> Result<()> {
    const MAIN_SWIFT: &str = include_str!("../../app/main.swift");
    const INFO_PLIST: &str = include_str!("../../app/Info.plist");

    if which("swiftc").is_none() {
        bail!("swiftc not found — install the Xcode Command Line Tools first (xcode-select --install)");
    }
    let app = config::home_dir().join("Applications").join("Herald.app");
    if app.exists() {
        bail!("{} already exists; remove it first if you want a rebuild", app.display());
    }

    let build_dir = tempfile_dir()?;
    let src = build_dir.join("main.swift");
    std::fs::write(&src, MAIN_SWIFT)?;

    let macos_dir = app.join("Contents").join("MacOS");
    std::fs::create_dir_all(&macos_dir)?;
    let binary = macos_dir.join("herald-notify");
    run_step("swiftc", &["-O", &src.to_string_lossy(), "-o", &binary.to_string_lossy()])?;
    std::fs::write(app.join("Contents").join("Info.plist"), INFO_PLIST)?;
    run_step("codesign", &["--force", "-s", "-", &app.to_string_lossy()])?;
    let _ = std::fs::remove_dir_all(&build_dir);

    println!("installed: {}", app.display());
    println!("verify before switching app_path:");
    println!("  1. '{}' 'Herald' 'authorization probe'   (must trigger the permission dialog or show a banner)", binary.display());
    println!("  2. '{}' status                            (must print: authorized)", binary.display());
    println!("  3. only then set [sinks.macos_native] app_path = \"{}\"", app.display());
    Ok(())
}

fn tempfile_dir() -> Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join(format!("herald-build-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn run_step(program: &str, args: &[&str]) -> Result<()> {
    let out = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("running {program}"))?;
    if !out.status.success() {
        bail!("{program} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(())
}

pub fn run(explicit_config: Option<&Path>) -> i32 {
    let mut healthy = true;

    // Config
    let loaded = config::load(explicit_config);
    let cfg = match &loaded {
        Ok((cfg, path)) => {
            let state = if path.exists() { "ok" } else { "missing (defaults apply)" };
            println!("config     {} — {state}", path.display());
            cfg.clone()
        }
        Err(err) => {
            println!("config     PARSE ERROR — {err:#}");
            healthy = false;
            config::Config::default()
        }
    };

    // Context
    let ctx = context::current();
    let frontmost = platform::terminal_is_frontmost(&ctx);
    println!(
        "context    harness={} terminal={} headless={} frontmost={}",
        ctx.harness.name(),
        ctx.terminal_bundle_id.as_deref().unwrap_or("-"),
        ctx.headless,
        frontmost.map_or("unknown".to_string(), |b| b.to_string()),
    );

    // Native presenter (the only API macOS 26 still honors)
    let system = SystemSink::from_config(&cfg);
    let presenter_ok = match system.presenter_binary() {
        Some(bin) => {
            let status = Command::new(&bin)
                .arg("status")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "status probe failed".to_string());
            println!("presenter  {} — {status}", bin.display());
            status.contains("authorized")
        }
        None => {
            println!("presenter  NOT FOUND (no Herald.app or ClaudeNotify.app, and no app_path configured)");
            false
        }
    };

    // Fallback backends
    let tn = which("terminal-notifier")
        .or_else(|| {
            let brew = Path::new("/opt/homebrew/bin/terminal-notifier");
            brew.exists().then(|| brew.to_path_buf())
        });
    println!(
        "fallbacks  terminal-notifier={} osascript={}",
        tn.map_or("not found".into(), |p| p.display().to_string()),
        which("osascript").map_or("not found".into(), |p| p.display().to_string()),
    );
    if !presenter_ok {
        println!("           WARNING: on macOS 26+ the fallbacks fail silently; a working presenter app is required");
        healthy = false;
    }

    // Harness CLIs
    println!(
        "harnesses  herdr={} cmux={} orca-env={}",
        which("herdr").map_or("not found".into(), |p| p.display().to_string()),
        which("cmux").map_or("not found".into(), |p| p.display().to_string()),
        std::env::var("ORCA_AGENT_HOOK_PORT").is_ok(),
    );

    // Exec sinks
    for exec_cfg in &cfg.sinks.exec {
        let sink = crate::sinks::exec::ExecSink::new(exec_cfg.clone());
        println!(
            "exec sink  {} — {}",
            exec_cfg.name,
            if sink.active() { "active" } else { "inactive (env not set)" }
        );
    }

    // Logbook
    let log = config::log_path();
    let log_ok = log
        .parent()
        .map(|dir| std::fs::create_dir_all(dir).is_ok())
        .unwrap_or(false)
        && std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .is_ok();
    println!("log        {} — {}", log.display(), if log_ok { "writable" } else { "NOT WRITABLE" });
    healthy &= log_ok;

    println!();
    if healthy {
        println!("doctor: ok");
        0
    } else {
        println!("doctor: problems found");
        1
    }
}
