// Copyright 2016 Mozilla Foundation
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Structured dump of hash inputs for cache miss debugging.
//!
//! Activated via `SCCACHE_LOG_HASH_INPUTS=/path/to/file.jsonl`.
//! When set, appends one JSON line per compilation to the specified file.

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::LazyLock;

static HASH_INPUTS_PATH: LazyLock<Option<PathBuf>> =
    LazyLock::new(|| std::env::var("SCCACHE_LOG_HASH_INPUTS").ok().map(PathBuf::from));

pub fn hash_inputs_enabled() -> bool {
    HASH_INPUTS_PATH.is_some()
}

/// All inputs that went into a cache hash key.
#[derive(Debug, Serialize)]
#[serde(tag = "compiler_family")]
pub enum HashInputs {
    #[serde(rename = "c_cpp")]
    CCpp(CCppHashInputs),
    #[serde(rename = "rust")]
    Rust(RustHashInputs),
}

/// Hash inputs for a C/C++ compilation.
#[derive(Debug, Serialize)]
pub struct CCppHashInputs {
    pub hash_key: String,
    pub output_file: String,
    pub cache_version: String,
    pub compiler_digest: String,
    pub plusplus: bool,
    pub language: String,
    pub arguments: Vec<String>,
    pub extra_hashes: Vec<String>,
    pub env_vars: BTreeMap<String, String>,
    pub preprocessor_output_digest: String,
    pub basedirs: Vec<String>,
}

/// Hash inputs for a Rust compilation.
#[derive(Debug, Serialize)]
pub struct RustHashInputs {
    pub hash_key: String,
    pub output_file: String,
    pub cache_version: String,
    pub compiler_shlibs_digests: Vec<String>,
    pub arguments: String,
    pub source_hashes: BTreeMap<String, String>,
    pub extern_hashes: BTreeMap<String, String>,
    pub staticlib_hashes: BTreeMap<String, String>,
    pub target_json_hash: Option<String>,
    pub env_deps: BTreeMap<String, String>,
    pub cargo_env_vars: BTreeMap<String, String>,
    pub cwd: String,
    pub compiler_version: String,
}

/// Append hash inputs as a JSON line to the configured file.
///
/// `cache_result` is injected into the JSON output (e.g. "hit", "miss").
pub fn emit_hash_inputs(inputs: &HashInputs, cache_result: &str) {
    let path = match HASH_INPUTS_PATH.as_ref() {
        Some(p) => p,
        None => return,
    };

    let json = match serde_json::to_value(inputs)
        .map(|mut v| {
            v.as_object_mut()
                .expect("HashInputs serializes as object")
                .insert("cache_result".into(), cache_result.into());
            v
        })
        .and_then(|v| serde_json::to_string(&v))
    {
        Ok(j) => j,
        Err(e) => {
            warn!("Failed to serialize hash inputs: {}", e);
            return;
        }
    };

    use std::io::Write;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                warn!(
                    "Failed to create parent directory for {}: {}",
                    path.display(),
                    e
                );
                return;
            }
        }
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path);
    match file {
        Ok(mut f) => {
            if let Err(e) = writeln!(f, "{}", json) {
                warn!("Failed to write hash inputs to {}: {}", path.display(), e);
            }
        }
        Err(e) => {
            warn!("Failed to open {} for writing: {}", path.display(), e);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_c_cpp_hash_inputs_serialization() {
        let inputs = HashInputs::CCpp(CCppHashInputs {
            hash_key: "abc123".into(),
            output_file: "foo.o".into(),
            cache_version: "12".into(),
            compiler_digest: "deadbeef".into(),
            plusplus: false,
            language: "c".into(),
            arguments: vec!["-O2".into(), "-Wall".into()],
            extra_hashes: vec![],
            env_vars: BTreeMap::from([("SDKROOT".into(), "/sdk".into())]),
            preprocessor_output_digest: "cafebabe".into(),
            basedirs: vec![],
        });
        let json = serde_json::to_string(&inputs).unwrap();
        assert!(json.contains("\"compiler_family\":\"c_cpp\""));
        assert!(json.contains("\"hash_key\":\"abc123\""));
        assert!(json.contains("\"SDKROOT\":\"/sdk\""));

        // Verify deterministic ordering by round-tripping
        let json2 = serde_json::to_string(&inputs).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn test_rust_hash_inputs_serialization() {
        let inputs = HashInputs::Rust(RustHashInputs {
            hash_key: "def456".into(),
            output_file: "libfoo".into(),
            cache_version: "6".into(),
            compiler_shlibs_digests: vec!["aaa".into()],
            arguments: "some args".into(),
            source_hashes: BTreeMap::from([("src/main.rs".into(), "h1".into())]),
            extern_hashes: BTreeMap::from([("libbar.rlib".into(), "h2".into())]),
            staticlib_hashes: BTreeMap::new(),
            target_json_hash: None,
            env_deps: BTreeMap::from([("OUT_DIR".into(), "/tmp/out".into())]),
            cargo_env_vars: BTreeMap::from([("CARGO_PKG_NAME".into(), "foo".into())]),
            cwd: "/home/user/project".into(),
            compiler_version: "rustc 1.78.0".into(),
        });
        let json = serde_json::to_string(&inputs).unwrap();
        assert!(json.contains("\"compiler_family\":\"rust\""));
        assert!(json.contains("\"hash_key\":\"def456\""));
        assert!(json.contains("\"src/main.rs\":\"h1\""));
    }

    #[test]
    fn test_btreemap_ordering_is_deterministic() {
        let mut map1 = BTreeMap::new();
        map1.insert("z_var".to_string(), "1".to_string());
        map1.insert("a_var".to_string(), "2".to_string());
        map1.insert("m_var".to_string(), "3".to_string());

        let inputs = HashInputs::CCpp(CCppHashInputs {
            hash_key: "test".into(),
            output_file: "test.o".into(),
            cache_version: "12".into(),
            compiler_digest: "digest".into(),
            plusplus: false,
            language: "c".into(),
            arguments: vec![],
            extra_hashes: vec![],
            env_vars: map1,
            preprocessor_output_digest: "pp".into(),
            basedirs: vec![],
        });

        let json = serde_json::to_string(&inputs).unwrap();
        let a_pos = json.find("\"a_var\"").unwrap();
        let m_pos = json.find("\"m_var\"").unwrap();
        let z_pos = json.find("\"z_var\"").unwrap();
        assert!(a_pos < m_pos);
        assert!(m_pos < z_pos);
    }
}
