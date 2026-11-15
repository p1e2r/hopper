//! Everything that touches the Minecraft folder on disk.

use serde::Deserialize;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const DISABLED: &str = ".disabled";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Mod = 0,
    ResourcePack = 1,
    Shader = 2,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Mod, Kind::ResourcePack, Kind::Shader];

    pub fn dir(self) -> &'static str {
        match self {
            Kind::Mod => "mods",
            Kind::ResourcePack => "resourcepacks",
            Kind::Shader => "shaderpacks",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Kind::Mod => "Mods",
            Kind::ResourcePack => "Resource Packs",
            Kind::Shader => "Shaders",
        }
    }
    /// Modrinth `project_type`.
    pub fn modrinth_type(self) -> &'static str {
        match self {
            Kind::Mod => "mod",
            Kind::ResourcePack => "resourcepack",
            Kind::Shader => "shader",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Kind::Mod => "jar",
            _ => "zip",
        }
    }
}

/// The standard Minecraft folder for this OS, if it exists.
pub fn default_dir() -> Option<PathBuf> {
    let dir = if cfg!(target_os = "macos") {
        dirs::home_dir()?.join("Library/Application Support/minecraft")
    } else if cfg!(target_os = "windows") {
        dirs::config_dir()?.join(".minecraft")
    } else {
        dirs::home_dir()?.join(".minecraft")
    };
    dir.is_dir().then_some(dir)
}

/// Loose check so people can't attach to a random folder by accident.
pub fn looks_like_minecraft(dir: &Path) -> bool {
    [
        "versions",
        "mods",
        "options.txt",
        "launcher_profiles.json",
        "saves",
        "resourcepacks",
    ]
    .iter()
    .any(|p| dir.join(p).exists())
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    /// File name without the `.disabled` suffix.
    pub file_name: String,
    /// Friendly name (from mod metadata when available).
    pub name: String,
    pub version: String,
    pub loader: String,
    pub enabled: bool,
    pub size: u64,
    pub is_dir: bool,
}

/// List everything of `kind` in the Minecraft folder, enabled or not.
pub fn scan(root: &Path, kind: Kind) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(root.join(kind.dir())) else {
        return out;
    };
    for de in rd.flatten() {
        let path = de.path();
        let Some(raw) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        if raw.starts_with('.') {
            continue;
        }
        let (file_name, enabled) = match raw.strip_suffix(DISABLED) {
            Some(base) => (base.to_owned(), false),
            None => (raw.clone(), true),
        };
        let Ok(meta) = de.metadata() else { continue };
        let is_dir = meta.is_dir();
        let lower = file_name.to_lowercase();
        let wanted = match kind {
            Kind::Mod => !is_dir && lower.ends_with(".jar"),
            Kind::ResourcePack => {
                (!is_dir && lower.ends_with(".zip"))
                    || (is_dir && path.join("pack.mcmeta").exists())
            }
            Kind::Shader => {
                (!is_dir && lower.ends_with(".zip")) || (is_dir && path.join("shaders").is_dir())
            }
        };
        if !wanted {
            continue;
        }
        let mut entry = Entry {
            name: file_name
                .trim_end_matches(".jar")
                .trim_end_matches(".zip")
                .to_owned(),
            file_name,
            version: String::new(),
            loader: String::new(),
            enabled,
            size: if is_dir { 0 } else { meta.len() },
            is_dir,
            path,
        };
        if kind == Kind::Mod {
            if let Some(m) = read_mod_meta(&entry.path) {
                if !m.name.is_empty() {
                    entry.name = m.name;
                }
                entry.version = m.version;
                entry.loader = m.loader;
            }
        }
        out.push(entry);
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

/// Flip a mod/pack on or off by renaming it to/from `<name>.disabled`.
/// This is the same convention other launchers use, so they stay compatible.
pub fn set_enabled(path: &Path, enabled: bool) -> io::Result<PathBuf> {
    let raw = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::other("bad file name"))?;
    let new_name = match (raw.strip_suffix(DISABLED), enabled) {
        (Some(base), true) => base.to_owned(),
        (None, false) => format!("{raw}{DISABLED}"),
        _ => return Ok(path.to_owned()),
    };
    let target = path.with_file_name(new_name);
    if target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", target.display()),
        ));
    }
    fs::rename(path, &target)?;
    Ok(target)
}

/// Copy a file the user picked or dropped into the right folder.
pub fn import(root: &Path, kind: Kind, src: &Path) -> io::Result<PathBuf> {
    let name = src
        .file_name()
        .ok_or_else(|| io::Error::other("bad file name"))?;
    let dir = root.join(kind.dir());
    fs::create_dir_all(&dir)?;
    let dest = dir.join(name);
    if dest == src {
        return Ok(dest);
    }
    fs::copy(src, &dest)?;
    Ok(dest)
}

