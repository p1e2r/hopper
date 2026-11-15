//! One-click updates from GitHub Releases.
//!
//! Release builds are stamped with their tag (`HOPPER_RELEASE_TAG`, set by CI).
//! We compare it with the latest release, download this platform's file,
//! check its SHA-256 against GitHub's, swap it in place of the running app and
//! let GPUI restart us. Builds made from source have no tag and never update.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const LATEST: &str = "https://api.github.com/repos/p1e2r/hopper/releases/latest";
pub const DOWNLOAD_PAGE: &str = "https://p1e2r.github.io/hopper/";

/// The release this binary was built for, if it is a release build.
pub fn current_tag() -> Option<&'static str> {
    option_env!("HOPPER_RELEASE_TAG").filter(|t| !t.is_empty())
}

/// File name of this platform's download in each release.
pub fn asset_name() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("Hopper-macos-apple-silicon.dmg")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("Hopper-macos-intel.dmg")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("Hopper-windows-x64.exe")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("Hopper-linux-x64.tar.gz")
    } else {
        None
    }
}

/// `v0.3.0` → (0,3,0,0), `v0.3.0-build.7` → (0,3,0,7).
pub fn parse_tag(tag: &str) -> Option<(u64, u64, u64, u64)> {
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    let (version, build) = match tag.split_once("-build.") {
        Some((v, b)) => (v, b.parse().ok()?),
        None => (tag, 0),
    };
    let mut parts = version.split('.').map(|p| p.parse::<u64>());
    let (a, b, c) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    if parts.next().is_some() {
        return None;
    }
    Some((a, b, c, build))
}

#[derive(Clone, Debug)]
pub struct Update {
    pub tag: String,
    url: String,
    sha256: Option<String>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    /// "sha256:<hex>" on current GitHub; absent on older releases.
    digest: Option<String>,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent(concat!("hopper-mod-manager/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(120))
        .build()
}

/// Ask GitHub for the latest release. `Ok(None)` means we're up to date (or
/// this isn't a release build, or there's no download for this platform).
pub fn check() -> Result<Option<Update>, String> {
    let (Some(current), Some(asset)) = (current_tag(), asset_name()) else {
        return Ok(None);
    };
    let latest: GhRelease = agent()
        .get(LATEST)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    let newer = match (parse_tag(&latest.tag_name), parse_tag(current)) {
        (Some(new), Some(cur)) => new > cur,
        _ => latest.tag_name != current,
    };
    if !newer {
        return Ok(None);
    }
    let Some(file) = latest.assets.into_iter().find(|a| a.name == asset) else {
        return Ok(None);
    };
    Ok(Some(Update {
        tag: latest.tag_name,
        url: file.browser_download_url,
        sha256: file
            .digest
            .and_then(|d| d.strip_prefix("sha256:").map(str::to_lowercase)),
    }))
}

/// Download and install `update`. Returns the path GPUI should relaunch.
pub fn install(update: &Update) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let work = std::env::temp_dir().join(format!("hopper-update-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let file = work.join(asset_name().ok_or("no download for this system")?);
    download(update, &file)?;
    let result = replace(&exe, &file, &work);
    let _ = fs::remove_dir_all(&work);
    result.map_err(|e| {
        if e.contains("ermission denied") || e.contains("Access is denied") {
            "Hopper can't replace itself in its current folder. Download the update from the website instead.".into()
        } else {
            e
        }
    })
}

fn download(update: &Update, dest: &Path) -> Result<(), String> {
    let mut reader = agent()
        .get(&update.url)
        .call()
        .map_err(|e| e.to_string())?
        .into_reader();
    let mut out = fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
    }
    if let Some(want) = &update.sha256 {
        let got: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if &got != want {
            return Err("the download was corrupted, try again".into());
        }
    }
    Ok(())
}

/// Remove leftovers from a previous update (Windows can't delete a running exe).
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = fs::remove_file(old_path(&exe));
    }
}

fn old_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    path.with_file_name(name)
}

#[cfg(target_os = "windows")]
fn replace(exe: &Path, new: &Path, _work: &Path) -> Result<PathBuf, String> {
    // A running exe can be renamed but not overwritten.
    let old = old_path(exe);
    let _ = fs::remove_file(&old);
    fs::rename(exe, &old).map_err(|e| e.to_string())?;
    if let Err(e) = fs::copy(new, exe) {
        let _ = fs::rename(&old, exe);
        return Err(e.to_string());
    }
    Ok(exe.to_owned())
}

