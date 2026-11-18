//! Bounded, read-only Windows 7 INF and installation dependency resolution.
//!
//! This module describes the operations an installer must perform. It does
//! not install a driver, execute co-installers, authenticate an INF, or grant
//! trust to a package whose CatalogFile is absent. In-box SP1 INFs can omit
//! CatalogFile; that is an unresolved trust dependency, never an exemption.
//!
//! Format references: Microsoft INF syntax, Manufacturer/Models sections,
//! DDInstall, DDInstall.Services, CopyFiles, DestinationDirs and AddService.
#![no_std]
extern crate alloc;
use alloc::{format, string::String, vec::Vec};

mod inf;
pub use inf::{Entry, Error, Inf, Section};
mod dependency;
pub use dependency::{DependencyPlan, InfSet, InstallContext, PlannedSection};
mod registry;
pub mod registry_apply;
pub use registry::{
    InterfacePlan, RegistryAction, RegistryOperation, RegistryPlan, RegistryRoot, RegistryScope,
    RegistryValue, RegistryView, RegistryWriteMode,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Architecture {
    X86,
    Amd64,
}
impl Architecture {
    fn decoration(self) -> &'static str {
        match self {
            Self::X86 => "ntx86",
            Self::Amd64 => "ntamd64",
        }
    }
}

