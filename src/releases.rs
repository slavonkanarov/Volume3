use semver::Version;
use serde::Deserialize;

pub const DOWNLOAD_ROOT: &str = "https://github.com/slavonkanarov/Volume3/releases/download";
pub const MAX_ASSET: u64 = 256 * 1024 * 1024;
pub const PORTABLE_FILES: [&str; 2] = ["volume3-tray.exe", "volume3.exe"];

#[derive(Deserialize)]
pub struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
    #[serde(default)]
    body: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
    pub digest: Option<String>,
}

impl Asset {
    pub fn sha256(&self) -> Option<&str> {
        let hash = self.digest.as_deref()?.strip_prefix("sha256:")?;
        (hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(hash)
    }

    fn valid(&self, tag: &str) -> bool {
        self.size > 0
            && self.size <= MAX_ASSET
            && self.sha256().is_some()
            && self.browser_download_url == format!("{DOWNLOAD_ROOT}/{tag}/{}", self.name)
    }
}

#[derive(Clone)]
pub struct Update {
    pub version: Version,
    pub assets: Vec<Asset>,
    pub notes: String,
}

pub fn compatible(releases: Vec<Release>, installed: bool) -> Vec<Update> {
    let mut updates: Vec<_> = releases
        .into_iter()
        .filter_map(|release| {
            let version = Version::parse(release.tag_name.strip_prefix('v')?).ok()?;
            if release.draft
                || !version.build.is_empty()
                || release.prerelease == version.pre.is_empty()
            {
                return None;
            }
            let msi = format!("Volume3-{version}-x64.msi");
            let msi_files = [msi.as_str()];
            let names: &[&str] = if installed {
                &msi_files
            } else {
                &PORTABLE_FILES
            };
            let assets = names
                .iter()
                .map(|&name| {
                    release
                        .assets
                        .iter()
                        .find(|asset| asset.name == name && asset.valid(&release.tag_name))
                        .cloned()
                })
                .collect::<Option<Vec<_>>>()?;
            Some(Update {
                version,
                assets,
                notes: release.body.unwrap_or_default(),
            })
        })
        .collect();
    updates.sort_by(|a, b| b.version.cmp_precedence(&a.version));
    updates.dedup_by(|a, b| a.version == b.version);
    updates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool, names: &[&str]) -> Release {
        Release {
            tag_name: tag.into(),
            draft: false,
            prerelease,
            body: None,
            assets: names
                .iter()
                .map(|name| Asset {
                    name: (*name).into(),
                    size: 1,
                    browser_download_url: format!("{DOWNLOAD_ROOT}/{tag}/{name}"),
                    digest: Some(format!("sha256:{}", "a".repeat(64))),
                })
                .collect(),
        }
    }

    fn newest(
        releases: Vec<Release>,
        current: &Version,
        installed: bool,
        include_prereleases: bool,
    ) -> Option<Update> {
        compatible(releases, installed).into_iter().find(|update| {
            update.version.cmp_precedence(current).is_gt()
                && (include_prereleases || update.version.pre.is_empty())
        })
    }

    #[test]
    fn catalog_keeps_older_versions_and_notes_for_manual_installation() {
        let mut older = release("v0.1.4", false, &PORTABLE_FILES);
        older.body = Some("Release notes".into());
        let catalog = compatible(
            vec![older, release("v0.1.5-rc.1", true, &PORTABLE_FILES)],
            false,
        );
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].version.to_string(), "0.1.5-rc.1");
        assert_eq!(catalog[1].notes, "Release notes");
    }

    #[test]
    fn chooses_newest_eligible_version_by_semver() {
        let candidates = || {
            vec![
                release("main-99-1", false, &PORTABLE_FILES),
                release("v0.1.4", false, &PORTABLE_FILES),
                release("v0.1.9", false, &PORTABLE_FILES),
                release("v0.1.10", false, &PORTABLE_FILES),
                release("v0.1.11-rc.2", true, &PORTABLE_FILES),
            ]
        };
        let current = Version::parse("0.1.4").unwrap();
        let selected = newest(candidates(), &current, false, false).unwrap();
        assert_eq!(selected.version.to_string(), "0.1.10");
        assert_eq!(selected.assets.len(), 2);
        assert_eq!(
            newest(candidates(), &current, false, true)
                .unwrap()
                .version
                .to_string(),
            "0.1.11-rc.2"
        );
        assert!(newest(candidates(), &selected.version, false, false).is_none());
    }

    #[test]
    fn requires_exact_assets_with_valid_digest_and_url() {
        let current = Version::parse("0.1.4").unwrap();
        let mut bad = release("v0.1.7", false, &["Volume3-0.1.7-x64.msi"]);
        bad.assets[0].digest = Some("sha256:abcd".into());
        let mut wrong_url = release("v0.1.6", false, &["Volume3-0.1.6-x64.msi"]);
        wrong_url.assets[0].browser_download_url.push_str("?wrong");
        let partial = release("v0.1.8", false, &PORTABLE_FILES[..1]);
        assert!(newest(vec![partial], &current, false, false).is_none());
        let msi = release("v0.1.5", false, &["Volume3-0.1.5-x64.msi"]);
        let selected = newest(vec![bad, wrong_url, msi], &current, true, false).unwrap();
        assert_eq!(selected.assets[0].name, "Volume3-0.1.5-x64.msi");
    }
}
