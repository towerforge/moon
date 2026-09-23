//! Updating moon in place: what GitHub published, whether it is newer than
//! what is running, and the download that replaces the binary. No terminal
//! here — the UI lives in `moon-tui`.

pub mod cache;
pub mod install;
pub mod target;
pub mod version;

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub use cache::CheckCache;
pub use install::{install_binary, InstallKind, Refusal};
pub use target::{Target, APP};
pub use version::Version;

/// Where the releases are published. The same repository `install.sh` reads.
pub const REPO: &str = "towerforge/moon";
const API: &str = "https://api.github.com/repos";
const CHECKSUMS: &str = "checksums.txt";
const UA: &str = concat!("moon-updater/", env!("CARGO_PKG_VERSION"));
/// Enough for the JSON calls; the download has no deadline of its own, only
/// the read timeout of the client.
const API_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("no prebuilt binary for {0}")]
    Unsupported(String),
    #[error("github: {0}")]
    Http(String),
    #[error("no release published yet at https://github.com/{REPO}/releases")]
    NoRelease,
    #[error("there is no release {0} to install")]
    NoTag(String),
    #[error("release v{version} has no {asset}")]
    NoAsset { version: String, asset: String },
    #[error("`{0}` is not a version like 1.2.3")]
    BadVersion(String),
    #[error("checksum mismatch: expected {expected}, got {got}")]
    Checksum { expected: String, got: String },
    #[error("{0} does not contain the moon binary")]
    Archive(String),
    #[error(
        "cannot write to {0}: run it again with sudo, or set MOON_INSTALL_DIR and use install.sh"
    )]
    Denied(PathBuf),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl From<reqwest::Error> for UpdateError {
    fn from(e: reqwest::Error) -> Self {
        let mut msg = e.to_string();
        if e.is_connect() || e.is_timeout() {
            msg = format!("{msg} (no connection?)");
        }
        UpdateError::Http(msg)
    }
}

/// A published release, with what the UI shows about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: Version,
    pub tag: String,
    pub notes: String,
    pub url: String,
    pub published_at: Option<chrono::DateTime<chrono::Utc>>,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }

    /// The asset this platform needs.
    pub fn asset_for(&self, target: &Target) -> Result<&Asset, UpdateError> {
        let name = target.asset_name();
        self.asset(&name).ok_or_else(|| UpdateError::NoAsset {
            version: self.version.to_string(),
            asset: name,
        })
    }

    /// `checksums.txt`, published next to the archives.
    pub fn checksums_url(&self) -> Option<&str> {
        self.asset(CHECKSUMS).map(|a| a.url.as_str())
    }

    /// The first lines of the release notes, as bullets: what the UI shows
    /// under "What's new". GitHub writes `* title by @who in <url>`.
    pub fn highlights(&self, max: usize) -> Vec<String> {
        self.notes
            .lines()
            .map(str::trim)
            .filter_map(|l| l.strip_prefix("* ").or_else(|| l.strip_prefix("- ")))
            .map(|l| match l.find(" by @") {
                Some(i) => l[..i].trim(),
                None => l,
            })
            .filter(|l| !l.is_empty() && !l.starts_with("**Full Changelog**"))
            .map(|l| l.to_string())
            .take(max)
            .collect()
    }
}

#[derive(Deserialize)]
struct WireRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    published_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    assets: Vec<WireAsset>,
}

#[derive(Deserialize)]
struct WireAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

impl TryFrom<WireRelease> for Release {
    type Error = UpdateError;

    fn try_from(w: WireRelease) -> Result<Self, Self::Error> {
        let version: Version = w
            .tag_name
            .parse()
            .map_err(|_| UpdateError::BadVersion(w.tag_name.clone()))?;
        Ok(Release {
            version,
            tag: w.tag_name,
            notes: w.body.unwrap_or_default(),
            url: w.html_url,
            published_at: w.published_at,
            assets: w
                .assets
                .into_iter()
                .map(|a| Asset {
                    name: a.name,
                    url: a.browser_download_url,
                    size: a.size,
                })
                .collect(),
        })
    }
}

/// Talks to the GitHub releases API. Anonymous: 60 calls an hour per address,
/// which the daily cache keeps well clear of.
pub struct Client {
    http: reqwest::Client,
    repo: String,
}

impl Client {
    pub fn new() -> Result<Self, UpdateError> {
        Self::for_repo(REPO)
    }

