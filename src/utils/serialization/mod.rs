use anyhow::Result;
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

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
    fn save_to_file<T, S: Serializer>(&self, path: &Path, data: &T, serializer: &S) -> Result<()>
    where
        T: Serialize;
    fn load_from_file<T, S: Serializer>(&self, path: &Path, serializer: &S) -> Result<T>
    where
        T: DeserializeOwned;
}

pub struct FileUtils;

impl FileSerializer for FileUtils {
    fn save_to_file<T, S: Serializer>(&self, path: &Path, data: &T, serializer: &S) -> Result<()>
    where
        T: serde::Serialize,
    {
        let content = serializer.serialize(data)?;
        let mut file = fs::File::create(path)?;
        file.write_all(&content)?;
        Ok(())
    }

    fn load_from_file<T, S: Serializer>(&self, path: &Path, serializer: &S) -> Result<T>
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
        let _serializer = JsonSerializer;
    }

    #[test]
    fn test_file_utils_creation() {
        let _file_utils = FileUtils;
    }
}