/// Work out what a dropped file is: jars are mods, zips are inspected.
pub fn guess_kind(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        "jar" => Some(Kind::Mod),
        "zip" => {
            let mut zip = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
            if zip.by_name("pack.mcmeta").is_ok() {
                return Some(Kind::ResourcePack);
            }
            let has_shaders = zip.file_names().any(|n| n.starts_with("shaders/"));
            has_shaders.then_some(Kind::Shader)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- mod metadata

pub struct ModMeta {
    pub name: String,
    pub version: String,
    pub loader: String,
}

fn zip_text<R: Read + io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    let mut s = String::new();
    zip.by_name(name).ok()?.read_to_string(&mut s).ok()?;
    Some(s)
}

fn clean_version(v: &str) -> String {
    // Forge jars often leave a build placeholder here.
    if v.contains("${") {
        String::new()
    } else {
        v.to_owned()
    }
}

pub fn read_mod_meta(jar: &Path) -> Option<ModMeta> {
    let mut zip = zip::ZipArchive::new(fs::File::open(jar).ok()?).ok()?;
    let s =
        |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();

    if let Some(text) = zip_text(&mut zip, "fabric.mod.json") {
        // Some mods ship raw newlines inside strings, which strict JSON rejects.
        let text: String = text
            .chars()
            .map(|c| {
                if c == '\n' || c == '\r' || c == '\t' {
                    ' '
                } else {
                    c
                }
            })
            .collect();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            return Some(ModMeta {
                name: s(&v, "name"),
                version: clean_version(&s(&v, "version")),
                loader: "Fabric".into(),
            });
        }
    }
    if let Some(text) = zip_text(&mut zip, "quilt.mod.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let ql = v.get("quilt_loader").cloned().unwrap_or_default();
            let name = ql.get("metadata").map(|m| s(m, "name")).unwrap_or_default();
            return Some(ModMeta {
                name,
                version: clean_version(&s(&ql, "version")),
                loader: "Quilt".into(),
            });
        }
    }
    for (file, loader) in [
        ("META-INF/neoforge.mods.toml", "NeoForge"),
        ("META-INF/mods.toml", "Forge"),
    ] {
        if let Some(text) = zip_text(&mut zip, file) {
            if let Ok(v) = text.parse::<toml::Table>() {
                let first = v
                    .get("mods")
                    .and_then(|m| m.as_array())
                    .and_then(|a| a.first());
                let g = |k: &str| {
                    first
                        .and_then(|m| m.get(k))
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_owned()
                };
                return Some(ModMeta {
                    name: g("displayName"),
                    version: clean_version(&g("version")),
                    loader: loader.into(),
                });
            }
        }
    }
    None
}

// ------------------------------------------------------------ installed versions

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub id: String,
    pub mc_version: String,
    /// "fabric", "quilt", "forge", "neoforge" or "" for vanilla.
    pub loader: String,
}

#[derive(Deserialize)]
struct VersionJson {
    #[serde(rename = "inheritsFrom")]
    inherits_from: Option<String>,
    id: Option<String>,
}

