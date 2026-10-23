use crate::syntax::Document;
use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};
use tower_lsp::lsp_types::Url;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct CompletionConfig {
    pub connections: String,
    pub parameter_overrides: String,
}
impl Default for CompletionConfig {
    fn default() -> Self {
        Self {
            connections: "placeholders".into(),
            parameter_overrides: "omit".into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub source_dirs: Vec<String>,
    pub include_dirs: Vec<String>,
    pub library_dirs: Vec<String>,
    pub exclude: Vec<String>,
    pub extensions: Vec<String>,
    pub defines: BTreeMap<String, String>,
    pub completion: CompletionConfig,
    pub max_files: usize,
    pub max_file_bytes: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            source_dirs: vec![".".into()],
            include_dirs: vec![],
            library_dirs: vec![],
            exclude: vec![],
            extensions: vec!["va".into(), "vams".into()],
            defines: BTreeMap::new(),
            completion: CompletionConfig::default(),
            max_files: 10000,
            max_file_bytes: 2 * 1024 * 1024,
        }
    }
}

pub fn absolute(path: &Path, base: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    };
    if let Ok(real) = fs::canonicalize(&joined) {
        return real;
    }
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            c => normalized.push(c),
        }
    }
    normalized
}
pub fn find_root(path: &Path) -> (PathBuf, String) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let path = absolute(path, &cwd);
    let dir = if path.is_dir() {
        path.as_path()
    } else {
        path.parent().unwrap_or(&path)
    };
    for parent in dir.ancestors() {
        if parent.join(".vams.json").is_file() {
            return (parent.to_owned(), ".vams.json".into());
        }
        if parent.join(".git").exists() {
            return (parent.to_owned(), "git".into());
        }
    }
    (dir.to_owned(), "file-directory".into())
}

#[derive(Clone)]
pub struct Project {
    pub root: PathBuf,
    pub reason: String,
    pub config: Config,
    pub config_error: Option<String>,
}
#[derive(Clone)]
pub struct Cached {
    pub modified: Option<SystemTime>,
    pub size: u64,
    pub document: Arc<Document>,
}
pub struct Scan {
    pub project: Project,
    pub files: BTreeMap<PathBuf, Cached>,
    pub warnings: Vec<String>,
}

