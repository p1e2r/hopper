//! Minimal Modrinth API v2 client: search, pick a compatible version, download.

use crate::mc::Kind;
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const API: &str = "https://api.modrinth.com/v2";
// Modrinth asks every client to identify itself.
const USER_AGENT: &str = concat!("hopper-mod-manager/", env!("CARGO_PKG_VERSION"));
pub const PAGE: usize = 20;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent(USER_AGENT)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build()
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Deserialize, Clone, Debug)]
pub struct Hit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub downloads: u64,
    pub project_type: String,
}

#[derive(Deserialize, Debug)]
pub struct SearchResult {
    pub hits: Vec<Hit>,
    pub total_hits: usize,
}

#[derive(Deserialize, Debug)]
struct Version {
    files: Vec<VFile>,
    #[serde(default)]
    dependencies: Vec<Dep>,
}

#[derive(Deserialize, Debug)]
struct VFile {
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    hashes: Hashes,
}

#[derive(Deserialize, Debug, Default)]
struct Hashes {
    sha1: Option<String>,
}

#[derive(Deserialize, Debug)]
struct Dep {
    project_id: Option<String>,
    version_id: Option<String>,
    dependency_type: String,
}

/// The version and loader picked in the top bar. Every search and every
/// download (dependencies included) is restricted to this.
#[derive(Clone, Debug, Default)]
pub struct Target {
    pub mc_version: String,
    pub loader: String,
}

impl Target {
    /// Quilt can load Fabric mods, so accept both there.
    fn loaders(&self) -> Vec<&str> {
        match self.loader.as_str() {
            "" => vec![],
            "quilt" => vec!["quilt", "fabric"],
            l => vec![l],
        }
    }
}

#[derive(Deserialize)]
struct GameVersion {
    version: String,
    version_type: String,
}

/// Every Minecraft release Modrinth knows about, newest first.
pub fn game_versions() -> Result<Vec<String>, String> {
    let all: Vec<GameVersion> = agent()
        .get(&format!("{API}/tag/game_version"))
        .call()
        .map_err(err)?
        .into_json()
        .map_err(err)?;
    Ok(all
        .into_iter()
        .filter(|v| v.version_type == "release")
        .map(|v| v.version)
        .collect())
}

/// A file on disk that Modrinth recognised.
#[derive(Clone, Debug)]
pub struct Identified {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub project_type: String,
    pub version_number: String,
}

#[derive(Deserialize)]
struct HashVersion {
    project_id: String,
    #[serde(default)]
    version_number: String,
}

#[derive(Deserialize)]
struct Project {
    id: String,
    slug: String,
    title: String,
    project_type: String,
}

