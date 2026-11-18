//! Read-only AddReg/DelReg expansion. No registry handle, mutation or trust.
//! Values are UTF-16LE/little-endian templates; numeric DIRIDs still need the
//! destination environment. HKR scope must survive service/event-log traversal.
use super::{
    number_or_zero, valid_guid, Architecture, Error, Inf, InfSet, InstallContext, Model,
    PendingDirective, Target,
};
use alloc::{format, string::String, vec::Vec};

const MAX_OPERATIONS: usize = 8192;
const MAX_DATA: usize = 1024 * 1024;

/// The root in the INF, before any device/user-specific key is opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RegistryRoot {
    ClassesRoot,
    CurrentUser,
    LocalMachine,
    Users,
    Relative,
}

/// Meaning of HKR at the directive's call site, not at the registry list.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RegistryScope {
    DeviceSoftware,
    DeviceHardware,
    Service(String),
    EventLog(String),
    Interface {
        class_guid: String,
        reference: String,
    },
}

/// Native means the target environment's native view, never the host tool's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RegistryView {
    Native,
    Bits32,
    Bits64,
}

/// Conditional behavior for a future registry transaction implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum RegistryWriteMode {
    Replace,
    IfAbsent,
    IfPresent,
    AppendUnique,
}

/// Registry type number and encoded data, with unresolved numeric DIRID tokens
/// explicitly identified. REG_EXPAND_SZ environment variables remain literal.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct RegistryValue {
    pub kind: u32,
    pub data: Vec<u8>,
    pub requires_directory_resolution: bool,
}

/// Description only. A successful parser result cannot authorize these actions.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum RegistryAction {
    CreateKey,
    /// Omitted data is distinct from an explicitly empty string. Preserve it
    /// until original SetupAPI behavior has been established for this form.
    AddWithoutData {
        name: Option<String>,
        flags: u32,
    },
    SetValue {
        name: String,
        value: RegistryValue,
        mode: RegistryWriteMode,
    },
    DeleteKey,
    DeleteValue(String),
    DeleteMultiString {
        name: String,
        value: String,
    },
}

/// One operation retaining the original registry-list line and HKR call scope.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct RegistryOperation {
    pub inf: String,
    pub section: String,
    pub line: usize,
    pub scope: RegistryScope,
    pub root: RegistryRoot,
    pub subkey: String,
    pub view: RegistryView,
    /// SDDL text from the list's .security section. Not validated or applied.
    pub security_descriptor: Option<String>,
    pub action: RegistryAction,
}

/// An INF-provisioned interface identity. No runtime symbolic link is created
/// or enabled. Other interface-install work is retained explicitly as pending.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct InterfacePlan {
    pub inf: String,
    pub section: String,
    pub line: usize,
    pub class_guid: String,
    pub reference: String,
    pub install_section: Option<String>,
    pub pending: Vec<PendingDirective>,
}

/// Supplemental registry dependencies, not a complete or executable installer.
/// The ordinary dependency plan still retains every AddReg/DelReg as pending.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct RegistryPlan {
    pub root_inf: String,
    pub target_architecture: Architecture,
    pub operations: Vec<RegistryOperation>,
    pub interfaces: Vec<InterfacePlan>,
}

