//! Explicit, bounded Include/Needs graph over caller-supplied INF documents.
//! No filesystem lookup, trust decisions, or installation side effects.
use super::{filename, Error, Inf, InstallPlan, Model, Section, Target};
use alloc::{format, string::String, vec::Vec};

const MAX_INFS: usize = 64;
const MAX_SECTIONS: usize = 256;
const MAX_DEPTH: usize = 32;
const MAX_OPERATIONS: usize = 8192;

/// HKR has different meaning in DDInstall and DDInstall.HW. Keep the calling
/// context when following Needs; it cannot be inferred from the needed name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum InstallContext {
    DeviceSoftware,
    Services,
    DeviceHardware,
    CoInstallers,
    Interfaces,
}

/// One visited section, retaining its owning INF's Strings and explicit
/// DestinationDirs, and the caller's effective default destination.
/// Pending directives, including registry work, remain unexecuted.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PlannedSection {
    pub inf: String,
    pub context: InstallContext,
    /// Outermost calling INF providing DefaultDestDir, if any. Explicit
    /// DestinationDirs mappings in this section's owning INF take precedence.
    pub default_destination_inf: Option<String>,
    pub plan: InstallPlan,
}

/// Descriptive dependency graph in dependency-first traversal order. This is
/// not a SetupAPI execution order or an authenticated/installable package.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct DependencyPlan {
    pub root_inf: String,
    pub install_section: String,
    pub included_infs: Vec<String>,
    pub sections: Vec<PlannedSection>,
}

/// Closed INF namespace supplied by the caller. Only documents explicitly
/// named by a section's Include are eligible for its external Needs lookup.
/// Merely adding an INF here does not make its sections globally visible.
pub struct InfSet<'a> {
    pub(crate) documents: &'a [(&'a str, &'a Inf)],
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Visit<'a> {
    document: usize,
    section: &'a str,
    context: InstallContext,
    default_owner: Option<usize>,
}
struct Traversal<'a> {
    active: Vec<Visit<'a>>,
    completed: Vec<Visit<'a>>,
    operations: usize,
    plan: DependencyPlan,
}

impl<'a> InfSet<'a> {
    /// Reject duplicate case-insensitive names, paths, non-INFs and excessive
    /// namespaces. Document contents remain borrowed; this grants no trust.
    pub fn new(documents: &'a [(&'a str, &'a Inf)]) -> Result<Self, Error> {
        if documents.is_empty() || documents.len() > MAX_INFS {
            return Err(Error::Limit);
        }
        for (i, (name, _)) in documents.iter().enumerate() {
            inf_name(name)?;
            if documents[..i]
                .iter()
                .any(|(old, _)| old.eq_ignore_ascii_case(name))
            {
                return Err(Error::Ambiguous);
            }
        }
        Ok(Self { documents })
    }

    /// Resolve a selected model and its explicitly named Needs sections.
    /// Associated Services/HW/CoInstallers/Interfaces are visited only for the
    /// root DDInstall. A needed section does not implicitly import siblings.
    ///
    /// # Errors
    /// Returns selection errors, missing Includes/Needs, ambiguous external
    /// names, cycles and resource limits. No partial success is returned.
    pub fn resolve(
        &self,
        root: &str,
        model: &Model,
        target: Target,
        device_ids: &[&str],
    ) -> Result<DependencyPlan, Error> {
        let document = self.document(root)?;
        let (name, inf) = self.documents[document];
        inf.version(target)?;
        if !inf.matching_models(target, device_ids)?.contains(model) {
            return Err(Error::NoMatch);
        }
        let section = inf.install_section(&model.install_section, target)?;
        let mut traversal = Traversal {
            active: Vec::new(),
            completed: Vec::new(),
            operations: 0,
            plan: DependencyPlan {
                root_inf: String::from(name),
                install_section: section.name.clone(),
                included_infs: Vec::new(),
                sections: Vec::new(),
            },
        };
        self.visit(
            document,
            section,
            InstallContext::DeviceSoftware,
            None,
            device_ids,
            &mut traversal,
        )?;
        for (suffix, context) in [
            ("Services", InstallContext::Services),
            ("HW", InstallContext::DeviceHardware),
            ("CoInstallers", InstallContext::CoInstallers),
            ("Interfaces", InstallContext::Interfaces),
        ] {
            if let Some(associated) = inf.section(&format!("{}.{suffix}", section.name)) {
                self.visit(
                    document,
                    associated,
                    context,
                    None,
                    device_ids,
                    &mut traversal,
                )?;
            }
        }
        Ok(traversal.plan)
    }

