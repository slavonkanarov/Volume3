use std::{
    env, fs,
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use reqwest::{blocking::Client, redirect::Policy};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use windows::{
    core::{w, PCWSTR, PWSTR},
    Win32::System::ApplicationInstallationAndServicing::{
        MsiEnumRelatedProductsW, MsiGetComponentPathW, INSTALLSTATE_LOCAL,
    },
};

use crate::{
    releases::{self, Release, Update},
    BUILD_VERSION,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Job<T> = Receiver<std::result::Result<T, String>>;
const API: &str = "https://api.github.com/repos/slavonkanarov/Volume3/releases";
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);

pub enum Event {
    Checked(std::result::Result<Option<Update>, String>, bool),
    Downloaded(std::result::Result<Downloaded, String>),
}

enum Pending {
    Check(Job<Option<Update>>, bool),
    Download(Job<Downloaded>),
}

pub struct Updater {
    executable: PathBuf,
    installed: bool,
    pub supported: bool,
    pub available: Option<Update>,
    pub status: String,
    pending: Option<Pending>,
    next_check: Instant,
}

impl Updater {
    pub fn new() -> std::io::Result<Self> {
        let executable = env::current_exe()?;
        let installed = msi_is_registered_for(&executable);
        let supported = installed
            || (!executable.with_file_name("volume3.installed").exists()
                && !executable
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case("volume3.exe")));
        let now = Instant::now();
        Ok(Self {
            executable,
            installed,
            supported,
            available: None,
            status: String::new(),
            pending: None,
            next_check: if env::var_os("VOLUME3_UPDATE_FAILED").is_some() {
                now + CHECK_INTERVAL
            } else {
                now
            },
        })
    }

    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn check(&mut self, manual: bool, include_prereleases: bool) {
        if !self.supported || self.busy() {
            return;
        }
        let installed = self.installed;
        self.next_check = Instant::now() + CHECK_INTERVAL;
        self.available = None;
        self.status = "Checking for updates...".into();
        self.pending = Some(Pending::Check(
            worker(move || check(installed, include_prereleases)),
            manual,
        ));
    }

    pub fn download(&mut self) {
        if !self.busy() {
            if let Some(update) = self.available.take() {
                self.status = format!("Downloading {}...", update.version);
                self.pending = Some(Pending::Download(worker(move || download(update))));
            }
        }
    }

    pub fn tick(&mut self, include_prereleases: bool) -> Option<Event> {
        let event = match &self.pending {
            Some(Pending::Check(job, manual)) => {
                completed(job).map(|result| Event::Checked(result, *manual))
            }
            Some(Pending::Download(job)) => completed(job).map(Event::Downloaded),
            None => {
                if Instant::now() >= self.next_check {
                    self.check(false, include_prereleases);
                }
                return None;
            }
        };
        if event.is_some() {
            self.pending = None;
        }
        event
    }

    pub fn install(&self, downloaded: Downloaded, config: &Path) -> Result<()> {
        let folder = downloaded.folder.path();
        let script = folder.join("install.ps1");
        let plan = folder.join("plan.json");
        fs::write(&script, include_str!("updates/install.ps1"))?;
        fs::write(
            &plan,
            serde_json::to_vec(&InstallPlan {
                executable: &self.executable,
                config: std::path::absolute(config)?,
                process_id: std::process::id(),
                installed: self.installed,
                packages: &downloaded.packages,
            })?,
        )?;
        let system_root = env::var_os("SystemRoot").ok_or("SystemRoot is not set")?;
        let powershell =
            PathBuf::from(system_root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        let mut child = Command::new(powershell)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(script)
            .arg(plan)
            .creation_flags(0x08000000)
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let ready = folder.join("ready");
        while !ready.is_file() {
            if child.try_wait()?.is_some() {
                return Err("Update helper exited before it was ready".into());
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                return Err("Update helper did not start in time".into());
            }
            thread::sleep(Duration::from_millis(25));
        }
        fs::write(folder.join("go"), b"")?;
        let _ = downloaded.folder.keep();
        Ok(())
    }
}

fn completed<T>(job: &Job<T>) -> Option<std::result::Result<T, String>> {
    match job.try_recv() {
        Ok(result) => Some(result),
        Err(mpsc::TryRecvError::Empty) => None,
        Err(mpsc::TryRecvError::Disconnected) => {
            Some(Err("Update worker stopped unexpectedly".into()))
        }
    }
}

fn worker<T: Send + 'static>(run: impl FnOnce() -> Result<T> + Send + 'static) -> Job<T> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(run().map_err(|error| error.to_string()));
    });
    receiver
}

