use std::{
    env, fs,
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
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
    config::UpdateSettings,
    releases::{self, Release, Update},
    BUILD_VERSION,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Job<T> = Receiver<std::result::Result<T, String>>;
const API: &str = "https://api.github.com/repos/slavonkanarov/Volume3/releases";
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);

pub enum Event {
    Checked(std::result::Result<Vec<Update>, String>, bool),
    Downloaded(std::result::Result<Downloaded, String>),
}

enum Pending {
    Check(Job<Vec<Update>>, bool),
    Download(Job<Downloaded>),
}

pub struct Updater {
    executable: PathBuf,
    installed: Option<String>,
    pub supported: bool,
    pub available: Option<Update>,
    pub status: String,
    pub releases: Vec<Update>,
    pub selected: usize,
    pub downloaded: Option<Downloaded>,
    pub install_requested: bool,
    pending: Option<Pending>,
    next_check: Instant,
    cancelled: Arc<AtomicBool>,
    received: Arc<AtomicU64>,
    total: u64,
}

impl Updater {
    pub fn new() -> std::io::Result<Self> {
        let executable = env::current_exe()?;
        let installed = msi_is_registered_for(&executable);
        let supported = !executable
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("volume3.exe"));
        let now = Instant::now();
        Ok(Self {
            executable,
            installed,
            supported,
            available: None,
            status: String::new(),
            pending: None,
            releases: Vec::new(),
            selected: 0,
            downloaded: None,
            install_requested: false,
            cancelled: Arc::new(AtomicBool::new(false)),
            received: Arc::new(AtomicU64::new(0)),
            total: 0,
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

    pub fn check(&mut self, manual: bool) {
        if !self.supported || self.busy() {
            return;
        }
        let installed = self.installed.is_some();
        self.cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = self.cancelled.clone();
        self.downloaded = None;
        self.next_check = Instant::now() + CHECK_INTERVAL;
        self.status = "Checking for updates...".into();
        self.pending = Some(Pending::Check(
            worker(move || check(installed, &cancelled)),
            manual,
        ));
    }

    pub fn download(&mut self, update: Update) {
        if self.busy() {
            return;
        }
        let current = semver::Version::parse(BUILD_VERSION).expect("valid build version");
        let downgrade = self.installed.is_some()
            && (
                update.version.major,
                update.version.minor,
                update.version.patch,
            ) < (current.major, current.minor, current.patch);
        let recovery = if downgrade {
            let Some(recovery) = self
                .releases
                .iter()
                .find(|release| release.version == current)
            else {
                self.status =
                    "Downgrade requires the current version's recovery MSI in Releases.".into();
                return;
            };
            recovery.assets.first().cloned()
        } else {
            None
        };
        self.cancelled = Arc::new(AtomicBool::new(false));
        self.received = Arc::new(AtomicU64::new(0));
        self.total = update.assets.iter().map(|asset| asset.size).sum::<u64>()
            + recovery.as_ref().map_or(0, |asset| asset.size);
        let cancelled = self.cancelled.clone();
        let received = self.received.clone();
        self.status = format!("Downloading {}...", update.version);
        self.downloaded = None;
        self.pending = Some(Pending::Download(worker(move || {
            download(update, recovery, &cancelled, &received)
        })));
    }

    pub fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.pending = None;
        self.downloaded = None;
        self.install_requested = false;
        self.status = "Cancelled.".into();
    }

    pub fn progress(&self) -> String {
        if matches!(self.pending, Some(Pending::Download(_))) && self.total > 0 {
            format!(
                "{} {}%",
                self.status,
                self.received.load(Ordering::Relaxed) * 100 / self.total
            )
        } else {
            self.status.clone()
        }
    }

    pub fn select_available(&mut self, include_prereleases: bool) {
        let current = semver::Version::parse(BUILD_VERSION).expect("valid build version");
        self.available = self
            .releases
            .iter()
            .find(|release| {
                release.version.cmp_precedence(&current).is_gt()
                    && (include_prereleases || release.version.pre.is_empty())
            })
            .cloned();
        self.selected = self
            .releases
            .iter()
            .position(|release| include_prereleases || release.version.pre.is_empty())
            .unwrap_or(0);
    }

    pub fn tick(&mut self, preferences: &UpdateSettings) -> Option<Event> {
        let event = match &self.pending {
            Some(Pending::Check(job, manual)) => {
                completed(job).map(|result| Event::Checked(result, *manual))
            }
            Some(Pending::Download(job)) => completed(job).map(Event::Downloaded),
            None => {
                if preferences.check_automatically
                    && self.downloaded.is_none()
                    && Instant::now() >= self.next_check
                {
                    self.check(false);
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
                installed: self.installed.is_some(),
                product: self.installed.as_deref(),
                recovery: downloaded.recovery.as_ref(),
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

fn check(installed: bool, cancelled: &AtomicBool) -> Result<Vec<Update>> {
    let client = client(Duration::from_secs(30))?;
    let mut releases = Vec::new();
    // Bound the catalog to recent releases to keep checks and manual selection responsive.
    for page in 1..=3 {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
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
    Ok(releases::compatible(releases, installed))
}

#[derive(Serialize)]
struct Package {
    name: String,
    sha256: String,
}

pub struct Downloaded {
    pub version: semver::Version,
    folder: TempDir,
    packages: Vec<Package>,
    recovery: Option<Package>,
}

fn download(
    update: Update,
    recovery: Option<releases::Asset>,
    cancelled: &AtomicBool,
    progress: &AtomicU64,
) -> Result<Downloaded> {
    let client = client(Duration::from_secs(180))?;
    let folder = tempfile::Builder::new()
        .prefix("volume3-update-")
        .tempdir()?;
    let mut packages = Vec::new();
    let recovery_name = recovery.as_ref().map(|asset| asset.name.clone());
    let mut recovery_package = None;
    for asset in update.assets.into_iter().chain(recovery) {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
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
            if cancelled.load(Ordering::Relaxed) {
                return Err("Cancelled".into());
            }
            let count = response.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            received += count as u64;
            progress.fetch_add(count as u64, Ordering::Relaxed);
            hash.update(&buffer[..count]);
            file.write_all(&buffer[..count])?;
        }
        if received != asset.size || format!("{:x}", hash.finalize()) != sha256 {
            return Err(format!("Download size or SHA-256 mismatch: {}", asset.name).into());
        }
        file.sync_all()?;
        let package = Package {
            name: asset.name,
            sha256,
        };
        if recovery_name.as_ref() == Some(&package.name) {
            recovery_package = Some(package);
        } else {
            packages.push(package);
        }
    }
    Ok(Downloaded {
        version: update.version,
        folder,
        packages,
        recovery: recovery_package,
    })
}

fn msi_is_registered_for(executable: &Path) -> Option<String> {
    let Ok(executable) = executable.canonicalize() else {
        return None;
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
            return Some(String::from_utf16_lossy(&product[..38]));
        }
    }
    None
}

#[derive(Serialize)]
struct InstallPlan<'a> {
    executable: &'a Path,
    config: PathBuf,
    process_id: u32,
    installed: bool,
    product: Option<&'a str>,
    recovery: Option<&'a Package>,
    packages: &'a [Package],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabling_checks_prevents_startup_work_and_keeps_manual_results() {
        let mut updater = Updater::new().unwrap();
        updater.next_check = Instant::now();
        let preferences = UpdateSettings {
            check_automatically: false,
            ..Default::default()
        };
        assert!(updater.tick(&preferences).is_none());
        assert!(!updater.busy());
        // A completed fake job exercises the manual event path without using the network.
        let (sender, receiver) = mpsc::channel();
        sender.send(Ok(Vec::new())).unwrap();
        updater.pending = Some(Pending::Check(receiver, true));
        assert!(matches!(
            updater.tick(&preferences),
            Some(Event::Checked(Ok(_), true))
        ));
        assert!(!updater.busy());
    }

    #[test]
    fn automatic_selection_excludes_older_versions_and_optional_prereleases() {
        let mut updater = Updater::new().unwrap();
        let current = semver::Version::parse(BUILD_VERSION).unwrap();
        let stable = semver::Version::new(current.major, current.minor, current.patch + 1);
        let prerelease = semver::Version::parse(&format!(
            "{}.{}.{}-rc.1",
            current.major,
            current.minor,
            current.patch + 2
        ))
        .unwrap();
        updater.releases = [prerelease.clone(), stable.clone(), current]
            .into_iter()
            .map(|version| Update {
                version,
                assets: Vec::new(),
                notes: String::new(),
            })
            .collect();
        updater.select_available(false);
        assert_eq!(updater.available.as_ref().unwrap().version, stable);
        assert_eq!(updater.selected, 1);
        updater.select_available(true);
        assert_eq!(updater.available.as_ref().unwrap().version, prerelease);
        updater.releases.drain(..2);
        updater.select_available(true);
        assert!(updater.available.is_none());
    }

    #[test]
    fn msi_downgrade_without_recovery_never_starts_a_download() {
        let mut updater = Updater::new().unwrap();
        updater.installed = Some("{00000000-0000-0000-0000-000000000000}".into());
        updater.download(Update {
            version: semver::Version::new(0, 0, 0),
            assets: Vec::new(),
            notes: String::new(),
        });
        assert!(!updater.busy());
        assert!(updater.status.contains("recovery MSI"));
    }
}
