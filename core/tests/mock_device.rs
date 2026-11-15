//! End-to-end tests against a fake PS5: a minimal FTP server, a Ghostpad TCP
//! server, an elfldr TCP server and a PKG Manager HTTP server. Exercises the
//! same paths the app drives on a real console.

use godsend_core::config::{Config, Host};
use godsend_core::device;
use godsend_core::ftp;
use godsend_core::ghostpad::Ghostpad;
use godsend_core::install;
use godsend_core::library;
use godsend_core::pad;
use godsend_core::payload;
use godsend_core::sfo::{build_sfo, SfoValue};
use godsend_core::transfer::JobManager;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

thread_local! {
    static PASSIVE: std::cell::RefCell<Option<TcpListener>> = std::cell::RefCell::new(None);
}

// ---------- fake FTP ----------

struct FakeFtp {
    port: u16,
}

impl FakeFtp {
    fn start(root: PathBuf) -> FakeFtp {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ftp");
        let port = listener.local_addr().unwrap().port();
        let served = root.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let root = served.clone();
                std::thread::spawn(move || serve_control(stream, root));
            }
        });
        FakeFtp { port }
    }

    fn host(&self) -> Host {
        let mut host = Host::new("test ps5", "127.0.0.1");
        host.ftp_port = self.port;
        host
    }
}

fn map(root: &Path, remote: &str) -> PathBuf {
    root.join(remote.trim_start_matches('/'))
}

fn serve_control(stream: TcpStream, root: PathBuf) {
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    let mut cwd = "/".to_string();
    fn reply(writer: &mut TcpStream, text: &str) {
        let _ = writer.write_all(format!("{text}\r\n").as_bytes());
    }
    reply(&mut writer, "220 mock ftp ready");
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let line = line.trim_end_matches(['\r', '\n']).to_string();
        let (cmd, arg) = match line.split_once(' ') {
            Some((c, a)) => (c.to_uppercase(), a.trim().to_string()),
            None => (line.to_uppercase(), String::new()),
        };
        match cmd.as_str() {
            "USER" => reply(&mut writer, "331 password required"),
            "PASS" => reply(&mut writer, "230 logged in"),
            "TYPE" | "NOOP" | "OPTS" | "MODE" | "STRU" | "SITE" | "ABOR" => {
                reply(&mut writer, "200 ok")
            }
            "SYST" => reply(&mut writer, "215 UNIX Type: L8"),
            "FEAT" => {
                let _ = writer.write_all(b"211-Features:\r\n UTF8\r\n211 End\r\n");
            }
            "PWD" => reply(&mut writer, &format!("257 \"{cwd}\"")),
            "CWD" => {
                let target = if arg.starts_with('/') {
                    arg.clone()
                } else {
                    format!("{}/{}", cwd.trim_end_matches('/'), arg)
                };
                if map(&root, &target).is_dir() {
                    cwd = target;
                    reply(&mut writer, "250 ok");
                } else {
                    reply(&mut writer, "550 no such directory");
                }
            }
            "CDUP" => {
                if let Some((parent, _)) = cwd.trim_end_matches('/').rsplit_once('/') {
                    cwd = if parent.is_empty() {
                        "/".into()
                    } else {
                        parent.into()
                    };
                }
                reply(&mut writer, "250 ok");
            }
            "PASV" | "EPSV" => {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let port = listener.local_addr().unwrap().port();
                if cmd == "EPSV" {
                    reply(
                        &mut writer,
                        &format!("229 Entering Extended Passive Mode (|||{port}|)"),
                    );
                } else {
                    reply(
                        &mut writer,
                        &format!(
                            "227 Entering Passive Mode (127,0,0,1,{},{})",
                            port / 256,
                            port % 256
                        ),
                    );
                }
                PASSIVE.with(|slot| *slot.borrow_mut() = Some(listener));
            }
            "MLSD" => reply(&mut writer, "502 MLSD not supported"),
            "LIST" => {
                let target = if arg.is_empty() || arg.starts_with('-') {
                    cwd.clone()
                } else {
                    arg.clone()
                };
                let path = map(&root, &target);
                reply(&mut writer, "150 here it comes");
                if let Some(listener) = PASSIVE.with(|slot| slot.borrow_mut().take()) {
                    if let Ok((mut data, _)) = listener.accept() {
                        if let Ok(entries) = std::fs::read_dir(&path) {
                            for entry in entries.flatten() {
                                let meta = entry.metadata().unwrap();
                                let name = entry.file_name().to_string_lossy().to_string();
                                let perm = if meta.is_dir() {
                                    "drwxr-xr-x"
                                } else {
                                    "-rw-r--r--"
                                };
                                let _ = data.write_all(
                                    format!(
                                        "{perm} 1 root root {} Jan 01 12:00 {name}\r\n",
                                        meta.len()
                                    )
                                    .as_bytes(),
                                );
                            }
                        }
                        let _ = data.flush();
                    }
                }
                reply(&mut writer, "226 transfer complete");
            }
            "SIZE" => {
                let path = map(&root, &arg);
                match std::fs::metadata(&path) {
                    Ok(meta) => reply(&mut writer, &format!("213 {}", meta.len())),
                    Err(_) => reply(&mut writer, "550 no such file"),
                }
            }
            "RETR" => {
                let path = map(&root, &arg);
                reply(&mut writer, "150 sending");
                if let Some(listener) = PASSIVE.with(|slot| slot.borrow_mut().take()) {
                    if let Ok((mut data, _)) = listener.accept() {
                        if let Ok(mut file) = std::fs::File::open(&path) {
                            let _ = std::io::copy(&mut file, &mut data);
                        }
                        let _ = data.flush();
                    }
                }
                reply(&mut writer, "226 transfer complete");
            }
            "STOR" => {
                let path = map(&root, &arg);
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                reply(&mut writer, "150 send it");
                if let Some(listener) = PASSIVE.with(|slot| slot.borrow_mut().take()) {
                    if let Ok((mut data, _)) = listener.accept() {
                        if let Ok(mut file) = std::fs::File::create(&path) {
                            let _ = std::io::copy(&mut data, &mut file);
                        }
                    }
                }
                reply(&mut writer, "226 transfer complete");
            }
            "MKD" => {
                let path = map(&root, &arg);
                let _ = std::fs::create_dir_all(&path);
                reply(&mut writer, "257 created");
            }
            "DELE" => {
                let path = map(&root, &arg);
                match std::fs::remove_file(&path) {
                    Ok(()) => reply(&mut writer, "250 deleted"),
                    Err(_) => reply(&mut writer, "550 delete failed"),
                }
            }
            "RMD" => {
                let path = map(&root, &arg);
                match std::fs::remove_dir_all(&path) {
                    Ok(()) => reply(&mut writer, "250 removed"),
                    Err(_) => reply(&mut writer, "550 rmdir failed"),
                }
            }
            "QUIT" => {
                reply(&mut writer, "221 bye");
                return;
            }
            _ => reply(&mut writer, "502 not implemented"),
        }
    }
}