    pub fn for_repo(repo: &str) -> Result<Self, UpdateError> {
        let http = reqwest::Client::builder()
            .user_agent(UA)
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            repo: repo.to_string(),
        })
    }

    /// The latest published release.
    pub async fn latest(&self) -> Result<Release, UpdateError> {
        self.release_at(&format!("{API}/{}/releases/latest", self.repo))
            .await
    }

    /// A specific version, by its tag.
    pub async fn release(&self, v: &Version) -> Result<Release, UpdateError> {
        self.release_at(&format!("{API}/{}/releases/tags/{}", self.repo, v.tag()))
            .await
            .map_err(|e| match e {
                UpdateError::NoRelease => UpdateError::NoTag(v.tag()),
                other => other,
            })
    }

    async fn release_at(&self, url: &str) -> Result<Release, UpdateError> {
        let resp = self
            .http
            .get(url)
            .timeout(API_TIMEOUT)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;
        match resp.status() {
            s if s.is_success() => {}
            reqwest::StatusCode::NOT_FOUND => return Err(UpdateError::NoRelease),
            reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::TOO_MANY_REQUESTS => {
                return Err(UpdateError::Http(
                    "rate limit reached, try again in a while".into(),
                ))
            }
            s => return Err(UpdateError::Http(format!("HTTP {}", s.as_u16()))),
        }
        let wire: WireRelease = resp.json().await?;
        Release::try_from(wire)
    }

    /// Text of an asset, for `checksums.txt`.
    pub async fn text(&self, url: &str) -> Result<String, UpdateError> {
        Ok(self
            .http
            .get(url)
            .timeout(API_TIMEOUT)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?)
    }

    /// Downloads an asset into memory, reporting bytes so far and total size.
    pub async fn download(
        &self,
        url: &str,
        mut on_progress: impl FnMut(u64, Option<u64>),
    ) -> Result<Vec<u8>, UpdateError> {
        let resp = self.http.get(url).send().await?.error_for_status()?;
        let total = resp.content_length();
        let mut out: Vec<u8> = Vec::with_capacity(total.unwrap_or(1 << 20) as usize);
        let mut stream = resp.bytes_stream();
        on_progress(0, total);
        while let Some(chunk) = stream.next().await {
            out.extend_from_slice(&chunk?);
            on_progress(out.len() as u64, total);
        }
        Ok(out)
    }
}

/// SHA-256 of the archive against the line for it in `checksums.txt`. A file
/// with no entry for this asset is not an error: `install.sh` warns and goes
/// on, and so does this.
pub fn verify_sha256(bytes: &[u8], checksums: &str, asset: &str) -> Result<bool, UpdateError> {
    let Some(expected) = checksums
        .lines()
        .filter_map(|l| l.split_once("  ").or_else(|| l.split_once(' ')))
        .find(|(_, name)| name.trim().trim_start_matches('*') == asset)
        .map(|(hash, _)| hash.trim().to_lowercase())
    else {
        return Ok(false);
    };
    let got = format!("{:x}", Sha256::digest(bytes));
    if got == expected {
        Ok(true)
    } else {
        Err(UpdateError::Checksum { expected, got })
    }
}

/// Pulls the moon binary out of the downloaded archive.
pub fn unpack(bytes: &[u8], target: &Target) -> Result<Vec<u8>, UpdateError> {
    let name = target.binary_name();
    let out = if target.is_windows() {
        unzip(bytes, &name)?
    } else {
        untar_gz(bytes, &name)?
    };
    match out {
        Some(b) if !b.is_empty() => Ok(b),
        _ => Err(UpdateError::Archive(target.asset_name())),
    }
}

fn untar_gz(bytes: &[u8], name: &str) -> Result<Option<Vec<u8>>, UpdateError> {
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);
    let entries = archive
        .entries()
        .map_err(|source| io_err("<archive>", source))?;
    for entry in entries {
        let mut entry = entry.map_err(|source| io_err("<archive>", source))?;
        let path = entry.path().map_err(|source| io_err("<archive>", source))?;
        if path.file_name().is_some_and(|f| f == name) {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|source| io_err(name, source))?;
            return Ok(Some(buf));
        }
    }
    Ok(None)
}

#[cfg(windows)]
fn unzip(bytes: &[u8], name: &str) -> Result<Option<Vec<u8>>, UpdateError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| UpdateError::Archive(e.to_string()))?;
    for i in 0..zip.len() {
        let mut f = zip
            .by_index(i)
            .map_err(|e| UpdateError::Archive(e.to_string()))?;
        let matches = std::path::Path::new(f.name())
            .file_name()
            .is_some_and(|n| n == name);
        if matches {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)
                .map_err(|source| io_err(name, source))?;
            return Ok(Some(buf));
        }
    }
    Ok(None)
}

