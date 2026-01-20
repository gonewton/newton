use crate::core::entities::{
    ArtifactMetadata, ErrorRecord, Iteration, OptimizationExecution, ToolResult, Workspace,
};
use anyhow::Result;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

pub trait Serializer {
    fn serialize<T: serde::Serialize>(&self, data: &T) -> Result<Vec<u8>>;
    fn deserialize<T: serde::de::DeserializeOwned>(&self, data: &[u8]) -> Result<T>;
}

pub struct JsonSerializer;

impl Serializer for JsonSerializer {
    fn serialize<T: serde::Serialize>(&self, data: &T) -> Result<Vec<u8>> {
        serde_json::to_vec(data).map_err(Into::into)
    }

    fn deserialize<T: serde::de::DeserializeOwned>(&self, data: &[u8]) -> Result<T> {
        serde_json::from_slice(data).map_err(Into::into)
    }
}

pub trait FileSerializer {
    fn save_to_file<T, S: Serializer>(
        &self,
        path: &PathBuf,
        data: &T,
        serializer: &S,
    ) -> Result<()>
    where
        T: Serialize;
    fn load_from_file<T, S: Serializer>(&self, path: &PathBuf, serializer: &S) -> Result<T>
    where
        T: DeserializeOwned;
}

pub struct FileUtils;

impl FileSerializer for FileUtils {
    fn save_to_file<T, S: Serializer>(&self, path: &PathBuf, data: &T, serializer: &S) -> Result<()>
    where
        T: serde::Serialize,
    {
        let content = serializer.serialize(data)?;
        let mut file = fs::File::create(path)?;
        file.write_all(&content)?;
        Ok(())
    }

    fn load_from_file<T, S: Serializer>(&self, path: &PathBuf, serializer: &S) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let mut file = fs::File::open(path)?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;
        serializer.deserialize(&content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_serializer_creation() {
        let serializer = JsonSerializer::default();
        assert!(true);
    }

    #[test]
    fn test_file_utils_creation() {
        let file_utils = FileUtils;
        assert!(true);
    }
}