/// Explicit SP1 target; product type and suite affect Manufacturer selection.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Target {
    pub architecture: Architecture,
    pub product_type: u32,
    pub suite_mask: u32,
}
impl Target {
    pub const SP1_AMD64: Self = Self {
        architecture: Architecture::Amd64,
        product_type: 1,
        suite_mask: 0,
    };
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Version {
    pub class: String,
    pub class_guid: String,
    pub provider: String,
    pub driver_date: String,
    pub driver_version: [u16; 4],
    /// Absence is retained for in-box packages; it never means unsigned is OK.
    pub catalog: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Model {
    pub manufacturer: String,
    pub description: String,
    pub models_section: String,
    pub install_section: String,
    /// Slot zero is the INF hardware ID, even if empty (as in HDAUDIO.INF).
    pub ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct CopyFile {
    pub source: String,
    pub destination: String,
    pub directory_id: u32,
    pub subdirectory: String,
    pub flags: u32,
    /// False for a named DestinationDirs entry; true for DefaultDestDir.
    /// Include/Needs can override only the latter via the calling INF.
    pub uses_default_destination: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Service {
    pub name: String,
    pub flags: u32,
    pub section: String,
    pub service_type: u32,
    pub start_type: u32,
    pub error_control: u32,
    /// DIRID tokens remain explicit; no host filesystem path is substituted.
    pub binary: String,
    pub load_order_group: Option<String>,
}

/// Unexecuted directives are retained with their section and original fields.
/// In particular AddReg/Include/Needs/CoInstallers must not silently disappear.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PendingDirective {
    pub section: String,
    pub key: String,
    pub fields: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct InstallPlan {
    pub install_section: String,
    pub copies: Vec<CopyFile>,
    pub services: Vec<Service>,
    pub pending: Vec<PendingDirective>,
}

impl Inf {
    pub fn version(&self, target: Target) -> Result<Version, Error> {
        let signature = self.required("Version", "Signature")?;
        if !signature.eq_ignore_ascii_case("$Windows NT$") {
            return Err(Error::InvalidVersion);
        }
        let driver = self
            .single("Version", "DriverVer")?
            .ok_or(Error::MissingKey)?;
        let fields = self.fields(driver)?;
        if fields.len() != 2 || !valid_date(&fields[0]) {
            return Err(Error::InvalidVersion);
        }
        let mut driver_version = [0; 4];
        let parts: Vec<_> = fields[1].split('.').collect();
        if parts.len() != 4 {
            return Err(Error::InvalidVersion);
        }
        for (out, part) in driver_version.iter_mut().zip(parts) {
            *out = part.parse().map_err(|_| Error::InvalidVersion)?;
        }
        let mut catalog = None;
        // CatalogFile permits architecture, NT, and undecorated spellings.
        for key in [
            format!("CatalogFile.{}", target.architecture.decoration()),
            String::from("CatalogFile.nt"),
            String::from("CatalogFile"),
        ] {
            if let Some(entry) = self.single("Version", &key)? {
                let name = self.one_field(entry)?;
                filename(&name)?;
                if !name.to_ascii_lowercase().ends_with(".cat") {
                    return Err(Error::InvalidPath);
                }
                catalog = Some(name);
                break;
            }
        }
        let class_guid = self.required("Version", "ClassGuid")?;
        if !valid_guid(&class_guid) {
            return Err(Error::InvalidVersion);
        }
        Ok(Version {
            class: self.required("Version", "Class")?,
            class_guid,
            provider: self.required("Version", "Provider")?,
            driver_date: fields[0].clone(),
            driver_version,
            catalog,
        })
    }

    /// Enumerate models from the best applicable section of each manufacturer.
    /// This is not a complete PnP driver ranking or signature-policy decision.
    pub fn models(&self, target: Target) -> Result<Vec<Model>, Error> {
        let section = self.section("Manufacturer").ok_or(Error::MissingSection)?;
        let mut models = Vec::new();
        for manufacturer in &section.entries {
            let key = manufacturer.key.as_deref().ok_or(Error::Syntax)?;
            let manufacturer_name = self.expand(key)?;
            let fields = self.fields(manufacturer)?;
            let base = fields
                .first()
                .filter(|s| !s.is_empty())
                .ok_or(Error::Syntax)?;
            let mut best: Option<([u32; 4], String)> = None;
            if fields.len() == 1 {
                // Undecorated Manufacturer entry refers to its base Models.
                best = Some(([0; 4], base.clone()));
            } else {
                for decoration in &fields[1..] {
                    if let Some(rank) = decoration_rank(decoration, target)? {
                        let name = format!("{base}.{decoration}");
                        match &best {
                            Some((prior, old))
                                if *prior == rank && !old.eq_ignore_ascii_case(&name) =>
                            {
                                return Err(Error::Ambiguous);
                            }
                            Some((prior, _)) if *prior >= rank => (),
                            _ => best = Some((rank, name)),
                        }
                    }
                }
            }
            let Some((_, name)) = best else { continue };
            let model_section = self.section(&name).ok_or(Error::MissingSection)?;
            for entry in &model_section.entries {
                let description = self.expand(entry.key.as_deref().ok_or(Error::Syntax)?)?;
                let fields = self.fields(entry)?;
                if fields.len() < 2
                    || fields[0].is_empty()
                    || fields[1..].iter().all(String::is_empty)
                {
                    return Err(Error::Syntax);
                }
                models.push(Model {
                    manufacturer: manufacturer_name.clone(),
                    description,
                    models_section: model_section.name.clone(),
                    install_section: fields[0].clone(),
                    ids: fields[1..].to_vec(),
                });
                if models.len() > 8192 {
                    return Err(Error::Limit);
                }
            }
        }
        Ok(models)
    }

    /// Exact, case-insensitive PnP ID matching. Returns every candidate, without
    /// claiming Windows' driver rank or applying ExcludeID until planning.
    pub fn matching_models(
        &self,
        target: Target,
        device_ids: &[&str],
    ) -> Result<Vec<Model>, Error> {
        Ok(self
            .models(target)?
            .into_iter()
            .filter(|m| {
                m.ids.iter().any(|id| {
                    !id.is_empty() && device_ids.iter().any(|d| id.eq_ignore_ascii_case(d))
                })
            })
            .collect())
    }

    /// Resolve direct CopyFiles/AddService dependencies. All other operations
    /// remain pending, including Include/Needs and service/event registry work.
    /// An install plan is descriptive; neither CAT trust nor install success.
    pub fn plan(
        &self,
        model: &Model,
        target: Target,
        device_ids: &[&str],
    ) -> Result<InstallPlan, Error> {
        // A caller cannot manufacture an arbitrary model to escape selection.
        if !self.matching_models(target, device_ids)?.contains(model) {
            return Err(Error::NoMatch);
        }
        let section = self.install_section(&model.install_section, target)?;
        let mut plan = InstallPlan {
            install_section: section.name.clone(),
            copies: Vec::new(),
            services: Vec::new(),
            pending: Vec::new(),
        };
        self.plan_section(section, device_ids, None, &mut plan)?;
        for suffix in ["Services", "HW", "CoInstallers", "Interfaces"] {
            if let Some(associated) = self.section(&format!("{}.{suffix}", section.name)) {
                self.plan_section(associated, device_ids, None, &mut plan)?;
            }
        }
        Ok(plan)
    }

    fn install_section(&self, base: &str, target: Target) -> Result<&Section, Error> {
        for name in [
            format!("{base}.{}", target.architecture.decoration()),
            format!("{base}.NT"),
            String::from(base),
        ] {
            if let Some(section) = self.section(&name) {
                return Ok(section);
            }
        }
        Err(Error::MissingSection)
    }

    fn plan_section(
        &self,
        section: &Section,
        device_ids: &[&str],
        inherited_destination: Option<&(u32, String)>,
        plan: &mut InstallPlan,
    ) -> Result<(), Error> {
        for entry in &section.entries {
            let key = entry.key.as_deref().ok_or(Error::Syntax)?;
            let fields = self.fields(entry)?;
            if key.eq_ignore_ascii_case("ExcludeId")
                && fields.iter().any(|id| {
                    device_ids
                        .iter()
                        .any(|device| id.eq_ignore_ascii_case(device))
                })
            {
                return Err(Error::Excluded);
            }
            if key.eq_ignore_ascii_case("CopyFiles") {
                for reference in &fields {
                    if let Some(name) = reference.strip_prefix('@') {
                        plan.copies.push(self.copy_file(
                            None,
                            &[String::from(name)],
                            inherited_destination,
                        )?);
                    } else {
                        let copy_section = self.section(reference).ok_or(Error::MissingSection)?;
                        for file in &copy_section.entries {
                            if file.key.is_some() {
                                return Err(Error::Syntax);
                            }
                            plan.copies.push(self.copy_file(
                                Some(reference),
                                &self.fields(file)?,
                                inherited_destination,
                            )?);
                        }
                    }
                    if plan.copies.len() > 8192 {
                        return Err(Error::Limit);
                    }
                }
            } else if key.eq_ignore_ascii_case("AddService") {
                if fields.len() < 3 || fields.len() > 5 {
                    return Err(Error::Syntax);
                }
                let flags = number_or_zero(&fields[1])?;
                let service_section = self.section(&fields[2]).ok_or(Error::MissingSection)?;
                let service = Service {
                    name: fields[0].clone(),
                    flags,
                    section: service_section.name.clone(),
                    service_type: number(&self.required(&fields[2], "ServiceType")?)?,
                    start_type: number(&self.required(&fields[2], "StartType")?)?,
                    error_control: number(&self.required(&fields[2], "ErrorControl")?)?,
                    binary: self.required(&fields[2], "ServiceBinary")?,
                    load_order_group: self.optional(&fields[2], "LoadOrderGroup")?,
                };
                if service.name.is_empty() || service.start_type > 4 || service.error_control > 3 {
                    return Err(Error::Syntax);
                }
                for operation in &service_section.entries {
                    let key = operation.key.as_deref().ok_or(Error::Syntax)?;
                    if ![
                        "ServiceType",
                        "StartType",
                        "ErrorControl",
                        "ServiceBinary",
                        "LoadOrderGroup",
                    ]
                    .iter()
                    .any(|known| key.eq_ignore_ascii_case(known))
                    {
                        self.pending(service_section, operation, plan)?;
                    }
                }
                if let Some(event_section) = fields.get(3).filter(|s| !s.is_empty()) {
                    let events = self.section(event_section).ok_or(Error::MissingSection)?;
                    for operation in &events.entries {
                        self.pending(events, operation, plan)?;
                    }
                }
                // The optional event-log flags are data for the future installer.
                self.pending(section, entry, plan)?;
                plan.services.push(service);
            } else {
                self.pending(section, entry, plan)?;
            }
        }
        Ok(())
    }

    fn pending(
        &self,
        section: &Section,
        entry: &Entry,
        plan: &mut InstallPlan,
    ) -> Result<(), Error> {
        if plan.pending.len() >= 8192 {
            return Err(Error::Limit);
        }
        plan.pending.push(PendingDirective {
            section: section.name.clone(),
            key: entry.key.clone().ok_or(Error::Syntax)?,
            fields: self.fields(entry)?,
        });
        Ok(())
    }

    fn copy_file(
        &self,
        section: Option<&str>,
        fields: &[String],
        inherited_destination: Option<&(u32, String)>,
    ) -> Result<CopyFile, Error> {
        if fields.is_empty() || fields.len() > 4 {
            return Err(Error::Syntax);
        }
        let destination = &fields[0];
        let source = fields
            .get(1)
            .filter(|s| !s.is_empty())
            .unwrap_or(destination);
        filename(source)?;
        filename(destination)?;
        if fields.get(2).is_some_and(|s| !s.is_empty()) {
            // Reserved temporary filename is not an installable source path.
            return Err(Error::UnsupportedDirective);
        }
        let flags = fields.get(3).map_or(Ok(0), |f| number_or_zero(f))?;
        let specific_destination = section
            .map(|s| self.single("DestinationDirs", s))
            .transpose()?
            .flatten();
        let uses_default_destination = specific_destination.is_none();
        let (directory_id, subdirectory) = if let Some(entry) = specific_destination {
            self.destination(entry)?
        } else if let Some(destination) = inherited_destination {
            destination.clone()
        } else {
            self.default_destination()?
                .ok_or(Error::MissingDestination)?
        };
        Ok(CopyFile {
            source: source.clone(),
            destination: destination.clone(),
            directory_id,
            subdirectory,
            flags,
            uses_default_destination,
        })
    }

    fn default_destination(&self) -> Result<Option<(u32, String)>, Error> {
        self.single("DestinationDirs", "DefaultDestDir")?
            .map(|entry| self.destination(entry))
            .transpose()
    }

    fn destination(&self, entry: &Entry) -> Result<(u32, String), Error> {
        let dirs = self.fields(entry)?;
        if dirs.is_empty() || dirs.len() > 2 {
            return Err(Error::Syntax);
        }
        let subdirectory = dirs.get(1).cloned().unwrap_or_default();
        relative_directory(&subdirectory)?;
        Ok((number(&dirs[0])?, subdirectory))
    }
}

fn number_or_zero(s: &str) -> Result<u32, Error> {
    if s.is_empty() {
        Ok(0)
    } else {
        number(s)
    }
}
fn number(s: &str) -> Result<u32, Error> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).map_err(|_| Error::Syntax)
    } else {
        s.parse().map_err(|_| Error::Syntax)
    }
}

/// TargetOSVersion decorations available to NT 6.1 (no Windows 10 build field).
fn decoration_rank(value: &str, target: Target) -> Result<Option<[u32; 4]>, Error> {
    let low = value.to_ascii_lowercase();
    let parts: Vec<_> = low.split('.').collect();
    if parts[0] != target.architecture.decoration() && parts[0] != "nt" {
        return Ok(None);
    }
    if parts.len() > 5 {
        return Err(Error::UnsupportedTarget);
    }
    let mut version = [0; 4];
    for (index, part) in parts[1..].iter().enumerate() {
        if !part.is_empty() {
            version[index] = number(part)?;
        } else if index < 2 {
            return Err(Error::Syntax);
        }
    }
    if (version[0], version[1]) > (6, 1) {
        return Ok(None);
    }
    if version[2] != 0 && version[2] != target.product_type {
        return Ok(None);
    }
    if version[3] & target.suite_mask != version[3] {
        return Ok(None);
    }
    // Version first, then explicit product/suite specificity. Architecture
    // specific entries take precedence over generic NT at equal specificity.
    Ok(Some([
        version[0],
        version[1],
        u32::from(version[2] != 0) * 2 + u32::from(version[3] != 0),
        u32::from(parts[0] != "nt"),
    ]))
}

fn filename(s: &str) -> Result<(), Error> {
    if s.is_empty()
        || s.len() > 255
        || s.ends_with(['.', ' '])
        || s.bytes().any(|b| b < 32 || b"\\/:*?\"<>|%".contains(&b))
        || s == "."
        || s == ".."
    {
        return Err(Error::InvalidPath);
    }
    // Windows aliases must not become host device names during future staging.
    let base = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL"].contains(&base.as_str())
        || ((base.starts_with("COM") || base.starts_with("LPT"))
            && base.len() == 4
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(Error::InvalidPath);
    }
    Ok(())
}
fn relative_directory(s: &str) -> Result<(), Error> {
    if s.is_empty() {
        return Ok(());
    }
    for part in s.split('\\') {
        filename(part)?;
    }
    Ok(())
}
fn valid_guid(s: &str) -> bool {
    s.len() == 38
        && s.starts_with('{')
        && s.ends_with('}')
        && s.as_bytes()[1..37].iter().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                *b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
fn valid_date(s: &str) -> bool {
    let parts: Vec<_> = s.split('/').collect();
    if parts.len() != 3 {
        return false;
    }
    let (Ok(m), Ok(d), Ok(y)) = (
        parts[0].parse::<u32>(),
        parts[1].parse::<u32>(),
        parts[2].parse::<u32>(),
    ) else {
        return false;
    };
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = match m {
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        1..=12 => 31,
        _ => 0,
    };
    (1601..=9999).contains(&y) && d > 0 && d <= days
}