// ---------- fake Ghostpad ----------

struct FakePad {
    port: u16,
    frames: Arc<Mutex<Vec<[u8; 16]>>>,
}

impl FakePad {
    fn start() -> FakePad {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind pad");
        let port = listener.local_addr().unwrap().port();
        let frames = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&frames);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let sink = Arc::clone(&sink);
                std::thread::spawn(move || {
                    let mut frame = [0u8; 16];
                    loop {
                        match stream.read_exact(&mut frame) {
                            Ok(()) => sink.lock().unwrap().push(frame),
                            Err(_) => break,
                        }
                    }
                });
            }
        });
        FakePad { port, frames }
    }
}

// ---------- fake elfldr ----------

struct FakeLoader {
    port: u16,
    received: Arc<Mutex<Vec<u8>>>,
}

impl FakeLoader {
    fn start() -> FakeLoader {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loader");
        let port = listener.local_addr().unwrap().port();
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&received);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let sink = Arc::clone(&sink);
                std::thread::spawn(move || {
                    let mut bytes = Vec::new();
                    let _ = stream.read_to_end(&mut bytes);
                    sink.lock().unwrap().extend_from_slice(&bytes);
                    let _ = stream.write_all(b"loaded");
                });
            }
        });
        FakeLoader { port, received }
    }
}

// ---------- fake PKG Manager ----------

fn fake_http() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind http");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
            });
        }
    });
    port
}

// ---------- fixtures ----------

fn appmeta_sfo(title: &str, title_id: &str) -> Vec<u8> {
    build_sfo(&[
        ("TITLE", SfoValue::Utf8(title.into())),
        ("TITLE_ID", SfoValue::Utf8(title_id.into())),
        ("VERSION", SfoValue::Utf8("01.000.000".into())),
    ])
}