#[cfg(target_os = "linux")]
fn replace(exe: &Path, new: &Path, work: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    run(
        "tar",
        &[
            "-xzf".as_ref(),
            new.as_os_str(),
            "-C".as_ref(),
            work.as_os_str(),
        ],
    )?;
    let binary = work.join("hopper").join("hopper");
    // Write next to the exe, then rename over it (allowed while running).
    let staged = exe.with_file_name(".hopper-update");
    fs::copy(&binary, &staged).map_err(|e| e.to_string())?;
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    fs::rename(&staged, exe).map_err(|e| {
        let _ = fs::remove_file(&staged);
        e.to_string()
    })?;
    Ok(exe.to_owned())
}

#[cfg(target_os = "macos")]
fn replace(exe: &Path, new: &Path, work: &Path) -> Result<PathBuf, String> {
    // .../Hopper.app/Contents/MacOS/hopper → .../Hopper.app
    let bundle = exe
        .ancestors()
        .nth(3)
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .ok_or("Hopper isn't running from an app bundle")?
        .to_owned();
    let mount = work.join("mnt");
    fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    run(
        "hdiutil",
        &[
            "attach".as_ref(),
            "-nobrowse".as_ref(),
            "-readonly".as_ref(),
            "-mountpoint".as_ref(),
            mount.as_os_str(),
            new.as_os_str(),
        ],
    )?;
    let result = (|| {
        let parent = bundle.parent().ok_or("bad app path")?;
        let staged = parent.join(".Hopper-update.app");
        let old = parent.join(".Hopper-old.app");
        let _ = fs::remove_dir_all(&staged);
        let _ = fs::remove_dir_all(&old);
        run(
            "ditto",
            &[mount.join("Hopper.app").as_os_str(), staged.as_os_str()],
        )?;
        fs::rename(&bundle, &old).map_err(|e| e.to_string())?;
        if let Err(e) = fs::rename(&staged, &bundle) {
            let _ = fs::rename(&old, &bundle);
            return Err(e.to_string());
        }
        let _ = fs::remove_dir_all(&old);
        Ok(bundle.clone())
    })();
    let _ = run(
        "hdiutil",
        &["detach".as_ref(), "-quiet".as_ref(), mount.as_os_str()],
    );
    result
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn replace(_exe: &Path, _new: &Path, _work: &Path) -> Result<PathBuf, String> {
    Err("updates aren't supported on this system".into())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run(cmd: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let out = std::process::Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{cmd} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_order_correctly() {
        assert_eq!(parse_tag("v0.3.0"), Some((0, 3, 0, 0)));
        assert_eq!(parse_tag("v0.3.0-build.7"), Some((0, 3, 0, 7)));
        assert!(parse_tag("v0.3.0-build.7") > parse_tag("v0.3.0-build.5"));
        assert!(parse_tag("v0.3.0-build.5") > parse_tag("v0.3.0"));
        assert!(parse_tag("v0.4.0") > parse_tag("v0.3.0-build.99"));
        assert_eq!(parse_tag("nightly"), None);
    }

    #[test]
    fn parses_github_release() {
        let r: GhRelease = serde_json::from_str(
            r#"{"tag_name":"v0.3.0-build.5","assets":[{"name":"Hopper-windows-x64.exe","browser_download_url":"https://github.com/x","digest":"sha256:ABC"},{"name":"Hopper-linux-x64.tar.gz","browser_download_url":"https://github.com/y"}]}"#,
        )
        .unwrap();
        assert_eq!(r.assets.len(), 2);
        assert_eq!(r.assets[0].digest.as_deref(), Some("sha256:ABC"));
        assert!(r.assets[1].digest.is_none());
    }

    #[test]
    fn old_path_appends_suffix() {
        assert_eq!(
            old_path(Path::new("/a/Hopper.exe")),
            PathBuf::from("/a/Hopper.exe.old")
        );
    }
}

#[cfg(test)]
mod live {
    /// Install a real release file into a throwaway path:
    /// `HOPPER_TEST_ASSET=/path/to/Hopper-linux-x64.tar.gz cargo test -- --ignored live_install`
    #[test]
    #[ignore]
    fn live_install() {
        let file = std::path::PathBuf::from(
            std::env::var("HOPPER_TEST_ASSET").expect("set HOPPER_TEST_ASSET"),
        );
        let work = std::env::temp_dir().join("hopper-live");
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(work.join("dl")).unwrap();
        let fake_exe = work.join("hopper-installed");
        std::fs::write(&fake_exe, b"old").unwrap();
        let path = super::replace(&fake_exe, &file, &work.join("dl")).expect("replace");
        let size = std::fs::metadata(&path).unwrap().len();
        eprintln!("installed {size} bytes at {}", path.display());
        assert!(size > 1_000_000);
    }
}
