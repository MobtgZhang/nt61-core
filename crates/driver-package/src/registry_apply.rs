//! Atomic application of the supported INF registry subset to an offline image.
//!
//! This is not an installer or a signature-policy decision. The caller must
//! supply explicit hive/view/HKR bindings. No device interfaces are created.
//! A batch is evaluated on a private copy and published only after every
//! operation and the final resource checks succeed. Persistence belongs to the
//! caller; in particular this is not a crash-recovery journal for live hives.
use crate::{
    RegistryAction, RegistryPlan, RegistryRoot, RegistryScope, RegistryValue, RegistryView,
    RegistryWriteMode,
};
use alloc::{collections::BTreeMap, string::String, vec::Vec};

const MAX_KEYS: usize = 16_384;
const MAX_VALUES: usize = 32_768;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_OPERATIONS: usize = 8192;
const MAX_PATH: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyError {
    Limit,
    InvalidName,
    InvalidData,
    MissingBinding,
    AmbiguousBinding,
    MissingDirectory,
    UnsupportedSecurity,
    UnsupportedOmittedData,
    UnsupportedInterfaces,
    UnsupportedComparison,
    TypeMismatch,
    DeleteRoot,
}

/// Map an INF root/view (and HKR scope) to a path in ONE offline image.
/// `source_prefix` is matched on key-component boundaries; the longest match
/// wins. For example HKLM SYSTEM\CurrentControlSet can map to ControlSet001
/// in an offline SYSTEM hive. Other HKLM paths remain inaccessible.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct RegistryBinding {
    pub root: RegistryRoot,
    pub scope: Option<RegistryScope>,
    pub view: RegistryView,
    pub source_prefix: String,
    pub destination: String,
}

/// Already encoded registry data. String data uses UTF-16LE, DWORD is LE.
/// Unlike RegistryValue this contains no unresolved DIRIDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredValue {
    pub kind: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplySummary {
    /// Operations changing state; one operation may create several parent keys.
    pub changed: usize,
    pub unchanged: usize,
}

/// Canonical ASCII key/value identities; Unicode VALUE DATA is supported.
/// Windows' invariant Unicode name folding requires the system upcase table.
/// Until that is supplied, non-ASCII identifiers are rejected, not conflated.
/// The empty path is the hive root and always exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryImage {
    keys: BTreeMap<String, BTreeMap<String, StoredValue>>,
}

impl Default for RegistryImage {
    fn default() -> Self {
        let mut keys = BTreeMap::new();
        keys.insert(String::new(), BTreeMap::new());
        Self { keys }
    }
}

impl RegistryImage {
    pub fn keys(&self) -> impl Iterator<Item = (&str, &BTreeMap<String, StoredValue>)> {
        self.keys.iter().map(|(key, values)| (key.as_str(), values))
    }

    pub fn contains_key(&self, path: &str) -> Result<bool, ApplyError> {
        Ok(self.keys.contains_key(&key_path(path)?))
    }

    pub fn value(&self, path: &str, name: &str) -> Result<Option<&StoredValue>, ApplyError> {
        let name = value_name(name)?;
        Ok(self
            .keys
            .get(&key_path(path)?)
            .and_then(|values| values.get(&name)))
    }

    /// Apply only registry operations, with no authentication or device state
    /// changes. An interface-bearing plan is rejected rather than partially
    /// executing an interface installation. Native/32/64 views never alias
    /// implicitly: the bindings must explicitly describe the target hive.
    ///
    /// # Errors
    /// Unsupported actions/SDDL, unmapped paths/views/scopes, malformed data,
    /// ambiguous bindings, invalid DIRIDs or resource exhaustion leave `self`
    /// byte-for-byte logically unchanged, including when earlier writes worked.
    pub fn apply(
        &mut self,
        plan: &RegistryPlan,
        bindings: &[RegistryBinding],
        directories: &[(u32, &str)],
    ) -> Result<ApplySummary, ApplyError> {
        if plan.operations.len() > MAX_OPERATIONS || bindings.len() > 1024 || directories.len() > 64
        {
            return Err(ApplyError::Limit);
        }
        if !plan.interfaces.is_empty() {
            return Err(ApplyError::UnsupportedInterfaces);
        }
        let bindings = validate_bindings(bindings)?;
        for (index, (id, directory)) in directories.iter().enumerate() {
            if directories[..index].iter().any(|(old, _)| old == id) {
                return Err(ApplyError::AmbiguousBinding);
            }
            if directory.contains('\0') || directory.len() > MAX_PATH {
                return Err(ApplyError::InvalidData);
            }
        }
        let mut next = self.clone();
        let mut summary = ApplySummary::default();
        for operation in &plan.operations {
            if operation.security_descriptor.is_some() {
                return Err(ApplyError::UnsupportedSecurity);
            }
            let source = key_path(&operation.subkey)?;
            let binding = bindings
                .iter()
                .filter(|(binding, prefix, _)| {
                    binding.root == operation.root
                        && binding.view == operation.view
                        && (binding.root != RegistryRoot::Relative
                            || binding.scope.as_ref() == Some(&operation.scope))
                        && under(&source, prefix)
                })
                .max_by_key(|(_, prefix, _)| prefix.len())
                .ok_or(ApplyError::MissingBinding)?;
            let suffix = source[binding.1.len()..].trim_start_matches('\\');
            let path = join(&binding.2, suffix)?;
            let changed = next.apply_one(&path, &operation.action, directories)?;
            if changed {
                summary.changed += 1;
            } else {
                summary.unchanged += 1;
            }
            next.check_limits()?;
        }
        *self = next;
        Ok(summary)
    }