impl InfSet<'_> {
    /// Resolve Include/Needs and expand AddReg/DelReg in device software,
    /// hardware, co-installer, service-install and event-log sections. Service
    /// and event-log sections get separate HKR scopes even when lists repeat.
    /// AddInterface references get distinct GUID/reference-string HKR scopes;
    /// nonregistry work in interface-install lists is retained as pending.
    ///
    /// # Errors
    /// Rejects dependency errors, missing registry lists, unknown registry
    /// types/flags, malformed values and excessive operations/data. No partial
    /// result is returned. No registry, DriverStore or trust state is changed.
    pub fn resolve_registry(
        &self,
        root: &str,
        model: &Model,
        target: Target,
        device_ids: &[&str],
    ) -> Result<RegistryPlan, Error> {
        let dependency = self.resolve(root, model, target, device_ids)?;
        let mut plan = RegistryPlan {
            root_inf: dependency.root_inf,
            target_architecture: target.architecture,
            operations: Vec::new(),
            interfaces: Vec::new(),
        };
        let mut bytes = 0;
        for section in dependency.sections {
            let inf = self.documents[self.document(&section.inf)?].1;
            let scope = match section.context {
                InstallContext::DeviceSoftware | InstallContext::CoInstallers => {
                    Some(RegistryScope::DeviceSoftware)
                }
                InstallContext::DeviceHardware => Some(RegistryScope::DeviceHardware),
                InstallContext::Services | InstallContext::Interfaces => None,
            };
            inf.registry_directives(
                &section.inf,
                &section.plan.install_section,
                scope.as_ref(),
                &mut plan.operations,
                &mut bytes,
            )?;
            // Use AddService references, not flattened pending directives: one
            // list may occur under several distinct service/event-log keys.
            for service in section.plan.services {
                inf.registry_directives(
                    &section.inf,
                    &service.section,
                    Some(&RegistryScope::Service(service.name)),
                    &mut plan.operations,
                    &mut bytes,
                )?;
            }
            let owner = inf
                .section(&section.plan.install_section)
                .ok_or(Error::MissingSection)?;
            for entry in &owner.entries {
                if section.context == InstallContext::Interfaces
                    && entry
                        .key
                        .as_deref()
                        .is_some_and(|k| k.eq_ignore_ascii_case("AddInterface"))
                {
                    let fields = inf.fields(entry)?;
                    if fields.is_empty() || fields.len() > 4 || !valid_guid(&fields[0]) {
                        return Err(Error::Syntax);
                    }
                    if fields.get(3).map_or(Ok(0), |s| number_or_zero(s))? != 0 {
                        return Err(Error::UnsupportedDirective);
                    }
                    let class_guid = fields[0].to_ascii_lowercase();
                    let reference = fields.get(1).cloned().unwrap_or_default();
                    if reference.contains(['\\', '/']) {
                        return Err(Error::InvalidPath);
                    }
                    let install = fields.get(2).filter(|s| !s.is_empty());
                    let mut pending = Vec::new();
                    if let Some(install) = install {
                        inf.registry_directives(
                            &section.inf,
                            install,
                            Some(&RegistryScope::Interface {
                                class_guid: class_guid.clone(),
                                reference: reference.clone(),
                            }),
                            &mut plan.operations,
                            &mut bytes,
                        )?;
                        for directive in &inf.section(install).ok_or(Error::MissingSection)?.entries
                        {
                            let key = directive.key.as_deref().ok_or(Error::Syntax)?;
                            if !key.eq_ignore_ascii_case("AddReg")
                                && !key.eq_ignore_ascii_case("DelReg")
                            {
                                pending.push(PendingDirective {
                                    section: install.clone(),
                                    key: String::from(key),
                                    fields: inf.fields(directive)?,
                                });
                            }
                        }
                    }
                    bytes += fields.iter().map(String::len).sum::<usize>()
                        + pending
                            .iter()
                            .map(|p| {
                                p.section.len()
                                    + p.key.len()
                                    + p.fields.iter().map(String::len).sum::<usize>()
                            })
                            .sum::<usize>();
                    if plan.interfaces.len() >= 1024 || bytes > MAX_DATA {
                        return Err(Error::Limit);
                    }
                    plan.interfaces.push(InterfacePlan {
                        inf: section.inf.clone(),
                        section: owner.name.clone(),
                        line: entry.line,
                        class_guid,
                        reference,
                        install_section: install.cloned(),
                        pending,
                    });
                    continue;
                }
                if !entry
                    .key
                    .as_deref()
                    .is_some_and(|k| k.eq_ignore_ascii_case("AddService"))
                {
                    continue;
                }
                let fields = inf.fields(entry)?;
                if let Some(events) = fields.get(3).filter(|s| !s.is_empty()) {
                    inf.registry_directives(
                        &section.inf,
                        events,
                        Some(&RegistryScope::EventLog(fields[0].clone())),
                        &mut plan.operations,
                        &mut bytes,
                    )?;
                }
            }
        }
        Ok(plan)
    }
}