#[cfg(not(windows))]
fn unzip(_bytes: &[u8], _name: &str) -> Result<Option<Vec<u8>>, UpdateError> {
    Err(UpdateError::Unsupported("zip archives".into()))
}

fn io_err(path: &str, source: std::io::Error) -> UpdateError {
    UpdateError::Io {
        path: PathBuf::from(path),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(notes: &str) -> Release {
        Release {
            version: "0.2.0".parse().unwrap(),
            tag: "v0.2.0".into(),
            notes: notes.into(),
            url: String::new(),
            published_at: None,
            assets: vec![
                Asset {
                    name: "moon-macos-aarch64.tar.gz".into(),
                    url: "https://example/moon-macos-aarch64.tar.gz".into(),
                    size: 10,
                },
                Asset {
                    name: CHECKSUMS.into(),
                    url: "https://example/checksums.txt".into(),
                    size: 1,
                },
            ],
        }
    }

    #[test]
    fn the_platform_asset_and_the_missing_one() {
        let r = release("");
        let mac = Target::new("macos", "aarch64", false);
        assert_eq!(r.asset_for(&mac).unwrap().size, 10);
        let win = Target::new("windows", "x86_64", false);
        assert!(matches!(
            r.asset_for(&win),
            Err(UpdateError::NoAsset { .. })
        ));
        assert_eq!(r.checksums_url(), Some("https://example/checksums.txt"));
    }

    #[test]
    fn the_notes_are_summarized_into_bullets() {
        let notes = "## What's Changed\n\
             * feat: model picker by @towerforge in https://github.com/x/pull/3\n\
             - fix: narrow terminals\n\
             \n\
             **Full Changelog**: https://github.com/x/compare/v0.1.1...v0.2.0\n";
        assert_eq!(
            release(notes).highlights(5),
            vec!["feat: model picker", "fix: narrow terminals"]
        );
        assert_eq!(release(notes).highlights(1).len(), 1);
        assert!(release("").highlights(5).is_empty());
    }

    #[test]
    fn the_checksum_matches_or_nothing_installs() {
        let data = b"moon";
        let hash = format!("{:x}", Sha256::digest(data));
        let file = format!("{hash}  moon-macos-aarch64.tar.gz\nother  moon-linux-x86_64.tar.gz\n");
        assert!(verify_sha256(data, &file, "moon-macos-aarch64.tar.gz").unwrap());
        // no line for this asset: nothing to check against, and that is not an error
        assert!(!verify_sha256(data, &file, "moon-windows-x86_64.zip").unwrap());
        let wrong = format!("{}  moon-macos-aarch64.tar.gz\n", "0".repeat(64));
        assert!(matches!(
            verify_sha256(data, &wrong, "moon-macos-aarch64.tar.gz"),
            Err(UpdateError::Checksum { .. })
        ));
    }

    #[test]
    fn it_pulls_the_binary_out_of_the_tar_gz() {
        let mut tar = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, "moon", &b"ELF!"[..]).unwrap();
        let raw = tar.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &raw).unwrap();
        let archive = gz.finish().unwrap();

        let t = Target::new("linux", "x86_64", false);
        assert_eq!(unpack(&archive, &t).unwrap(), b"ELF!");
    }

    // the zip path only exists on Windows, and so does the test that covers
    // it; it was checked against this same `zip` on macOS before being gated
    #[cfg(windows)]
    #[test]
    fn it_pulls_the_exe_out_of_the_zip() {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        w.start_file("moon.exe", opts).unwrap();
        std::io::Write::write_all(&mut w, b"MZ!").unwrap();
        let archive = w.finish().unwrap().into_inner();

        let t = Target::new("windows", "x86_64", false);
        assert_eq!(unpack(&archive, &t).unwrap(), b"MZ!");
        // the tar.gz path does not answer for a zip
        let linux = Target::new("linux", "x86_64", false);
        assert!(unpack(&archive, &linux).is_err());
    }

    #[test]
    fn an_archive_with_no_binary_does_not_pass() {
        let mut tar = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_cksum();
        tar.append_data(&mut header, "README", &b"hey"[..]).unwrap();
        let raw = tar.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &raw).unwrap();
        let archive = gz.finish().unwrap();

        let t = Target::new("linux", "x86_64", false);
        assert!(matches!(unpack(&archive, &t), Err(UpdateError::Archive(_))));
    }
}