    fn apply_one(
        &mut self,
        path: &str,
        action: &RegistryAction,
        directories: &[(u32, &str)],
    ) -> Result<bool, ApplyError> {
        match action {
            RegistryAction::CreateKey => self.ensure_key(path),
            RegistryAction::AddWithoutData { .. } => Err(ApplyError::UnsupportedOmittedData),
            RegistryAction::DeleteKey => {
                if path.is_empty() {
                    return Err(ApplyError::DeleteRoot);
                }
                let count = self.keys.len();
                self.keys.retain(|key, _| !under(key, path));
                Ok(count != self.keys.len())
            }
            RegistryAction::DeleteValue(name) => {
                let name = value_name(name)?;
                Ok(self
                    .keys
                    .get_mut(path)
                    .and_then(|v| v.remove(&name))
                    .is_some())
            }
            RegistryAction::SetValue { name, value, mode } => {
                let name = value_name(name)?;
                let mut value = resolve_value(value, directories)?;
                let prior = self.keys.get(path).and_then(|v| v.get(&name));
                match mode {
                    RegistryWriteMode::IfAbsent if prior.is_some() => return Ok(false),
                    RegistryWriteMode::IfPresent if prior.is_none() => return Ok(false),
                    RegistryWriteMode::AppendUnique => {
                        if value.kind != 7 || prior.is_some_and(|v| v.kind != 7) {
                            return Err(ApplyError::TypeMismatch);
                        }
                        let incoming = multi_strings(&value.data)?;
                        let mut strings = prior
                            .map(|v| multi_strings(&v.data))
                            .transpose()?
                            .unwrap_or_default();
                        require_ascii_comparison(&strings)?;
                        require_ascii_comparison(&incoming)?;
                        for string in incoming {
                            if !strings.iter().any(|old| old.eq_ignore_ascii_case(&string)) {
                                strings.push(string);
                            }
                        }
                        value.data = encode_multi(&strings);
                    }
                    _ => (),
                }
                if prior == Some(&value) {
                    return Ok(false);
                }
                self.ensure_key(path)?;
                self.keys
                    .get_mut(path)
                    .ok_or(ApplyError::InvalidName)?
                    .insert(name, value);
                Ok(true)
            }
            RegistryAction::DeleteMultiString { name, value } => {
                let name = value_name(name)?;
                let Some(prior) = self.keys.get_mut(path).and_then(|v| v.get_mut(&name)) else {
                    return Ok(false);
                };
                if prior.kind != 7 {
                    return Err(ApplyError::TypeMismatch);
                }
                let mut strings = multi_strings(&prior.data)?;
                require_ascii_comparison(&strings)?;
                if !value.is_ascii() {
                    return Err(ApplyError::UnsupportedComparison);
                }
                let count = strings.len();
                strings.retain(|old| !old.eq_ignore_ascii_case(value));
                if strings.len() == count {
                    return Ok(false);
                }
                prior.data = encode_multi(&strings);
                Ok(true)
            }
        }
    }

    fn ensure_key(&mut self, path: &str) -> Result<bool, ApplyError> {
        if self.keys.contains_key(path) {
            return Ok(false);
        }
        for (offset, _) in path.match_indices('\\') {
            self.keys.entry(String::from(&path[..offset])).or_default();
        }
        self.keys.entry(String::from(path)).or_default();
        Ok(true)
    }

    fn check_limits(&self) -> Result<(), ApplyError> {
        let mut bytes = 0usize;
        let mut count = 0usize;
        if self.keys.len() > MAX_KEYS {
            return Err(ApplyError::Limit);
        }
        for (key, values) in &self.keys {
            bytes = bytes.checked_add(key.len()).ok_or(ApplyError::Limit)?;
            count = count.checked_add(values.len()).ok_or(ApplyError::Limit)?;
            for (name, value) in values {
                bytes = bytes
                    .checked_add(name.len())
                    .and_then(|n| n.checked_add(value.data.len()))
                    .ok_or(ApplyError::Limit)?;
            }
            if bytes > MAX_BYTES || count > MAX_VALUES {
                return Err(ApplyError::Limit);
            }
        }
        Ok(())
    }
}

fn key_path(path: &str) -> Result<String, ApplyError> {
    if !path.is_ascii() || path.len() > MAX_PATH || path.bytes().any(|b| b < 32) {
        return Err(ApplyError::InvalidName);
    }
    let mut result = String::new();
    for component in path.split('\\').filter(|p| !p.is_empty()) {
        if component.len() > 255 {
            return Err(ApplyError::InvalidName);
        }
        if !result.is_empty() {
            result.push('\\');
        }
        result.push_str(&component.to_ascii_lowercase());
    }
    Ok(result)
}