    pub(crate) fn document(&self, name: &str) -> Result<usize, Error> {
        inf_name(name)?;
        self.documents
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(name))
            .ok_or(Error::MissingInclude)
    }

    fn visit(
        &self,
        document: usize,
        section: &'a Section,
        context: InstallContext,
        inherited_default: Option<usize>,
        device_ids: &[&str],
        traversal: &mut Traversal<'a>,
    ) -> Result<(), Error> {
        let (name, inf) = self.documents[document];
        let default_owner = inherited_default.or(inf.default_destination()?.map(|_| document));
        let visit = Visit {
            document,
            section: &section.name,
            context,
            default_owner,
        };
        if traversal.active.contains(&visit) {
            return Err(Error::DependencyCycle);
        }
        if traversal.completed.contains(&visit) {
            return Ok(());
        }
        if traversal.active.len() >= MAX_DEPTH
            || traversal.completed.len() + traversal.active.len() >= MAX_SECTIONS
        {
            return Err(Error::Limit);
        }
        traversal.active.push(visit);
        let mut plan = InstallPlan {
            install_section: section.name.clone(),
            copies: Vec::new(),
            services: Vec::new(),
            pending: Vec::new(),
        };
        let destination = default_owner
            .map(|owner| self.documents[owner].1.default_destination())
            .transpose()?
            .flatten();
        // Resolve the inherited default before parsing copies. An included
        // INF may legally omit its own default when the caller supplies it.
        inf.plan_section(section, device_ids, destination.as_ref(), &mut plan)?;
        traversal.operations += plan.copies.len() + plan.services.len() + plan.pending.len();
        if traversal.operations > MAX_OPERATIONS {
            return Err(Error::Limit);
        }

        let mut includes = Vec::new();
        for directive in &plan.pending {
            if directive.section.eq_ignore_ascii_case(&section.name)
                && directive.key.eq_ignore_ascii_case("Include")
            {
                for name in &directive.fields {
                    let index = self.document(name)?;
                    if !includes.contains(&index) {
                        includes.push(index);
                    }
                    let canonical = self.documents[index].0;
                    if !traversal
                        .plan
                        .included_infs
                        .iter()
                        .any(|n| n.eq_ignore_ascii_case(canonical))
                    {
                        traversal.plan.included_infs.push(String::from(canonical));
                    }
                }
            }
        }
        for directive in &plan.pending {
            if directive.section.eq_ignore_ascii_case(&section.name)
                && directive.key.eq_ignore_ascii_case("Needs")
            {
                for needed in &directive.fields {
                    if needed.is_empty() {
                        return Err(Error::Syntax);
                    }
                    // Local sections are authoritative. Otherwise require a
                    // unique owner among this section's explicit Includes;
                    // unrelated documents and prior visits cannot satisfy it.
                    let found = if let Some(local) = inf.section(needed) {
                        (document, local)
                    } else {
                        let mut found = None;
                        for &index in &includes {
                            if let Some(external) = self.documents[index].1.section(needed) {
                                if found.is_some() {
                                    return Err(Error::Ambiguous);
                                }
                                found = Some((index, external));
                            }
                        }
                        found.ok_or(Error::MissingSection)?
                    };
                    self.visit(
                        found.0,
                        found.1,
                        context,
                        default_owner,
                        device_ids,
                        traversal,
                    )?;
                }
            }
        }
        plan.pending.retain(|p| {
            !p.section.eq_ignore_ascii_case(&section.name)
                || (!p.key.eq_ignore_ascii_case("Include") && !p.key.eq_ignore_ascii_case("Needs"))
        });
        traversal.plan.sections.push(PlannedSection {
            inf: String::from(name),
            context,
            default_destination_inf: default_owner
                .map(|owner| String::from(self.documents[owner].0)),
            plan,
        });
        traversal.active.pop();
        traversal.completed.push(visit);
        Ok(())
    }
}

fn inf_name(name: &str) -> Result<(), Error> {
    filename(name)?;
    if !name.to_ascii_lowercase().ends_with(".inf") {
        return Err(Error::InvalidPath);
    }
    Ok(())
}