fn client(timeout: Duration) -> Result<Client> {
    Ok(Client::builder()
        .user_agent(concat!("Volume3/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() < 5
                && matches!(
                    attempt.url().host_str(),
                    Some("release-assets.githubusercontent.com")
                )
                && attempt.url().scheme() == "https"
            {
                attempt.follow()
            } else {
                attempt.error("Unexpected GitHub redirect")
            }
        }))
        .build()?)
}

fn check(installed: bool, include_prereleases: bool) -> Result<Option<Update>> {
    let current = semver::Version::parse(BUILD_VERSION)?;
    let client = client(Duration::from_secs(30))?;
    let mut releases = Vec::new();
    // The updater only needs recent releases, not a catalog of older versions.
    for page in 1..=3 {
        let response = client
            .get(API)
            .query(&[("per_page", 100), ("page", page)])
            .header("Accept", "application/vnd.github+json")
            .send()?
            .error_for_status()?;
        let mut body = Vec::new();
        const LIMIT: u64 = 8 * 1024 * 1024;
        response.take(LIMIT + 1).read_to_end(&mut body)?;
        if body.len() as u64 > LIMIT {
            return Err("GitHub release list exceeds size limit".into());
        }
        let page: Vec<Release> = serde_json::from_slice(&body)?;
        let done = page.len() < 100;
        releases.extend(page);
        if done {
            break;
        }
    }
    Ok(releases::newest(
        releases,
        &current,
        installed,
        include_prereleases,
    ))
}

#[derive(Serialize)]
struct Package {
    name: String,
    sha256: String,
}

pub struct Downloaded {
    folder: TempDir,
    packages: Vec<Package>,
}

fn download(update: Update) -> Result<Downloaded> {
    let client = client(Duration::from_secs(180))?;
    let folder = tempfile::Builder::new()
        .prefix("volume3-update-")
        .tempdir()?;
    let mut packages = Vec::new();
    for asset in update.assets {
        let sha256 = asset
            .sha256()
            .ok_or("Invalid release checksum")?
            .to_ascii_lowercase();
        let mut response = client
            .get(&asset.browser_download_url)
            .send()?
            .error_for_status()?
            .take(asset.size + 1);
        let mut file = fs::File::create_new(folder.path().join(&asset.name))?;
        let mut hash = Sha256::new();
        let mut received = 0;
        let mut buffer = [0; 65536];
        loop {
            let count = response.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            received += count as u64;
            hash.update(&buffer[..count]);
            file.write_all(&buffer[..count])?;
        }
        if received != asset.size || format!("{:x}", hash.finalize()) != sha256 {
            return Err(format!("Download size or SHA-256 mismatch: {}", asset.name).into());
        }
        file.sync_all()?;
        packages.push(Package {
            name: asset.name,
            sha256,
        });
    }
    Ok(Downloaded { folder, packages })
}

fn msi_is_registered_for(executable: &Path) -> bool {
    let Ok(executable) = executable.canonicalize() else {
        return false;
    };
    // These are the package's UpgradeCode and tray component GUID in Volume3.wxs.
    let upgrade = w!("{51C13508-AAE9-4FA7-8FF7-33694E890351}");
    let component = w!("{861D5651-A04E-44C4-B5B6-EFF1A175C5C8}");
    let mut product = [0; 39];
    let mut path = vec![0; 32768];
    for index in 0..100 {
        if unsafe { MsiEnumRelatedProductsW(upgrade, None, index, PWSTR(product.as_mut_ptr())) }
            != 0
        {
            break;
        }
        let mut len = path.len() as u32;
        let state = unsafe {
            MsiGetComponentPathW(
                PCWSTR(product.as_ptr()),
                component,
                Some(PWSTR(path.as_mut_ptr())),
                Some(&mut len),
            )
        };
        if state == INSTALLSTATE_LOCAL
            && path.get(..len as usize).is_some_and(|path| {
                use std::os::windows::ffi::OsStringExt;
                PathBuf::from(std::ffi::OsString::from_wide(path))
                    .canonicalize()
                    .is_ok_and(|path| path == executable)
            })
        {
            return true;
        }
    }
    false
}

#[derive(Serialize)]
struct InstallPlan<'a> {
    executable: &'a Path,
    config: PathBuf,
    process_id: u32,
    installed: bool,
    packages: &'a [Package],
}