fn value_name(name: &str) -> Result<String, ApplyError> {
    if !name.is_ascii() || name.len() > 16_383 || name.bytes().any(|b| b < 32) {
        return Err(ApplyError::InvalidName);
    }
    Ok(name.to_ascii_lowercase())
}

fn under(key: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || key == prefix
        || key
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('\\'))
}

fn join(base: &str, suffix: &str) -> Result<String, ApplyError> {
    let mut path = String::from(base);
    if !path.is_empty() && !suffix.is_empty() {
        path.push('\\');
    }
    path.push_str(suffix);
    if path.len() > MAX_PATH {
        return Err(ApplyError::Limit);
    }
    Ok(path)
}

type BoundPaths<'a> = Vec<(&'a RegistryBinding, String, String)>;
fn validate_bindings(bindings: &[RegistryBinding]) -> Result<BoundPaths<'_>, ApplyError> {
    let mut result: BoundPaths<'_> = Vec::new();
    for binding in bindings {
        if (binding.root == RegistryRoot::Relative) != binding.scope.is_some() {
            return Err(ApplyError::InvalidName);
        }
        let prefix = key_path(&binding.source_prefix)?;
        let destination = key_path(&binding.destination)?;
        if result.iter().any(|(prior, old, _)| {
            prior.root == binding.root
                && prior.scope == binding.scope
                && prior.view == binding.view
                && old == &prefix
        }) {
            return Err(ApplyError::AmbiguousBinding);
        }
        result.push((binding, prefix, destination));
    }
    Ok(result)
}

fn decode(data: &[u8]) -> Result<String, ApplyError> {
    if data.len() > MAX_BYTES || !data.len().is_multiple_of(2) {
        return Err(ApplyError::InvalidData);
    }
    let units: Vec<_> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .collect();
    String::from_utf16(&units).map_err(|_| ApplyError::InvalidData)
}

fn multi_strings(data: &[u8]) -> Result<Vec<String>, ApplyError> {
    let text = decode(data)?;
    let body = text.strip_suffix("\0\0").ok_or(ApplyError::InvalidData)?;
    if body.is_empty() {
        return Ok(Vec::new());
    }
    body.split('\0')
        .map(|part| {
            if part.is_empty() {
                Err(ApplyError::InvalidData)
            } else {
                Ok(String::from(part))
            }
        })
        .collect()
}

fn encode_multi(strings: &[String]) -> Vec<u8> {
    let mut units = Vec::new();
    for text in strings {
        units.extend(text.encode_utf16());
        units.push(0);
    }
    if units.is_empty() {
        units.push(0);
    }
    units.push(0);
    units.into_iter().flat_map(u16::to_le_bytes).collect()
}

fn require_ascii_comparison(strings: &[String]) -> Result<(), ApplyError> {
    if strings.iter().any(|s| !s.is_ascii()) {
        Err(ApplyError::UnsupportedComparison)
    } else {
        Ok(())
    }
}

fn resolve_value(
    value: &RegistryValue,
    directories: &[(u32, &str)],
) -> Result<StoredValue, ApplyError> {
    if value.data.len() > MAX_BYTES {
        return Err(ApplyError::Limit);
    }
    let data = match value.kind {
        1 | 2 | 7 => {
            let text = decode(&value.data)?;
            if value.kind == 7 {
                multi_strings(&value.data)?;
            } else if !text.ends_with('\0') || text[..text.len() - 1].contains('\0') {
                return Err(ApplyError::InvalidData);
            }
            // Inspect the actual data, not just the public parser's advisory
            // flag. A caller-created plan must not leave numeric DIRIDs hidden.
            let mut remaining = text.as_str();
            let mut output = String::new();
            while let Some(start) = remaining.find('%') {
                output.push_str(&remaining[..start]);
                remaining = &remaining[start..];
                let Some(end) = remaining[1..].find('%').map(|n| n + 1) else {
                    break;
                };
                let token = &remaining[1..end];
                if !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit()) {
                    let id = token
                        .parse::<u32>()
                        .map_err(|_| ApplyError::MissingDirectory)?;
                    let (_, directory) = directories
                        .iter()
                        .find(|(key, _)| *key == id)
                        .ok_or(ApplyError::MissingDirectory)?;
                    output.push_str(directory);
                } else {
                    output.push_str(&remaining[..=end]);
                }
                remaining = &remaining[end + 1..];
                if output.len() > MAX_BYTES / 2 {
                    return Err(ApplyError::Limit);
                }
            }
            output.push_str(remaining);
            let encoded: Vec<_> = output.encode_utf16().flat_map(u16::to_le_bytes).collect();
            if encoded.len() > MAX_BYTES {
                return Err(ApplyError::Limit);
            }
            encoded
        }
        4 if value.data.len() != 4 => return Err(ApplyError::InvalidData),
        _ if value.requires_directory_resolution => return Err(ApplyError::InvalidData),
        _ => value.data.clone(),
    };
    Ok(StoredValue {
        kind: value.kind,
        data,
    })
}
