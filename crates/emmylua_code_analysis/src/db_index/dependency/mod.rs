mod file_dependency_relation;

use std::collections::{HashMap, HashSet};

use file_dependency_relation::FileDenpendencyRelation;

use crate::FileId;

use super::LuaIndex;

#[derive(Debug)]
pub struct LuaDenpendencyIndex {
    dependencies: HashMap<FileId, HashSet<FileId>>,
}

impl LuaDenpendencyIndex {
    pub fn new() -> Self {
        Self {
            dependencies: HashMap::new(),
        }
    }

    pub fn add_required_file(&mut self, file_id: FileId, dependency_id: FileId) {
        self.dependencies
            .entry(file_id)
            .or_insert_with(HashSet::new)
            .insert(dependency_id);
    }

    pub fn get_required_files(&self, file_id: &FileId) -> Option<&HashSet<FileId>> {
        self.dependencies.get(file_id)
    }

    pub fn get_file_dependencies<'a>(&'a self) -> FileDenpendencyRelation<'a> {
        FileDenpendencyRelation::new(&self.dependencies)
    }
}

impl LuaIndex for LuaDenpendencyIndex {
    fn remove(&mut self, file_id: FileId) {
        self.dependencies.remove(&file_id);
    }

    fn clear(&mut self) {
        self.dependencies.clear();
    }
}
