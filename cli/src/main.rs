//! GODsend PS5 command line.
//!
//! Every command talks to one configured console over FTP, elfldr and the
//! Ghostpad payload.

use godsend_core::config::{Config, Host};
use godsend_core::convert;
use godsend_core::device;
use godsend_core::doctor;
use godsend_core::fm;
use godsend_core::ghostpad::Ghostpad;
use godsend_core::homebrew;
use godsend_core::install;
use godsend_core::library;
use godsend_core::pad;
use godsend_core::payload;
use godsend_core::sources;
use godsend_core::store;
use godsend_core::transfer::JobManager;
use godsend_core::{Error, Result};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" || args[0] == "help" {
        usage();
        return;
    }
    if let Err(err) = run(&args) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn usage() {
    println!(
        r#"GODsend PS5 {version}

Usage: godsend-ps5 <command> [options]

Hosts
  hosts                          list configured consoles
  add-host <addr> [--name N] [--ftp-port N] [--pad-port N] [--payload-port N]
  rm-host <id>                   remove a console
  use <id>                       make a console active
  discover [--subnet 192.168.1]  scan the LAN on 2121 and 1337
  status [<id>]                  probe FTP, etaHEN, Ghostpad and elfldr
  doctor                         preflight checks

Library and files
  scan                           list installed apps, images and packages
  ls <remote-path>               list a console directory
  mkdir <remote-path>            create a remote directory
  rm <remote-path> [--dir]       delete a remote file or directory
  push <local> <remote-dir>      upload a file
  pull <remote> [<local-dir>]    download a file

Install and payloads
  install <local>                PKG, image, folder, ELF or ZIP to the console
  payloads [--refresh]           list the payload catalogue
  payload <id>                   download a payload and send it
  payload-file <path.elf>        send a local ELF to elfldr
  search <query>                 search the game index
  download <slug>                download a game's best link

Controller
  pad <button>                   tap one button
  combo <a+b+c> [ms]             press buttons together
  macro "cross,down+options"     run a sequence of presses
  analog <L|R> <direction>       tilt a stick briefly

Conversions
  inspect <path>                 show format, system and title id
  iso <folder> <out.iso>         build an ISO9660 + Joliet image
  extract <in.iso> <out-dir>     extract an ISO
  zip <folder> <out.zip>         pack a folder
  unzip <in.zip> <out-dir>       unpack a ZIP
  split <file> [--chunk-mb N]    split for FAT32
  merge <base> <out>             join <base>.0, <base>.1, ...
"#,
        version = godsend_core::VERSION
    );
}