fn fixture_root() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let slot = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "godsend-ps5-mock-{}-{slot}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("user/app/PPSA01234")).unwrap();
    std::fs::create_dir_all(root.join("user/appmeta/PPSA01234")).unwrap();
    std::fs::write(
        root.join("user/appmeta/PPSA01234/param.sfo"),
        appmeta_sfo("Demon's Souls", "PPSA01234"),
    )
    .unwrap();
    std::fs::write(root.join("user/appmeta/PPSA01234/icon0.png"), b"\x89PNG\r\n\x1a\nfake").unwrap();

    std::fs::create_dir_all(root.join("data/homebrew/God of War")).unwrap();
    std::fs::write(
        root.join("data/homebrew/God of War/PPSA01235 - God of War.exfat"),
        vec![1u8; 4096],
    )
    .unwrap();
    std::fs::create_dir_all(root.join("data/homebrew/God of War/sce_sys")).unwrap();
    std::fs::write(
        root.join("data/homebrew/God of War/sce_sys/param.sfo"),
        appmeta_sfo("God of War", "PPSA01235"),
    )
    .unwrap();

    std::fs::create_dir_all(root.join("data/ps5upload/pkg_library")).unwrap();
    let mut pkg = vec![0u8; 512];
    pkg[0..4].copy_from_slice(&godsend_core::pkg::MAGIC);
    pkg[4..8].copy_from_slice(&1u32.to_be_bytes());
    pkg[0x30..0x30 + 36].copy_from_slice(b"UP0000-PPSA09999_00-0000000000000000");
    std::fs::write(root.join("data/ps5upload/pkg_library/base.pkg"), pkg).unwrap();

    std::fs::create_dir_all(root.join("data/etaHEN")).unwrap();
    std::fs::write(
        root.join("data/etaHEN/config.ini"),
        b"# etaHEN\nFTP=1\nVERSION=2.5B\n",
    )
    .unwrap();

    std::fs::create_dir_all(root.join("mnt")).unwrap();
    root
}

fn test_config(root: &Path) -> Config {
    let mut config = Config::default();
    config.download_dir = root.join("downloads").to_string_lossy().to_string();
    config.homebrew_dir = "/data/homebrew".into();
    config.pkg_dir = "/data/ps5upload/pkg_library".into();
    config
}

