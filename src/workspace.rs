use crate::{
    project::{absolute, Cached, Project, Scan},
    syntax::{Document, Kind},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tower_lsp::lsp_types::Url;

pub struct Workspace {
    pub project: Project,
    pub disk: BTreeMap<PathBuf, Cached>,
    pub open: BTreeMap<Url, Arc<Document>>,
    pub warnings: Vec<String>,
    pub scanning: bool,
    pub generation: u64,
}
pub type Target = (Arc<Document>, usize);
impl Workspace {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            disk: BTreeMap::new(),
            open: BTreeMap::new(),
            warnings: vec![],
            scanning: false,
            generation: 0,
        }
    }
    pub fn files(&self) -> BTreeMap<PathBuf, Arc<Document>> {
        let mut docs: BTreeMap<_, _> = self
            .disk
            .iter()
            .map(|(p, c)| (p.clone(), c.document.clone()))
            .collect();
        for doc in self.open.values() {
            docs.insert(absolute(&doc.path, &self.project.root), doc.clone());
        }
        docs
    }
    pub fn document(&self, uri: &Url) -> Option<Arc<Document>> {
        self.open.get(uri).cloned().or_else(|| {
            uri.to_file_path().ok().and_then(|p| {
                self.disk
                    .get(&absolute(&p, &self.project.root))
                    .map(|c| c.document.clone())
            })
        })
    }
    pub fn update(&mut self, uri: Url, text: String, version: i32) -> bool {
        if self
            .open
            .get(&uri)
            .is_some_and(|d| d.version.is_some_and(|v| v >= version))
        {
            return false;
        }
        match Document::parse(
            uri.clone(),
            text,
            Some(version),
            &self.project.config.defines,
        ) {
            Ok(doc) => {
                self.open.insert(uri, Arc::new(doc));
                true
            }
            Err(e) => {
                self.warnings.push(e);
                false
            }
        }
    }
    pub fn merge(&mut self, scan: Scan) {
        let defines_changed = self.project.config.defines != scan.project.config.defines;
        self.project = scan.project;
        self.disk = scan.files;
        self.warnings = scan.warnings;
        self.scanning = false;
        self.generation += 1;
        if defines_changed {
            self.open = self
                .open
                .iter()
                .filter_map(|(u, d)| {
                    Document::parse(
                        u.clone(),
                        d.text.clone(),
                        d.version,
                        &self.project.config.defines,
                    )
                    .ok()
                    .map(|doc| (u.clone(), Arc::new(doc)))
                })
                .collect();
        }
    }
    pub fn globals(&self, name: &str, kind: Option<Kind>) -> Vec<Target> {
        let mut targets = Vec::new();
        for doc in self.files().into_values() {
            for (id, symbol) in doc.symbols.iter().enumerate() {
                if symbol.scope == 0 && symbol.name == name && kind.is_none_or(|k| symbol.kind == k)
                {
                    targets.push((doc.clone(), id));
                }
            }
        }
        targets
    }
    pub fn local(&self, doc: &Arc<Document>, name: &str, mut scope: usize) -> Option<Target> {
        loop {
            let mut found = doc
                .symbols
                .iter()
                .enumerate()
                .filter(|(_, s)| s.scope == scope && s.name == name);
            let first = found.next();
            if found.next().is_some() {
                return None;
            }
            if let Some((id, _)) = first {
                return Some((doc.clone(), id));
            }
            if let Some(parent) = doc.scopes[scope].parent {
                scope = parent;
            } else {
                return None;
            }
        }
    }
    pub fn resolve(&self, doc: &Arc<Document>, byte: usize) -> Vec<Target> {
        let Some(occurrence) = doc.occurrence(byte) else {
            return vec![];
        };
        if let Some((id, _)) = doc
            .symbols
            .iter()
            .enumerate()
            .find(|(_, s)| s.span == occurrence.span)
        {
            return vec![(doc.clone(), id)];
        }
        for instance in &doc.instances {
            if instance.type_span == occurrence.span {
                return self.globals(&instance.target, Some(Kind::Module));
            }
            let port = instance
                .ports
                .iter()
                .any(|(_, span)| span == &occurrence.span);
            let parameter = instance
                .parameters
                .iter()
                .any(|(_, span)| span == &occurrence.span);
            if port || parameter {
                let mut result = vec![];
                for (target, id) in self.globals(&instance.target, Some(Kind::Module)) {
                    let scope = target.symbols[id].child_scope;
                    for (index, symbol) in target.symbols.iter().enumerate() {
                        if Some(symbol.scope) == scope
                            && symbol.name == occurrence.name
                            && symbol.kind == if port { Kind::Port } else { Kind::Parameter }
                        {
                            result.push((target.clone(), index));
                        }
                    }
                }
                return result;
            }
        }
        if let Some(target) = self.local(doc, &occurrence.name, occurrence.scope) {
            return vec![target];
        }
        self.globals(&occurrence.name, None)
    }
    pub fn info(&self) -> serde_json::Value {
        let mut value = self.project.info();
        value["indexed_files"] = self.disk.len().into();
        value["open_documents"] = self.open.len().into();
        value["indexing"] = self.scanning.into();
        value["generation"] = self.generation.into();
        value["warnings"] = serde_json::json!(self.warnings);
        value
    }
    pub fn is_library(&self, path: &Path) -> bool {
        self.project
            .config
            .library_dirs
            .iter()
            .any(|p| path.starts_with(absolute(Path::new(p), &self.project.root)))
    }
}