fn run(args: &[String]) -> Result<()> {
    let mut config = Config::load();
    let cmd = args[0].as_str();
    match cmd {
        "hosts" => {
            for host in &config.hosts {
                let marker = if config.active_host.as_deref() == Some(&host.id) {
                    "*"
                } else {
                    " "
                };
                println!(
                    "{marker} {:<28} {:<20} ftp :{} pad :{} loader :{}",
                    host.id, host.name, host.ftp_port, host.pad_port, host.payload_port
                );
            }
            if config.hosts.is_empty() {
                println!("no consoles yet: godsend-ps5 add-host 192.168.1.50");
            }
            Ok(())
        }
        "add-host" => {
            let addr = args
                .get(1)
                .ok_or_else(|| Error::Invalid("add-host <addr>".into()))?;
            let name = flag_value(args, "--name").unwrap_or_else(|| addr.clone());
            let mut host = Host::new(&name, addr);
            if let Some(port) = flag_value(args, "--ftp-port").and_then(|v| v.parse().ok()) {
                host.ftp_port = port;
            }
            if let Some(port) = flag_value(args, "--pad-port").and_then(|v| v.parse().ok()) {
                host.pad_port = port;
            }
            if let Some(port) = flag_value(args, "--payload-port").and_then(|v| v.parse().ok()) {
                host.payload_port = port;
            }
            config.upsert_host(host.clone());
            config.save()?;
            println!("added {} ({})", host.name, host.id);
            Ok(())
        }
        "rm-host" => {
            let id = args
                .get(1)
                .ok_or_else(|| Error::Invalid("rm-host <id>".into()))?;
            config.remove_host(id);
            config.save()?;
            println!("removed {id}");
            Ok(())
        }
        "use" => {
            let id = args.get(1).ok_or_else(|| Error::Invalid("use <id>".into()))?;
            if !config.hosts.iter().any(|h| &h.id == id) {
                return Err(Error::NotFound(format!("no host {id}")));
            }
            config.active_host = Some(id.clone());
            config.save()?;
            println!("active console: {id}");
            Ok(())
        }
        "discover" => {
            let subnet = flag_value(args, "--subnet").unwrap_or_else(auto_subnet);
            let found = device::discover(&subnet)?;
            for host in &found {
                println!("{} {}", host.addr, host.ftp_port);
            }
            println!("{} console(s)", found.len());
            Ok(())
        }
        "status" => {
            let host = match args.get(1) {
                Some(id) => config
                    .hosts
                    .iter()
                    .find(|h| &h.id == id || &h.addr == id)
                    .cloned()
                    .ok_or_else(|| Error::NotFound(format!("no host {id}")))?,
                None => config.require_active()?,
            };
            let status = device::probe(&host);
            println!("{} {} ({})", host.name, host.addr, host.id);
            println!("  ftp     {} on port {}", yes_no(status.ftp_ok), status.ftp_port.unwrap_or(host.ftp_port));
            println!("  etaHEN  {}{}", yes_no(status.etahen_ok), version_suffix(&status.etahen_version));
            println!("  homebrew {}", yes_no(status.homebrew_ok));
            println!("  ghostpad {} on {}", yes_no(status.pad_ok), host.pad_port);
            println!("  elfldr  {} on {}", yes_no(status.payload_ok), host.payload_port);
            println!("  pkg mgr {} on {}", yes_no(status.pkg_manager_ok), host.pkg_manager_port);
            if !status.message.is_empty() {
                println!("  note    {}", status.message);
            }
            Ok(())
        }
        "doctor" => {
            for check in doctor::run(&config) {
                println!("[{}] {} - {}", if check.ok { "ok" } else { "--" }, check.name, check.detail);
            }
            Ok(())
        }
        "scan" => {
            let host = config.require_active()?;
            let view = library::scan(&host)?;
            println!("{} title(s), {} package(s)", view.games.len(), view.packages.len());
            for game in &view.games {
                println!(
                    "  {:<28} {:<9} {:<9} {}",
                    game.title_id.clone().unwrap_or_else(|| "-".into()),
                    game.system,
                    game.kind,
                    game.title
                );
            }
            for package in &view.packages {
                println!(
                    "  {:<28} package   {:<9} {}",
                    package.title_id.clone().unwrap_or_else(|| "-".into()),
                    package.kind,
                    package.title
                );
            }
            Ok(())
        }
        "ls" => {
            let host = config.require_active()?;
            let path = args.get(1).ok_or_else(|| Error::Invalid("ls <remote-path>".into()))?;
            for entry in fm::remote_list(&host, path)? {
                println!(
                    "{:<10} {:>12} {}",
                    if entry.is_dir { "dir" } else { "file" },
                    entry.size_bytes,
                    entry.name
                );
            }
            Ok(())
        }
        "mkdir" => {
            let host = config.require_active()?;
            let path = args.get(1).ok_or_else(|| Error::Invalid("mkdir <remote-path>".into()))?;
            fm::mkdir_remote(&host, path)?;
            println!("created {path}");
            Ok(())
        }
        "rm" => {
            let host = config.require_active()?;
            let path = args.get(1).ok_or_else(|| Error::Invalid("rm <remote-path>".into()))?;
            fm::delete_remote(&host, path, has_flag(args, "--dir"))?;
            println!("deleted {path}");
            Ok(())
        }
        "push" => {
            let host = config.require_active()?;
            let local = args.get(1).ok_or_else(|| Error::Invalid("push <local> <remote-dir>".into()))?.clone();
            let remote = args.get(2).ok_or_else(|| Error::Invalid("push <local> <remote-dir>".into()))?.clone();
            let config_for_job = Config::load();
            run_job("upload", move |ctx| {
                fm::upload(&host, &PathBuf::from(&local), &remote, config_for_job.split_bytes, ctx)?;
                Ok(())
            })
        }
        "pull" => {
            let host = config.require_active()?;
            let remote = args.get(1).ok_or_else(|| Error::Invalid("pull <remote> [<local-dir>]".into()))?.clone();
            let local = args.get(2).cloned().unwrap_or_else(|| ".".into());
            run_job("download", move |ctx| {
                fm::download(&host, &remote, &PathBuf::from(&local), ctx)?;
                Ok(())
            })
        }
        "install" => {
            let host = config.require_active()?;
            let config_for_job = Config::load();
            let local = PathBuf::from(
                args.get(1)
                    .ok_or_else(|| Error::Invalid("install <local>".into()))?,
            );
            run_job("install", move |ctx| {
                let report = install::install_local(&host, &local, &config_for_job, ctx)?;
                ctx.message(report.message);
                Ok(())
            })
        }
        "payloads" => {
            let entries = homebrew::catalog(&config, has_flag(args, "--refresh"))?;
            for entry in &entries {
                println!(
                    "  {:<34} {:<24} {:<12} {}",
                    entry.id, entry.name, entry.version, entry.category
                );
            }
            println!("{} entries", entries.len());
            Ok(())
        }
        "payload" => {
            let host = config.require_active()?;
            let config_for_job = Config::load();
            let id = args.get(1).ok_or_else(|| Error::Invalid("payload <id>".into()))?;
            let entry = homebrew::catalog(&config_for_job, false)?
                .into_iter()
                .find(|e| &e.id == id)
                .ok_or_else(|| Error::NotFound(format!("no payload {id}")))?;
            run_job("payload", move |ctx| {
                let report = homebrew::install(&host, &config_for_job, &entry, ctx)?;
                ctx.message(report.message);
                Ok(())
            })
        }
        "payload-file" => {
            let host = config.require_active()?;
            let path = PathBuf::from(
                args.get(1)
                    .ok_or_else(|| Error::Invalid("payload-file <path>".into()))?,
            );
            let report = payload::send_file(&host, &path)?;
            println!(
                "sent {} bytes{}",
                report.bytes,
                if report.reply.is_empty() {
                    String::new()
                } else {
                    format!(": {}", report.reply)
                }
            );
            Ok(())
        }
        "search" => {
            let query = args.get(1).cloned().unwrap_or_default();
            let hits = sources::gamesearch(&config.gamesearch_base, &query, 25)?;
            for hit in &hits {
                println!("{:<40} {:<8} {} links", hit.slug, hit.platform, hit.link_count);
            }
            Ok(())
        }
        "download" => {
            let config_for_job = Config::load();
            let slug = args.get(1).ok_or_else(|| Error::Invalid("download <slug>".into()))?;
            let hit = sources::gamesearch_detail(&config.gamesearch_base, slug)?;
            run_job("download", move |ctx| {
                let path = store::download_hit(&hit, &config_for_job, ctx)?;
                ctx.message(format!("saved to {}", path.display()));
                Ok(())
            })
        }
        "pad" => {
            let host = config.require_active()?;
            let name = args.get(1).ok_or_else(|| Error::Invalid("pad <button>".into()))?;
            let session = Ghostpad::connect(&host)?;
            pad::press(&session, name)?;
            println!("pressed {name}");
            Ok(())
        }
        "combo" => {
            let host = config.require_active()?;
            let combo = args.get(1).ok_or_else(|| Error::Invalid("combo <a+b>".into()))?;
            let parts: Vec<&str> = combo
                .split('+')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            let ms = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(120);
            let session = Ghostpad::connect(&host)?;
            pad::combo(&session, &parts, ms)?;
            println!("pressed {combo}");
            Ok(())
        }
        "macro" => {
            let host = config.require_active()?;
            let script = args
                .get(1)
                .ok_or_else(|| Error::Invalid("macro \"cross,down+options\"".into()))?;
            let session = Ghostpad::connect(&host)?;
            pad::macro_run(&session, script, 120)?;
            println!("macro done");
            Ok(())
        }
        "analog" => {
            let host = config.require_active()?;
            let side = args.get(1).ok_or_else(|| Error::Invalid("analog <L|R> <direction>".into()))?;
            let direction = args.get(2).ok_or_else(|| Error::Invalid("analog <L|R> <direction>".into()))?;
            let session = Ghostpad::connect(&host)?;
            pad::analog(&session, side, direction)?;
            std::thread::sleep(std::time::Duration::from_millis(250));
            pad::analog(&session, side, "centre")?;
            println!("{side} stick {direction}");
            Ok(())
        }
        "inspect" => {
            let path = PathBuf::from(args.get(1).ok_or_else(|| Error::Invalid("inspect <path>".into()))?);
            let info = convert::inspect(&path)?;
            println!("name      {}", info.name);
            println!("kind      {}", info.kind);
            println!("system    {}", info.system);
            println!("title id  {}", info.title_id.unwrap_or_else(|| "-".into()));
            println!("size      {} bytes", info.size_bytes);
            if info.parts > 1 {
                println!("parts     {}", info.parts);
            }
            Ok(())
        }
        "iso" => {
            let src = PathBuf::from(args.get(1).ok_or_else(|| Error::Invalid("iso <folder> <out.iso>".into()))?);
            let out = PathBuf::from(args.get(2).ok_or_else(|| Error::Invalid("iso <folder> <out.iso>".into()))?);
            run_job("iso", move |ctx| convert::folder_to_iso(&src, &out, ctx).map(|_| ()))
        }
        "extract" => {
            let src = PathBuf::from(args.get(1).ok_or_else(|| Error::Invalid("extract <in.iso> <out-dir>".into()))?);
            let out = PathBuf::from(args.get(2).ok_or_else(|| Error::Invalid("extract <in.iso> <out-dir>".into()))?);
            run_job("extract", move |ctx| convert::iso_to_folder(&src, &out, ctx).map(|_| ()))
        }
        "zip" => {
            let src = PathBuf::from(args.get(1).ok_or_else(|| Error::Invalid("zip <folder> <out.zip>".into()))?);
            let out = PathBuf::from(args.get(2).ok_or_else(|| Error::Invalid("zip <folder> <out.zip>".into()))?);
            run_job("zip", move |ctx| convert::folder_to_zip(&src, &out, ctx).map(|_| ()))
        }
        "unzip" => {
            let src = PathBuf::from(args.get(1).ok_or_else(|| Error::Invalid("unzip <in.zip> <out-dir>".into()))?);
            let out = PathBuf::from(args.get(2).ok_or_else(|| Error::Invalid("unzip <in.zip> <out-dir>".into()))?);
            run_job("unzip", move |ctx| convert::zip_to_folder(&src, &out, ctx).map(|_| ()))
        }
        "split" => {
            let src = PathBuf::from(args.get(1).ok_or_else(|| Error::Invalid("split <file>".into()))?);
            let chunk = flag_value(args, "--chunk-mb")
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|v| *v > 0)
                .unwrap_or(4095)
                * 1024
                * 1024;
            run_job("split", move |ctx| convert::split_file(&src, chunk, ctx).map(|_| ()))
        }
        "merge" => {
            let src = PathBuf::from(args.get(1).ok_or_else(|| Error::Invalid("merge <base> <out>".into()))?);
            let out = PathBuf::from(args.get(2).ok_or_else(|| Error::Invalid("merge <base> <out>".into()))?);
            run_job("merge", move |ctx| convert::merge_file(&src, &out, ctx).map(|_| ()))
        }
        other => Err(Error::Invalid(format!("unknown command {other}; run godsend-ps5 help"))),
    }
}