/// Read `versions/` to find which game versions and loaders are installed,
/// most recently touched first.
pub fn detect_profiles(root: &Path) -> Vec<Profile> {
    let mut found: Vec<(std::time::SystemTime, Profile)> = Vec::new();
    let Ok(rd) = fs::read_dir(root.join("versions")) else {
        return Vec::new();
    };
    for de in rd.flatten() {
        let dir = de.path();
        if !dir.is_dir() {
            continue;
        }
        let id = de.file_name().to_string_lossy().into_owned();
        let Ok(text) = fs::read_to_string(dir.join(format!("{id}.json"))) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<VersionJson>(&text) else {
            continue;
        };
        let lower = id.to_lowercase();
        let loader = ["neoforge", "forge", "fabric", "quilt"]
            .into_iter()
            .find(|l| lower.contains(l))
            .unwrap_or("");
        let mc_version = match json.inherits_from {
            Some(v) => v,
            None if loader.is_empty() => json.id.unwrap_or_else(|| id.clone()),
            None => continue, // modded profile that doesn't say what it is built on
        };
        let time = de
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        let p = Profile {
            id,
            mc_version,
            loader: loader.to_owned(),
        };
        if !found
            .iter()
            .any(|(_, o)| o.mc_version == p.mc_version && o.loader == p.loader)
        {
            found.push((time, p));
        }
    }
    // Modded profiles first (that's what a mod manager is for), then newest.
    found.sort_by(|a, b| {
        a.1.loader
            .is_empty()
            .cmp(&b.1.loader.is_empty())
            .then(b.0.cmp(&a.0))
    });
    found.into_iter().map(|(_, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hopper-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_zip(path: &Path, files: &[(&str, &str)]) {
        let mut w = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, body) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn scan_toggle_and_metadata() {
        let root = tmp("scan");
        fs::create_dir_all(root.join("mods")).unwrap();
        make_zip(
            &root.join("mods/sodium-fabric-0.6.jar"),
            &[(
                "fabric.mod.json",
                "{\"id\":\"sodium\",\"name\":\"Sodium\",\"version\":\"0.6.0\",\"description\":\"line\nbreak\"}",
            )],
        );
        make_zip(
            &root.join("mods/jei.jar.disabled"),
            &[(
                "META-INF/neoforge.mods.toml",
                "modLoader=\"javafml\"\n[[mods]]\nmodId=\"jei\"\ndisplayName=\"Just Enough Items\"\nversion=\"${file.jarVersion}\"\n",
            )],
        );
        fs::write(root.join("mods/readme.txt"), "x").unwrap();

        let mods = scan(&root, Kind::Mod);
        assert_eq!(mods.len(), 2);
        assert_eq!(mods[0].name, "Just Enough Items");
        assert!(!mods[0].enabled);
        assert_eq!(mods[0].version, "");
        assert_eq!(mods[0].loader, "NeoForge");
        assert_eq!(mods[1].name, "Sodium");
        assert_eq!(mods[1].version, "0.6.0");
        assert!(mods[1].enabled);

        // switch Sodium off and back on
        let off = set_enabled(&mods[1].path, false).unwrap();
        assert!(off.ends_with("sodium-fabric-0.6.jar.disabled"));
        assert!(!scan(&root, Kind::Mod)[1].enabled);
        let on = set_enabled(&off, true).unwrap();
        assert!(on.ends_with("sodium-fabric-0.6.jar"));
        assert!(
            scan(&root, Kind::Mod)
                .iter()
                .all(|m| m.name != "Sodium" || m.enabled)
        );
    }

    #[test]
    fn guess_and_import() {
        let root = tmp("import");
        let src = tmp("import-src");
        make_zip(&src.join("Faithful.zip"), &[("pack.mcmeta", "{}")]);
        make_zip(&src.join("Complementary.zip"), &[("shaders/final.fsh", "")]);
        make_zip(
            &src.join("lithium.jar"),
            &[("fabric.mod.json", "{\"name\":\"Lithium\"}")],
        );
        make_zip(&src.join("random.zip"), &[("hello.txt", "")]);

        assert_eq!(
            guess_kind(&src.join("Faithful.zip")),
            Some(Kind::ResourcePack)
        );
        assert_eq!(
            guess_kind(&src.join("Complementary.zip")),
            Some(Kind::Shader)
        );
        assert_eq!(guess_kind(&src.join("lithium.jar")), Some(Kind::Mod));
        assert_eq!(guess_kind(&src.join("random.zip")), None);

        for f in ["Faithful.zip", "Complementary.zip", "lithium.jar"] {
            let p = src.join(f);
            import(&root, guess_kind(&p).unwrap(), &p).unwrap();
        }
        assert_eq!(scan(&root, Kind::ResourcePack).len(), 1);
        assert_eq!(scan(&root, Kind::Shader).len(), 1);
        assert_eq!(scan(&root, Kind::Mod)[0].name, "Lithium");
    }

    #[test]
    fn profiles() {
        let root = tmp("profiles");
        for (id, body) in [
            ("1.21.1", "{\"id\":\"1.21.1\"}"),
            (
                "fabric-loader-0.17.2-26.2",
                "{\"id\":\"fabric-loader-0.17.2-26.2\",\"inheritsFrom\":\"26.2\"}",
            ),
            (
                "neoforge-21.1.72",
                "{\"id\":\"neoforge-21.1.72\",\"inheritsFrom\":\"1.21.1\"}",
            ),
        ] {
            fs::create_dir_all(root.join("versions").join(id)).unwrap();
            fs::write(
                root.join("versions").join(id).join(format!("{id}.json")),
                body,
            )
            .unwrap();
        }
        let p = detect_profiles(&root);
        assert_eq!(p.len(), 3);
        assert!(p.contains(&Profile {
            id: "fabric-loader-0.17.2-26.2".into(),
            mc_version: "26.2".into(),
            loader: "fabric".into()
        }));
        assert!(p.contains(&Profile {
            id: "neoforge-21.1.72".into(),
            mc_version: "1.21.1".into(),
            loader: "neoforge".into()
        }));
        assert_eq!(p[2].loader, ""); // vanilla sorts last
        assert!(looks_like_minecraft(&root));
    }
}