impl Inf {
    fn registry_directives(
        &self,
        inf_name: &str,
        section: &str,
        scope: Option<&RegistryScope>,
        operations: &mut Vec<RegistryOperation>,
        bytes: &mut usize,
    ) -> Result<(), Error> {
        let section = self.section(section).ok_or(Error::MissingSection)?;
        for directive in &section.entries {
            let key = directive.key.as_deref().ok_or(Error::Syntax)?;
            let add = key.eq_ignore_ascii_case("AddReg");
            if !add && !key.eq_ignore_ascii_case("DelReg") {
                // Include/Needs inside service-install lists is not part of
                // the resolved graph; do not silently claim that closure.
                if matches!(
                    scope,
                    Some(
                        RegistryScope::Service(_)
                            | RegistryScope::EventLog(_)
                            | RegistryScope::Interface { .. }
                    )
                ) && (key.eq_ignore_ascii_case("Include") || key.eq_ignore_ascii_case("Needs"))
                {
                    return Err(Error::UnsupportedDirective);
                }
                continue;
            }
            let scope = scope.ok_or(Error::UnsupportedDirective)?;
            for name in self.fields(directive)? {
                let list = self.section(&name).ok_or(Error::MissingSection)?;
                let security = if add {
                    self.registry_security(&name)?
                } else {
                    None
                };
                for entry in &list.entries {
                    if entry.key.is_some() {
                        return Err(Error::Syntax);
                    }
                    let fields = self.fields(entry)?;
                    if fields.len() < 2 {
                        return Err(Error::Syntax);
                    }
                    let root = match fields[0].to_ascii_uppercase().as_str() {
                        "HKCR" => RegistryRoot::ClassesRoot,
                        "HKCU" => RegistryRoot::CurrentUser,
                        "HKLM" => RegistryRoot::LocalMachine,
                        "HKU" => RegistryRoot::Users,
                        "HKR" => RegistryRoot::Relative,
                        _ => return Err(Error::UnsupportedDirective),
                    };
                    // Registry key names are not filenames: asterisks, dots
                    // and repeated separators occur in original SP1 INFs.
                    let flags = fields.get(3).map_or(Ok(0), |s| number_or_zero(s))?;
                    let view = view(flags)?;
                    let action = if add {
                        add_action(&fields, flags)?
                    } else {
                        del_action(&fields, flags)?
                    };
                    *bytes += fields.iter().map(String::len).sum::<usize>()
                        + security.as_ref().map_or(0, String::len)
                        + match &action {
                            RegistryAction::SetValue { value, .. } => value.data.len(),
                            _ => 0,
                        };
                    if operations.len() >= MAX_OPERATIONS || *bytes > MAX_DATA {
                        return Err(Error::Limit);
                    }
                    operations.push(RegistryOperation {
                        inf: String::from(inf_name),
                        section: list.name.clone(),
                        line: entry.line,
                        scope: scope.clone(),
                        root,
                        subkey: fields[1].clone(),
                        view,
                        security_descriptor: security.clone(),
                        action,
                    });
                }
            }
        }
        Ok(())
    }

    fn registry_security(&self, name: &str) -> Result<Option<String>, Error> {
        let Some(section) = self.section(&format!("{name}.security")) else {
            return Ok(None);
        };
        if section.entries.len() != 1 || section.entries[0].key.is_some() {
            return Err(Error::Syntax);
        }
        let sddl = self.one_field(&section.entries[0])?;
        if sddl.is_empty() {
            return Err(Error::Syntax);
        }
        Ok(Some(sddl))
    }
}

fn view(flags: u32) -> Result<RegistryView, Error> {
    match flags & 0x5000 {
        0 => Ok(RegistryView::Native),
        0x1000 => Ok(RegistryView::Bits64),
        0x4000 => Ok(RegistryView::Bits32),
        _ => Err(Error::Syntax),
    }
}

