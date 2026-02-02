use newton::utils::serialization::{FileSerializer, FileUtils, JsonSerializer, Serializer};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq)]
struct TestData {
    name: String,
    value: i32,
    items: Vec<String>,
}

impl TestData {
    fn new(name: &str, value: i32) -> Self {
        TestData {
            name: name.to_string(),
            value,
            items: vec!["item1".to_string(), "item2".to_string()],
        }
    }
}

#[test]
fn test_json_serializer_serialize() {
    let serializer = JsonSerializer;
    let data = TestData::new("test", 42);

    let result = serializer.serialize(&data);
    assert!(result.is_ok());

    let serialized = result.unwrap();
    assert!(!serialized.is_empty());

    // Verify it's valid JSON
    let json_str = String::from_utf8(serialized).unwrap();
    assert!(json_str.contains("test"));
    assert!(json_str.contains("42"));
}

#[test]
fn test_json_serializer_deserialize() {
    let serializer = JsonSerializer;
    let data = TestData::new("test", 42);

    let serialized = serializer.serialize(&data).unwrap();
    let deserialized: TestData = serializer.deserialize(&serialized).unwrap();

    assert_eq!(data, deserialized);
}

#[test]
fn test_json_serializer_complex_data() {
    let serializer = JsonSerializer;
    let mut map = HashMap::new();
    map.insert("key1".to_string(), "value1".to_string());
    map.insert("key2".to_string(), "value2".to_string());

    let result = serializer.serialize(&map);
    assert!(result.is_ok());

    let serialized = result.unwrap();
    let deserialized: HashMap<String, String> = serializer.deserialize(&serialized).unwrap();

    assert_eq!(deserialized.len(), 2);
    assert_eq!(deserialized.get("key1"), Some(&"value1".to_string()));
    assert_eq!(deserialized.get("key2"), Some(&"value2".to_string()));
}

#[test]
fn test_json_serializer_invalid_data() {
    let serializer = JsonSerializer;
    let invalid_json = b"{ invalid json }";

    let result: Result<TestData, _> = serializer.deserialize(invalid_json);
    assert!(result.is_err());
}

#[test]
fn test_json_serializer_empty_data() {
    let serializer = JsonSerializer;
    let empty_json = b"";

    let result: Result<TestData, _> = serializer.deserialize(empty_json);
    assert!(result.is_err());
}

#[test]
fn test_json_serializer_unicode() {
    let serializer = JsonSerializer;
    let data = TestData {
        name: "测试中文 🚀".to_string(),
        value: 100,
        items: vec!["项目1".to_string(), "项目2".to_string()],
    };

    let serialized = serializer.serialize(&data).unwrap();
    let deserialized: TestData = serializer.deserialize(&serialized).unwrap();

    assert_eq!(data, deserialized);
}

#[test]
fn test_file_utils_creation() {
    let _file_utils = FileUtils;
    // Test that FileUtils can be created without error
}

#[test]
fn test_file_utils_save_and_load() {
    let temp_dir = TempDir::new().unwrap();
    let file_utils = FileUtils;
    let serializer = JsonSerializer;

    let data = TestData::new("file test", 123);
    let file_path = temp_dir.path().join("test_data.json");

    // Save the data
    let save_result = file_utils.save_to_file(&file_path, &data, &serializer);
    assert!(save_result.is_ok());
    assert!(file_path.exists());

    // Load the data
    let load_result: Result<TestData, _> = file_utils.load_from_file(&file_path, &serializer);
    assert!(load_result.is_ok());

    let loaded_data = load_result.unwrap();
    assert_eq!(data, loaded_data);
}

#[test]
fn test_file_utils_load_nonexistent() {
    let temp_dir = TempDir::new().unwrap();
    let file_utils = FileUtils;
    let serializer = JsonSerializer;

    let nonexistent_path = temp_dir.path().join("nonexistent.json");
    let result: Result<TestData, _> = file_utils.load_from_file(&nonexistent_path, &serializer);
    assert!(result.is_err());
}

#[test]
fn test_file_utils_save_nested_path() {
    let temp_dir = TempDir::new().unwrap();
    let file_utils = FileUtils;
    let serializer = JsonSerializer;

    let data = TestData::new("nested test", 456);
    let nested_path = temp_dir
        .path()
        .join("nested")
        .join("deep")
        .join("test.json");

    let result = file_utils.save_to_file(&nested_path, &data, &serializer);
    assert!(result.is_ok());
    assert!(nested_path.exists());
}

#[test]
fn test_file_utils_large_data() {
    let temp_dir = TempDir::new().unwrap();
    let file_utils = FileUtils;
    let serializer = JsonSerializer;

    let large_data = TestData {
        name: "large test".to_string(),
        value: 999999,
        items: (0..1000).map(|i| format!("item_{}", i)).collect(),
    };

    let file_path = temp_dir.path().join("large_data.json");

    let save_result = file_utils.save_to_file(&file_path, &large_data, &serializer);
    assert!(save_result.is_ok());

    let load_result: Result<TestData, _> = file_utils.load_from_file(&file_path, &serializer);
    assert!(load_result.is_ok());

    let loaded_data = load_result.unwrap();
    assert_eq!(large_data.name, loaded_data.name);
    assert_eq!(large_data.value, loaded_data.value);
    assert_eq!(large_data.items.len(), loaded_data.items.len());
}

#[test]
fn test_file_roundtrip_with_multiple_types() {
    let temp_dir = TempDir::new().unwrap();
    let file_utils = FileUtils;
    let serializer = JsonSerializer;

    // Test with different data types
    let test_cases: Vec<(PathBuf, Box<dyn Fn() -> serde_json::Value + '_>)> = vec![
        (
            temp_dir.path().join("string.json"),
            Box::new(|| serde_json::Value::String("test string".to_string())),
        ),
        (
            temp_dir.path().join("number.json"),
            Box::new(|| serde_json::Value::Number(serde_json::Number::from(42))),
        ),
        (
            temp_dir.path().join("bool.json"),
            Box::new(|| serde_json::Value::Bool(true)),
        ),
        (
            temp_dir.path().join("array.json"),
            Box::new(|| serde_json::Value::Array(vec![1.into(), 2.into(), 3.into()])),
        ),
        (
            temp_dir.path().join("object.json"),
            Box::new(|| {
                let mut obj = serde_json::Map::new();
                obj.insert("key".to_string(), "value".into());
                obj.into()
            }),
        ),
    ];

    for (path, data_fn) in test_cases {
        let data = data_fn();

        // Save
        let save_result = file_utils.save_to_file(&path, &data, &serializer);
        assert!(save_result.is_ok(), "Failed to save to {:?}", path);

        // Load
        let load_result: Result<serde_json::Value, _> =
            file_utils.load_from_file(&path, &serializer);
        assert!(load_result.is_ok(), "Failed to load from {:?}", path);

        let loaded_data = load_result.unwrap();
        assert_eq!(data, loaded_data, "Data mismatch for {:?}", path);
    }
}

#[test]
fn test_serialization_with_null_values() {
    let serializer = JsonSerializer;

    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq)]
    struct TestWithOptional {
        name: Option<String>,
        value: Option<i32>,
    }

    let data = TestWithOptional {
        name: None,
        value: Some(42),
    };

    let serialized = serializer.serialize(&data).unwrap();
    let deserialized: TestWithOptional = serializer.deserialize(&serialized).unwrap();

    assert_eq!(data, deserialized);
    assert!(deserialized.name.is_none());
    assert_eq!(deserialized.value, Some(42));
}