fn run_job<F>(label: &str, work: F) -> Result<()>
where
    F: FnOnce(&godsend_core::transfer::JobContext) -> Result<()> + Send + 'static,
{
    let manager = JobManager::new();
    let id = manager.spawn("cli", label, |_| {}, work);
    let mut last_pct = 0u64;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let Some(job) = manager.get(id) else {
            continue;
        };
        let pct = if job.total_bytes > 0 {
            job.done_bytes * 100 / job.total_bytes
        } else {
            0
        };
        if pct >= last_pct + 10 {
            last_pct = pct;
            eprintln!("{label}: {pct}%");
        }
        match job.state {
            godsend_core::JobState::Running => {}
            godsend_core::JobState::Done => {
                println!("{label}: done {}", job.message);
                return Ok(());
            }
            godsend_core::JobState::Failed => return Err(Error::Other(job.message)),
            godsend_core::JobState::Cancelled => return Err(Error::Other("cancelled".into())),
        }
    }
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let index = args.iter().position(|a| a == flag)?;
    args.get(index + 1).cloned()
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "up"
    } else {
        "missing"
    }
}

fn version_suffix(version: &Option<String>) -> String {
    version
        .as_ref()
        .map(|v| format!(" v{v}"))
        .unwrap_or_default()
}

fn auto_subnet() -> String {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok();
    if let Some(socket) = socket {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                if let std::net::IpAddr::V4(ip) = addr.ip() {
                    let octets = ip.octets();
                    return format!("{}.{}.{}", octets[0], octets[1], octets[2]);
                }
            }
        }
    }
    "192.168.1".into()
}
