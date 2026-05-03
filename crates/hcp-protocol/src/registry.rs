use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareImage {
    pub name: String,
    pub version: String,
    pub files: Vec<String>,
    pub size: usize,
    pub ecc_signals: u32,
}

#[derive(Debug, Default)]
pub struct ImageRegistry {
    images: RwLock<HashMap<String, HardwareImage>>,
}

impl ImageRegistry {
    pub fn new() -> Self { Self::default() }
    pub fn publish(&self, img: HardwareImage) {
        let key = format!("{}:{}", img.name, img.version);
        self.images.write().unwrap().insert(key, img);
    }
    pub fn list(&self) -> Vec<HardwareImage> {
        self.images.read().unwrap().values().cloned().collect()
    }
    pub fn get(&self, key: &str) -> Option<HardwareImage> {
        self.images.read().unwrap().get(key).cloned()
    }
}