fn add_action(fields: &[String], flags: u32) -> Result<RegistryAction, Error> {
    if flags & 0x0000_8fc0 != 0 {
        return Err(Error::UnsupportedDirective);
    }
    let name = fields.get(2).cloned().unwrap_or_default();
    if flags & 0x2010 != 0 {
        // KEYONLY ignores the supplied value name and data by definition.
        return Ok(RegistryAction::CreateKey);
    }
    if flags & 4 != 0 {
        return Ok(if name.is_empty() {
            RegistryAction::DeleteKey
        } else {
            RegistryAction::DeleteValue(name)
        });
    }
    let mode = match flags & 0x2a {
        0 => RegistryWriteMode::Replace,
        2 => RegistryWriteMode::IfAbsent,
        0x20 => RegistryWriteMode::IfPresent,
        8 if flags & 0xffff_0001 == 0x10000 => RegistryWriteMode::AppendUnique,
        _ => return Err(Error::UnsupportedDirective),
    };
    let data = fields.get(4..).unwrap_or_default();
    if data.is_empty() {
        return Ok(RegistryAction::AddWithoutData {
            name: fields.get(2).cloned(),
            flags,
        });
    }
    let mut encoded = Vec::new();
    let kind = match flags & 0xffff_0001 {
        0 | 0x20000 => {
            if data.len() > 1 {
                return Err(Error::Syntax);
            }
            utf16(&mut encoded, data.first().map_or("", String::as_str));
            if flags & 0x20000 == 0 {
                1
            } else {
                2
            }
        }
        0x10000 => {
            // Empty elements cannot be represented inside REG_MULTI_SZ.
            if data.iter().any(String::is_empty) && data.len() > 1 {
                return Err(Error::Syntax);
            }
            for value in data.iter().filter(|s| !s.is_empty()) {
                utf16(&mut encoded, value);
            }
            if encoded.is_empty() {
                encoded.extend_from_slice(&[0, 0]);
            }
            encoded.extend_from_slice(&[0, 0]);
            7
        }
        0x10001 => {
            if data.len() != 1 {
                return Err(Error::Syntax);
            }
            if data[0].is_empty() {
                return Err(Error::Syntax);
            }
            encoded.extend_from_slice(&number_or_zero(&data[0])?.to_le_bytes());
            4
        }
        // Explicit binary REG_EXPAND_SZ/MULTI_SZ/SZ encodings are rejected;
        // those registry types have their documented string-only INF forms.
        value if value & 1 == 1 => {
            let kind = match value {
                1 => 3,
                0x20001 => 0,
                other => other >> 16,
            };
            // QWORD's numeric/binary input convention is not validated against
            // the original SP1 installer, so leave it explicitly unsupported.
            if [1, 2, 7, 11].contains(&kind) {
                return Err(Error::UnsupportedDirective);
            }
            for byte in data {
                encoded.push(u8::from_str_radix(byte, 16).map_err(|_| Error::Syntax)?);
            }
            kind
        }
        _ => return Err(Error::UnsupportedDirective),
    };
    Ok(RegistryAction::SetValue {
        name,
        value: RegistryValue {
            kind,
            data: encoded,
            requires_directory_resolution: data.iter().any(|s| contains_dirid(s)),
        },
        mode,
    })
}

fn del_action(fields: &[String], flags: u32) -> Result<RegistryAction, Error> {
    // Supported NT 6.1 DelReg forms; newer property/registry flags are errors.
    let behavior = flags & !0x5000;
    let name = fields.get(2).cloned().unwrap_or_default();
    match behavior {
        0 | 0x2000 if fields.len() <= 4 => Ok(if name.is_empty() || behavior == 0x2000 {
            RegistryAction::DeleteKey
        } else {
            RegistryAction::DeleteValue(name)
        }),
        0x18002 if fields.len() == 5 && !name.is_empty() && !fields[4].is_empty() => {
            Ok(RegistryAction::DeleteMultiString {
                name,
                value: fields[4].clone(),
            })
        }
        _ => Err(Error::UnsupportedDirective),
    }
}

fn utf16(out: &mut Vec<u8>, value: &str) {
    for unit in value.encode_utf16().chain(core::iter::once(0)) {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

fn contains_dirid(value: &str) -> bool {
    value
        .split('%')
        .enumerate()
        .any(|(index, s)| index % 2 == 1 && !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}