impl Project {
    pub fn new(root: PathBuf, reason: String) -> Self {
        let reason = if reason == "client"
            && !root.join(".vams.json").exists()
            && !root.join(".git").exists()
        {
            "file-directory".to_owned()
        } else {
            reason
        };
        let mut p = Self {
            root,
            reason,
            config: Config::default(),
            config_error: None,
        };
        p.reload();
        p
    }
    pub fn reload(&mut self) -> bool {
        let file = self.root.join(".vams.json");
        if !file.exists() {
            let changed = self.config != Config::default();
            self.config = Config::default();
            self.config_error = None;
            return changed;
        }
        let parsed = (|| -> Result<Config, String> {
            let raw = fs::read_to_string(&file).map_err(|e| e.to_string())?;
            let cfg: Config = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            if cfg.version != 1 {
                return Err("Only configuration version 1 is supported".into());
            }
            if cfg.max_files == 0 || cfg.max_file_bytes == 0 {
                return Err("File limits must be positive".into());
            }
            if !["placeholders", "empty"].contains(&cfg.completion.connections.as_str()) {
                return Err("completion.connections must be placeholders or empty".into());
            }
            if !["omit", "defaults"].contains(&cfg.completion.parameter_overrides.as_str()) {
                return Err("completion.parameter_overrides must be omit or defaults".into());
            }
            for pattern in &cfg.exclude {
                Glob::new(pattern).map_err(|e| e.to_string())?;
            }
            Ok(cfg)
        })();
        match parsed {
            Ok(config) => {
                let changed = self.config != config;
                self.config = config;
                self.config_error = None;
                changed
            }
            Err(e) => {
                self.config_error = Some(format!(
                    "{}: {e}; using last valid configuration",
                    file.display()
                ));
                false
            }
        }
    }
    pub fn resolve_include(&self, source: &Path, name: &str) -> Option<PathBuf> {
        let path = Path::new(name);
        let mut bases = vec![
            source.parent().unwrap_or(&self.root).to_owned(),
            self.root.clone(),
        ];
        bases.extend(
            self.config
                .include_dirs
                .iter()
                .map(|p| absolute(Path::new(p), &self.root)),
        );
        bases
            .into_iter()
            .map(|base| absolute(path, &base))
            .find(|p| p.is_file())
    }
    pub fn info(&self) -> serde_json::Value {
        serde_json::json!({"root":self.root,"reason":self.reason,"configuration":self.root.join(".vams.json"),
        "source_dirs":self.config.source_dirs.iter().map(|p|absolute(Path::new(p),&self.root)).collect::<Vec<_>>(),
        "library_dirs":self.config.library_dirs.iter().map(|p|absolute(Path::new(p),&self.root)).collect::<Vec<_>>(),
        "include_dirs":self.config.include_dirs.iter().map(|p|absolute(Path::new(p),&self.root)).collect::<Vec<_>>(),
        "configuration_error":self.config_error,"limits":{"files":self.config.max_files,"file_bytes":self.config.max_file_bytes},
        "preprocessing":"literal includes and line directives; no token macro expansion or per-translation-unit header macro state"})
    }
    pub fn scan(mut self, cache: BTreeMap<PathBuf, Cached>) -> Scan {
        let changed = self.reload();
        let mut files = BTreeMap::new();
        let mut warnings = Vec::new();
        let mut builder = GlobSetBuilder::new();
        for pattern in &self.config.exclude {
            if let Ok(glob) = Glob::new(pattern) {
                builder.add(glob);
            }
            if !pattern.contains('*') {
                if let Ok(glob) = Glob::new(&format!("{pattern}/**")) {
                    builder.add(glob);
                }
            }
        }
        let excludes = builder.build().unwrap_or_default();
        let mut pending = VecDeque::new();
        let mut discovered = BTreeSet::new();
        let roots: Vec<_> = self
            .config
            .source_dirs
            .iter()
            .chain(&self.config.library_dirs)
            .map(|p| absolute(Path::new(p), &self.root))
            .collect();
        for root in roots {
            if !root.exists() {
                warnings.push(format!("Source directory not found: {}", root.display()));
                continue;
            }
            let project_root = self.root.clone();
            let scan_root = root.clone();
            let excludes = excludes.clone();
            let mut walk = WalkBuilder::new(&root);
            // A loose file must not trigger a recursive scan of an unmarked
            // home directory. A {} project marker opts into recursive discovery.
            if self.reason == "file-directory" {
                walk.max_depth(Some(1));
            }
            walk.hidden(false)
                .follow_links(false)
                .require_git(false)
                .filter_entry(move |entry| {
                    if entry.depth() == 0 {
                        return true;
                    }
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy();
                    if entry.file_type().is_some_and(|t| t.is_dir()) {
                        if [
                            ".git",
                            "build",
                            "target",
                            "dist",
                            "simulation",
                            "node_modules",
                            "xcelium.d",
                            "INCA_libs",
                            ".test-work",
                        ]
                        .contains(&name.as_ref())
                        {
                            return false;
                        }
                        if path != scan_root
                            && (path.join(".vams.json").exists() || path.join(".git").exists())
                        {
                            return false;
                        }
                    }
                    !excludes.is_match(path.strip_prefix(&project_root).unwrap_or(path))
                });
            for entry in walk.build() {
                match entry {
                    Ok(e) => {
                        if e.file_type().is_some_and(|t| t.is_file())
                            && e.path()
                                .extension()
                                .and_then(|s| s.to_str())
                                .is_some_and(|s| self.config.extensions.iter().any(|x| x == s))
                        {
                            let path = absolute(e.path(), &self.root);
                            if discovered.insert(path.clone()) {
                                pending.push_back(path);
                            }
                        }
                        if pending.len() >= self.config.max_files {
                            warnings.push("Source discovery file limit reached".into());
                            break;
                        }
                    }
                    Err(e) => warnings.push(e.to_string()),
                }
            }
        }
        while let Some(path) = pending.pop_front() {
            if files.len() >= self.config.max_files {
                warnings.push("Index file limit reached".into());
                break;
            }
            let metadata = match fs::metadata(&path) {
                Ok(m) => m,
                Err(e) => {
                    warnings.push(format!("{}: {e}", path.display()));
                    continue;
                }
            };
            if metadata.len() > self.config.max_file_bytes {
                warnings.push(format!("File too large: {}", path.display()));
                continue;
            }
            let modified = metadata.modified().ok();
            let size = metadata.len();
            let cached = cache
                .get(&path)
                .filter(|c| !changed && c.modified == modified && c.size == size)
                .cloned();
            let entry = if let Some(cached) = cached {
                cached
            } else {
                let raw = match fs::read_to_string(&path) {
                    Ok(s) if !s.contains('\0') => s,
                    Ok(_) => {
                        warnings.push(format!("Binary file skipped: {}", path.display()));
                        continue;
                    }
                    Err(e) => {
                        warnings.push(format!("{}: {e}", path.display()));
                        continue;
                    }
                };
                let Ok(uri) = Url::from_file_path(&path) else {
                    continue;
                };
                match Document::parse(uri, raw, None, &self.config.defines) {
                    Ok(doc) => Cached {
                        modified,
                        size,
                        document: Arc::new(doc),
                    },
                    Err(e) => {
                        warnings.push(e);
                        continue;
                    }
                }
            };
            for include in &entry.document.includes {
                if let Some(header) = self.resolve_include(&path, &include.name) {
                    if discovered.insert(header.clone()) {
                        pending.push_back(header);
                    }
                } else {
                    warnings.push(format!(
                        "{}: unresolved include {}",
                        path.display(),
                        include.name
                    ));
                }
            }
            files.insert(path, entry);
        }
        warnings.sort();
        warnings.dedup();
        warnings.truncate(100);
        Scan {
            project: self,
            files,
            warnings,
        }
    }
}