fn sha1_of(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

/// Look files up on Modrinth by their SHA-1. Only exact, unmodified files that
/// were published on Modrinth match; everything else is simply left out.
pub fn identify(files: &[PathBuf]) -> Result<HashMap<PathBuf, Identified>, String> {
    let hashed: Vec<(String, &PathBuf)> = files
        .iter()
        .filter_map(|p| Some((sha1_of(p)?, p)))
        .collect();
    let mut out = HashMap::new();
    if hashed.is_empty() {
        return Ok(out);
    }
    let hashes: Vec<&str> = hashed.iter().map(|(h, _)| h.as_str()).collect();
    let versions: HashMap<String, HashVersion> = agent()
        .post(&format!("{API}/version_files"))
        .send_json(serde_json::json!({ "hashes": hashes, "algorithm": "sha1" }))
        .map_err(err)?
        .into_json()
        .map_err(err)?;

    // The hash lookup only gives project ids; fetch their names in bulk.
    let mut ids: Vec<&str> = versions.values().map(|v| v.project_id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut projects: HashMap<String, Project> = HashMap::new();
    for chunk in ids.chunks(100) {
        let found: Vec<Project> = agent()
            .get(&format!("{API}/projects"))
            .query("ids", &serde_json::to_string(chunk).map_err(err)?)
            .call()
            .map_err(err)?
            .into_json()
            .map_err(err)?;
        projects.extend(found.into_iter().map(|p| (p.id.clone(), p)));
    }
    for (hash, path) in hashed {
        let Some(version) = versions.get(&hash) else {
            continue;
        };
        let Some(project) = projects.get(&version.project_id) else {
            continue;
        };
        out.insert(
            path.clone(),
            Identified {
                project_id: project.id.clone(),
                slug: project.slug.clone(),
                title: project.title.clone(),
                project_type: project.project_type.clone(),
                version_number: version.version_number.clone(),
            },
        );
    }
    Ok(out)
}

pub fn search(
    query: &str,
    kind: Kind,
    target: Option<&Target>,
    offset: usize,
) -> Result<SearchResult, String> {
    let mut facets: Vec<Vec<String>> = vec![vec![format!("project_type:{}", kind.modrinth_type())]];
    if let Some(t) = target {
        if !t.mc_version.is_empty() {
            facets.push(vec![format!("versions:{}", t.mc_version)]);
        }
        if kind == Kind::Mod && !t.loaders().is_empty() {
            facets.push(
                t.loaders()
                    .iter()
                    .map(|l| format!("categories:{l}"))
                    .collect(),
            );
        }
    }
    agent()
        .get(&format!("{API}/search"))
        .query("query", query)
        .query("facets", &serde_json::to_string(&facets).map_err(err)?)
        .query("limit", &PAGE.to_string())
        .query("offset", &offset.to_string())
        .query(
            "index",
            if query.trim().is_empty() {
                "downloads"
            } else {
                "relevance"
            },
        )
        .call()
        .map_err(err)?
        .into_json()
        .map_err(err)
}

fn best_version(project: &str, kind: Kind, target: Option<&Target>) -> Result<Version, String> {
    let mut req = agent().get(&format!("{API}/project/{project}/version"));
    if let Some(t) = target {
        if !t.mc_version.is_empty() {
            req = req.query(
                "game_versions",
                &serde_json::to_string(&[&t.mc_version]).map_err(err)?,
            );
        }
        if kind == Kind::Mod && !t.loaders().is_empty() {
            req = req.query(
                "loaders",
                &serde_json::to_string(&t.loaders()).map_err(err)?,
            );
        }
    }
    // Modrinth returns newest first.
    let versions: Vec<Version> = req.call().map_err(err)?.into_json().map_err(err)?;
    versions
        .into_iter()
        .find(|v| !v.files.is_empty())
        .ok_or_else(|| match target {
            Some(t) if kind == Kind::Mod => {
                format!("no version for Minecraft {} ({})", t.mc_version, t.loader)
            }
            Some(t) => format!("no version for Minecraft {}", t.mc_version),
            None => "no downloadable files".to_owned(),
        })
}

fn download(file: &VFile, dir: &Path) -> Result<Option<String>, String> {
    // Never trust a remote file name as a path.
    let name = Path::new(&file.filename)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("bad file name from Modrinth")?
        .to_owned();
    fs::create_dir_all(dir).map_err(err)?;
    let dest = dir.join(&name);
    if dest.exists() || dir.join(format!("{name}.disabled")).exists() {
        return Ok(None); // already there
    }
    let part = dir.join(format!(".{name}.part"));
    let mut reader = agent().get(&file.url).call().map_err(err)?.into_reader();
    let mut out = fs::File::create(&part).map_err(err)?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    let result = (|| loop {
        let n = reader.read(&mut buf).map_err(err)?;
        if n == 0 {
            return Ok::<(), String>(());
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(err)?;
    })();
    drop(out);
    let checked = result.and_then(|_| {
        let got: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        match &file.hashes.sha1 {
            Some(want) if !want.eq_ignore_ascii_case(&got) => {
                Err(format!("{name}: download was corrupted, try again"))
            }
            _ => Ok(()),
        }
    });
    if let Err(e) = checked {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    fs::rename(&part, &dest).map_err(err)?;
    Ok(Some(name))
}

/// Install a project into the Minecraft folder. For mods, required dependencies
/// come along too. Returns the names of the files that were actually written.
pub fn install(
    root: &Path,
    project: &str,
    kind: Kind,
    target: Option<&Target>,
) -> Result<Vec<String>, String> {
    let mut written = Vec::new();
    let mut seen = HashSet::new();
    install_inner(root, project, None, kind, target, &mut seen, &mut written)?;
    Ok(written)
}

fn install_inner(
    root: &Path,
    project: &str,
    version_id: Option<&str>,
    kind: Kind,
    target: Option<&Target>,
    seen: &mut HashSet<String>,
    written: &mut Vec<String>,
) -> Result<(), String> {
    if !seen.insert(project.to_owned()) {
        return Ok(());
    }
    let version = match version_id {
        Some(id) => agent()
            .get(&format!("{API}/version/{id}"))
            .call()
            .map_err(err)?
            .into_json()
            .map_err(err)?,
        None => best_version(project, kind, target)?,
    };
    let file = version
        .files
        .iter()
        .find(|f| f.primary)
        .or(version.files.first())
        .ok_or("no files")?;
    if let Some(name) = download(file, &root.join(kind.dir()))? {
        written.push(name);
    }
    if kind == Kind::Mod {
        for dep in version
            .dependencies
            .iter()
            .filter(|d| d.dependency_type == "required")
        {
            let Some(dep_project) = &dep.project_id else {
                continue;
            };
            // A missing dependency shouldn't undo the mod the user asked for.
            if let Err(e) = install_inner(
                root,
                dep_project,
                dep.version_id.as_deref(),
                kind,
                target,
                seen,
                written,
            ) {
                written.push(format!("(dependency {dep_project} failed: {e})"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_api_shapes() {
        let s: SearchResult = serde_json::from_str(
            r#"{"hits":[{"project_id":"AANobbMI","project_type":"mod","slug":"sodium","author":"jellysquid3","title":"Sodium","description":"d","categories":["fabric"],"versions":["1.21.1"],"downloads":123,"follows":1,"icon_url":null,"latest_version":"x","license":"x","client_side":"required","server_side":"unsupported","gallery":[],"color":1}],"offset":0,"limit":1,"total_hits":42}"#,
        )
        .unwrap();
        assert_eq!(s.total_hits, 42);
        assert_eq!(s.hits[0].slug, "sodium");

        let v: Vec<Version> = serde_json::from_str(
            r#"[{"id":"abc","project_id":"p","name":"n","version_number":"1","game_versions":["1.21.1"],"loaders":["fabric"],"files":[{"hashes":{"sha1":"aa","sha512":"bb"},"url":"https://cdn.modrinth.com/x.jar","filename":"x.jar","primary":true,"size":1,"file_type":null}],"dependencies":[{"version_id":null,"project_id":"P7dR8mSH","file_name":null,"dependency_type":"required"}]}]"#,
        )
        .unwrap();
        assert_eq!(v[0].dependencies[0].project_id.as_deref(), Some("P7dR8mSH"));
        assert!(v[0].files[0].primary);
    }

    #[test]
    fn parses_identify_shapes() {
        let v: HashMap<String, HashVersion> = serde_json::from_str(
            r#"{"619e250c133106bacc3e3b560839bd4b324dfda8":{"id":"x","project_id":"AANobbMI","version_number":"mc1.21-0.6.0","name":"n","files":[],"dependencies":[]}}"#,
        )
        .unwrap();
        assert_eq!(
            v["619e250c133106bacc3e3b560839bd4b324dfda8"].project_id,
            "AANobbMI"
        );
        let p: Vec<Project> = serde_json::from_str(
            r#"[{"id":"AANobbMI","slug":"sodium","title":"Sodium","project_type":"mod","description":"d","downloads":1}]"#,
        )
        .unwrap();
        assert_eq!(p[0].title, "Sodium");
        // sha1 of a known string
        let f = std::env::temp_dir().join(format!("hopper-sha-{}", std::process::id()));
        fs::write(&f, "abc").unwrap();
        assert_eq!(
            sha1_of(&f).unwrap(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn quilt_accepts_fabric() {
        let t = Target {
            mc_version: "1.21.1".into(),
            loader: "quilt".into(),
        };
        assert_eq!(t.loaders(), vec!["quilt", "fabric"]);
    }
}
