use combinator_app::{
    ensure_output_parent, validate_resource_limits, FileSink, OutputRecord, OutputSink,
    ResourceLimits,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const PROFILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub version: u32,
    pub active_mode: String,
    pub combine: CombineProfile,
    pub join: JoinProfile,
    pub output_path: String,
    pub overwrite: bool,
    pub limits: LimitsProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CombineProfile {
    pub sources: Vec<String>,
    pub file_sources: Vec<Option<String>>,
    pub file_formats: Vec<String>,
    pub list_delimiter: String,
    #[serde(default)]
    pub field_separator: String,
    pub template: String,
    pub template_file: String,
    pub template_file_mode: bool,
    pub transforms: String,
    pub filters: String,
    pub names: String,
    pub offset: String,
    pub limit: String,
    pub choose: String,
    pub length: String,
    pub operation: String,
    pub format: String,
    #[serde(default)]
    pub formula_policy: String,
    pub zip_policy: String,
    pub reverse: bool,
    pub reverse_fields: bool,
    pub lean_jsonl: bool,
    pub shard_index: String,
    pub shard_count: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JoinProfile {
    pub left_path: String,
    pub right_path: String,
    pub left_key: String,
    pub right_key: String,
    pub format: String,
    pub kind: String,
    pub offset: String,
    pub limit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsProfile {
    pub max_combinations: String,
    pub max_output_bytes: String,
    pub max_input_bytes: String,
    pub max_item_bytes: String,
    pub max_items_per_list: String,
    pub max_total_items: String,
    pub max_lists: String,
    pub timeout_ms: String,
    pub join_max_records: String,
    pub join_fanout: String,
}

impl Default for LimitsProfile {
    fn default() -> Self {
        let limits = ResourceLimits::default();
        Self {
            max_combinations: limits.max_combinations.to_string(),
            max_output_bytes: limits.max_output_bytes.to_string(),
            max_input_bytes: limits.max_input_bytes.to_string(),
            max_item_bytes: limits.max_item_bytes.to_string(),
            max_items_per_list: limits.max_items_per_list.to_string(),
            max_total_items: limits.max_total_items.to_string(),
            max_lists: limits.max_lists.to_string(),
            timeout_ms: String::new(),
            join_max_records: limits.max_join_records.to_string(),
            join_fanout: limits.max_join_key_fanout.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Preferences {
    pub recent_profiles: Vec<String>,
    pub last_profile: Option<String>,
    pub default_output_directory: Option<String>,
}

pub fn load_profile(path: &Path) -> Result<Profile, String> {
    let text =
        fs::read_to_string(path).map_err(|error| format!("could not read profile: {error}"))?;
    let profile: Profile = serde_json::from_str(&text)
        .map_err(|error| format!("profile is not valid JSON: {error}"))?;
    if profile.version != PROFILE_VERSION {
        return Err(format!(
            "unsupported profile version {}; expected {}",
            profile.version, PROFILE_VERSION
        ));
    }
    validate_profile_limits(&profile.limits)?;
    Ok(resolve_profile_paths(
        profile,
        path.parent().unwrap_or_else(|| Path::new(".")),
    ))
}

fn validate_profile_limits(limits: &LimitsProfile) -> Result<(), String> {
    let limits = ResourceLimits {
        max_output_bytes: parse_u128(&limits.max_output_bytes, "max-output-bytes")?,
        max_input_bytes: parse_usize(&limits.max_input_bytes, "max-input-bytes")?,
        max_item_bytes: parse_usize(&limits.max_item_bytes, "max-item-bytes")?,
        max_items_per_list: parse_usize(&limits.max_items_per_list, "max-items-per-list")?,
        max_lists: parse_usize(&limits.max_lists, "max-lists")?,
        max_total_items: parse_usize(&limits.max_total_items, "max-total-items")?,
        max_combinations: parse_u128(&limits.max_combinations, "max-combinations")?,
        max_join_records: parse_usize(&limits.join_max_records, "max-join-records")?,
        max_join_key_fanout: parse_u128(&limits.join_fanout, "max-join-key-fanout")?,
        timeout_ms: if limits.timeout_ms.is_empty() {
            None
        } else {
            Some(parse_u64(&limits.timeout_ms, "timeout-ms")?)
        },
    };
    validate_resource_limits(&limits).map_err(|error| format!("RESOURCE_LIMIT_TOO_HIGH: {error}"))
}

fn parse_u128(value: &str, field: &str) -> Result<u128, String> {
    value
        .parse()
        .map_err(|_| format!("profile limit {field} is not a non-negative integer"))
}

fn parse_u64(value: &str, field: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|_| format!("profile limit {field} is not a non-negative integer"))
}

fn parse_usize(value: &str, field: &str) -> Result<usize, String> {
    value
        .parse()
        .map_err(|_| format!("profile limit {field} is not a non-negative integer"))
}

pub fn save_profile(path: &Path, mut profile: Profile) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    profile = relativize_profile_paths(profile, parent);
    let text = serde_json::to_string_pretty(&profile)
        .map_err(|error| format!("could not encode profile: {error}"))?;
    atomic_write(path, &format!("{text}\n"))
}

pub fn load_preferences() -> Preferences {
    preferences_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_preferences(preferences: &Preferences) -> Result<(), String> {
    let Some(path) = preferences_path() else {
        return Ok(());
    };
    ensure_output_parent(&path).map_err(|error| format!("{}: {}", error.code, error.message))?;
    let text = serde_json::to_string_pretty(preferences)
        .map_err(|error| format!("could not encode preferences: {error}"))?;
    atomic_write(&path, &format!("{text}\n"))
}

fn atomic_write(path: &Path, contents: &str) -> Result<(), String> {
    let mut sink =
        FileSink::open(path, true).map_err(|error| format!("{}: {}", error.code, error.message))?;
    sink.record(OutputRecord {
        ordinal: 0,
        value: contents.to_string(),
        fields: Vec::new(),
    })
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    sink.commit()
        .map_err(|error| format!("{}: {}", error.code, error.message))
}

pub fn remember_profile(mut preferences: Preferences, path: &Path) -> Preferences {
    let path = path.to_string_lossy().into_owned();
    preferences.recent_profiles.retain(|item| item != &path);
    preferences.recent_profiles.insert(0, path.clone());
    preferences.recent_profiles.truncate(8);
    preferences.last_profile = Some(path);
    preferences
}

fn preferences_path() -> Option<PathBuf> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Some(
            PathBuf::from(appdata)
                .join("Combinator")
                .join("preferences.json"),
        );
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|base| base.join("combinator").join("preferences.json"))
}

fn resolve(path: &str, base: &Path) -> String {
    if path.is_empty() {
        return String::new();
    }
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        path.to_string()
    } else {
        base.join(candidate).to_string_lossy().into_owned()
    }
}

fn store(path: &str, base: &Path) -> String {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        if let Ok(relative) = candidate.strip_prefix(base) {
            return relative.to_string_lossy().into_owned();
        }
    }
    path.to_string()
}

fn resolve_profile_paths(mut profile: Profile, base: &Path) -> Profile {
    profile.output_path = resolve(&profile.output_path, base);
    profile.combine.template_file = resolve(&profile.combine.template_file, base);
    for path in profile.combine.file_sources.iter_mut().flatten() {
        *path = resolve(path, base);
    }
    profile.join.left_path = resolve(&profile.join.left_path, base);
    profile.join.right_path = resolve(&profile.join.right_path, base);
    profile
}

fn relativize_profile_paths(mut profile: Profile, base: &Path) -> Profile {
    profile.output_path = store(&profile.output_path, base);
    profile.combine.template_file = store(&profile.combine.template_file, base);
    for path in profile.combine.file_sources.iter_mut().flatten() {
        *path = store(path, base);
    }
    profile.join.left_path = store(&profile.join.left_path, base);
    profile.join.right_path = store(&profile.join.right_path, base);
    profile
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Profile {
        Profile {
            version: PROFILE_VERSION,
            active_mode: "join".into(),
            combine: CombineProfile {
                file_sources: vec![Some("input.csv".into())],
                field_separator: "|".into(),
                formula_policy: "Reject".into(),
                ..Default::default()
            },
            join: JoinProfile {
                left_path: "left.csv".into(),
                right_path: "right.csv".into(),
                ..Default::default()
            },
            output_path: "output.txt".into(),
            overwrite: true,
            limits: LimitsProfile::default(),
        }
    }

    #[test]
    fn profile_round_trip_preserves_relative_paths() {
        let folder =
            std::env::temp_dir().join(format!("combinator-profile-test-{}", std::process::id()));
        let path = folder.join("profile.json");
        fs::create_dir_all(&folder).expect("create profile folder");
        save_profile(&path, sample()).expect("save profile");
        let loaded = load_profile(&path).expect("load profile");
        assert_eq!(loaded.active_mode, "join");
        assert_eq!(loaded.combine.formula_policy, "Reject");
        assert_eq!(
            loaded.combine.file_sources,
            vec![Some(
                folder.join("input.csv").to_string_lossy().into_owned()
            )]
        );
        assert_eq!(loaded.combine.field_separator, "|");
        assert_eq!(
            loaded.join.left_path,
            folder.join("left.csv").to_string_lossy()
        );
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn profile_load_rejects_limits_above_compiled_ceilings() {
        let path = std::env::temp_dir().join(format!(
            "combinator-profile-limits-test-{}.json",
            std::process::id()
        ));
        let mut profile = sample();
        profile.limits.max_combinations = (combinator_app::HARD_MAX_COMBINATIONS + 1).to_string();
        save_profile(&path, profile).expect("save hostile profile");

        let error = load_profile(&path).unwrap_err();
        assert!(error.starts_with("RESOURCE_LIMIT_TOO_HIGH:"));
        assert!(error.contains("max-combinations"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn older_profile_without_formula_policy_still_loads() {
        let mut value = serde_json::to_value(sample()).expect("encode profile");
        value["combine"]
            .as_object_mut()
            .expect("combine object")
            .remove("formula_policy");
        let loaded: Profile = serde_json::from_value(value).expect("load older profile");
        assert!(loaded.combine.formula_policy.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn profile_save_rejects_nested_symlink_parent() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "combinator-profile-symlink-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("real")).expect("create profile folder");
        symlink(root.join("real"), root.join("linked")).expect("create symlink");

        let error = save_profile(&root.join("linked/profile.json"), sample()).unwrap_err();
        assert!(error.contains("UNSAFE_OUTPUT_PATH"));
        fs::remove_dir_all(root).expect("remove profile folder");
    }

    #[test]
    fn unsupported_profile_version_is_rejected() {
        let folder =
            std::env::temp_dir().join(format!("combinator-version-test-{}", std::process::id()));
        let path = folder.join("profile.json");
        fs::create_dir_all(&folder).expect("create test folder");
        let mut profile = sample();
        profile.version = PROFILE_VERSION + 1;
        fs::write(&path, serde_json::to_string(&profile).unwrap()).expect("write profile");
        assert!(load_profile(&path)
            .unwrap_err()
            .contains("unsupported profile version"));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn recent_profiles_are_unique_and_bounded() {
        let mut preferences = Preferences::default();
        for index in 0..10 {
            preferences =
                remember_profile(preferences, Path::new(&format!("profile-{index}.json")));
        }
        preferences = remember_profile(preferences, Path::new("profile-5.json"));
        assert_eq!(preferences.recent_profiles.len(), 8);
        assert_eq!(preferences.recent_profiles[0], "profile-5.json");
    }
}