fn wait_for<F: Fn() -> bool>(check: F) -> bool {
    for _ in 0..200 {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

// ---------- tests ----------

#[test]
fn scans_apps_images_and_packages() {
    let root = fixture_root();
    let ftp = FakeFtp::start(root.clone());
    let host = ftp.host();
    let view = library::scan_with(&host, &test_config(&root)).expect("scan");
    let titles: Vec<&str> = view.games.iter().map(|g| g.title.as_str()).collect();
    assert!(titles.contains(&"Demon's Souls"), "apps: {titles:?}");
    assert!(titles.contains(&"God of War"), "images: {titles:?}");
    let image = view.games.iter().find(|g| g.title == "God of War").unwrap();
    assert_eq!(image.kind, "image");
    assert_eq!(image.title_id.as_deref(), Some("PPSA01235"));
    assert!(view.packages.iter().any(|p| p.title_id.as_deref() == Some("PPSA09999")));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn fetches_and_caches_an_icon() {
    let root = fixture_root();
    let ftp = FakeFtp::start(root.clone());
    let host = ftp.host();
    let bytes = library::icon_bytes(&host, "/user/appmeta/PPSA01234/icon0.png").expect("icon");
    assert!(bytes.starts_with(b"\x89PNG"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn uploads_a_pkg_into_the_install_folder() {
    let root = fixture_root();
    let ftp = FakeFtp::start(root.clone());
    let host = ftp.host();
    let config = test_config(&root);
    let local = root.join("incoming.pkg");
    let mut pkg = vec![0u8; 512];
    pkg[0..4].copy_from_slice(&godsend_core::pkg::MAGIC);
    std::fs::write(&local, &pkg).unwrap();

    let manager = JobManager::new();
    let host_for_job = host.clone();
    let config_for_job = config.clone();
    let local_for_job = local.clone();
    manager.spawn(
        "install",
        "test",
        |_| {},
        move |ctx| {
            install::install_local(&host_for_job, &local_for_job, &config_for_job, ctx)?;
            Ok(())
        },
    );
    let uploaded = root.join("data/ps5upload/pkg_library/incoming.pkg");
    assert!(wait_for(|| uploaded.exists()), "pkg was not uploaded");
    assert_eq!(std::fs::read(&uploaded).unwrap().len(), 512);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn uploads_a_folder_dump_to_homebrew() {
    let root = fixture_root();
    let ftp = FakeFtp::start(root.clone());
    let host = ftp.host();
    let config = test_config(&root);
    let dump = root.join("incoming/PPSA05555");
    std::fs::create_dir_all(dump.join("sce_sys")).unwrap();
    std::fs::write(dump.join("eboot.bin"), vec![9u8; 64]).unwrap();
    std::fs::write(
        dump.join("sce_sys/param.sfo"),
        appmeta_sfo("Folder Dump", "PPSA05555"),
    )
    .unwrap();

    let manager = JobManager::new();
    let (h, c, d) = (host.clone(), config.clone(), dump.clone());
    manager.spawn(
        "install",
        "test",
        |_| {},
        move |ctx| {
            install::install_local(&h, &d, &c, ctx)?;
            Ok(())
        },
    );
    let eboot = root.join("data/homebrew/PPSA05555/eboot.bin");
    assert!(wait_for(|| eboot.exists()), "folder dump was not uploaded");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn sends_a_payload_to_elfldr() {
    let loader = FakeLoader::start();
    let mut host = Host::new("test", "127.0.0.1");
    host.payload_port = loader.port;
    let report = payload::send(&host, b"\x7fELF fake payload").expect("send");
    assert_eq!(report.bytes, 17);
    assert!(wait_for(|| loader.received.lock().unwrap().len() == 17));
    assert_eq!(&loader.received.lock().unwrap()[..4], b"\x7fELF");
}

#[test]
fn drives_the_virtual_pad() {
    let pad_server = FakePad::start();
    let mut host = Host::new("test", "127.0.0.1");
    host.pad_port = pad_server.port;
    let session = Ghostpad::connect(&host).expect("connect pad");
    pad::press(&session, "cross").expect("press");
    pad::analog(&session, "L", "up_right").expect("stick");
    std::thread::sleep(Duration::from_millis(60));
    let frames = pad_server.frames.lock().unwrap().clone();
    assert!(!frames.is_empty(), "no frames reached the payload");
    let cross = godsend_core::pad::Button::parse("cross").unwrap().bit();
    assert!(
        frames.iter().any(|f| u32::from_be_bytes([f[4], f[5], f[6], f[7]]) & cross != 0),
        "cross was never held"
    );
    assert!(
        frames.iter().any(|f| f[8] == 255 && f[9] == 0),
        "stick never reached up_right"
    );
    // Releasing one button leaves the other held.
    pad::hold(&session, "cross").expect("hold cross");
    pad::hold(&session, "circle").expect("hold circle");
    std::thread::sleep(Duration::from_millis(40));
    pad::release_button(&session, "cross").expect("release cross");
    std::thread::sleep(Duration::from_millis(40));
    let mask = pad_server
        .frames
        .lock()
        .unwrap()
        .last()
        .map(|f| u32::from_be_bytes([f[4], f[5], f[6], f[7]]))
        .unwrap_or(0);
    let circle = godsend_core::pad::Button::parse("circle").unwrap().bit();
    assert_eq!(mask & cross, 0, "cross should be lifted");
    assert_eq!(mask & circle, circle, "circle should still be held");

    drop(session);
    // Dropping the session must send a neutral frame, or the payload keeps the
    // last button held.
    std::thread::sleep(Duration::from_millis(80));
    let last = *pad_server.frames.lock().unwrap().last().expect("frames");
    assert_eq!(u32::from_be_bytes([last[4], last[5], last[6], last[7]]), 0);
    assert_eq!(last[8], 128);
    assert_eq!(last[9], 128);
}

#[test]
fn probes_ftp_etahen_pad_loader_and_pkg_manager() {
    let root = fixture_root();
    let ftp = FakeFtp::start(root.clone());
    let pad_server = FakePad::start();
    let loader = FakeLoader::start();
    let http = fake_http();
    let mut host = ftp.host();
    host.pad_port = pad_server.port;
    host.payload_port = loader.port;
    host.pkg_manager_port = http;

    let status = device::probe(&host);
    assert!(status.ftp_ok, "ftp: {}", status.message);
    assert!(status.etahen_ok);
    assert_eq!(status.etahen_version.as_deref(), Some("2.5B"));
    assert!(status.homebrew_ok);
    assert!(status.pad_ok);
    assert!(status.payload_ok);
    assert!(status.pkg_manager_ok);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn lists_a_remote_directory() {
    let root = fixture_root();
    let ftp = FakeFtp::start(root.clone());
    let host = ftp.host();
    let mut conn = ftp::connect(&host).expect("connect");
    let entries = ftp::list(&mut conn, "/data/homebrew").expect("list");
    assert!(entries.iter().any(|e| e.is_dir && e.name == "God of War"));
    let _ = conn.quit();
    let _ = std::fs::remove_dir_all(&root);
}
