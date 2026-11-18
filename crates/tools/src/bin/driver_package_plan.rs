//! INF inspection and optional NEW offline registry fragment export.
use clap::Parser;
use nt61_driver_package::registry_apply::{RegistryBinding, RegistryImage};
use nt61_driver_package::{
    Architecture, DependencyPlan, Inf, InfSet, InstallPlan, Model, RegistryPlan, Target, Version,
};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::PathBuf};

#[derive(Parser)]
#[command(
    about = "Inspect Windows 7 SP1 INF dependencies; does not install or authenticate a driver"
)]
struct Args {
    #[arg(long)]
    inf: PathBuf,
    #[arg(long, value_parser = ["amd64", "x86"], default_value = "amd64")]
    arch: String,
    #[arg(long = "hardware-id", action = clap::ArgAction::Append, conflicts_with = "all_models")]
    hardware_ids: Vec<String>,
    #[arg(long)]
    all_models: bool,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=3))]
    product_type: u32,
    #[arg(long, default_value_t = 0)]
    suite_mask: u32,
    /// Resolve explicit Include/Needs edges using only the supplied documents.
    #[arg(long)]
    resolve_dependencies: bool,
    /// Expand supported AddReg/DelReg lists with typed data and HKR scopes.
    #[arg(long, requires = "resolve_dependencies")]
    resolve_registry: bool,
    #[arg(long = "include-inf", action = clap::ArgAction::Append, requires = "resolve_dependencies")]
    include_infs: Vec<PathBuf>,
    /// Write a new supplemental REGF fragment, never replace an existing hive.
    #[arg(long, requires_all = ["resolve_registry", "registry_bindings"], conflicts_with = "all_models")]
    export_registry_hive: Option<PathBuf>,
    /// JSON with explicit root/view/HKR bindings and DIRID directory mappings.
    #[arg(long, requires = "export_registry_hive")]
    registry_bindings: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingConfig {
    bindings: Vec<RegistryBinding>,
    directories: Vec<Directory>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Directory {
    id: u32,
    path: String,
}

#[derive(Serialize)]
struct HiveExport {
    path: PathBuf,
    bytes: usize,
    keys: usize,
    values: usize,
    changed_operations: usize,
    unchanged_operations: usize,
    complete_installation: bool,
}

#[derive(Serialize)]
struct Selection {
    model: Model,
    plan: InstallPlan,
    dependency_plan: Option<DependencyPlan>,
    registry_plan: Option<RegistryPlan>,
}

#[derive(Serialize)]
struct Report {
    schema: u32,
    inf_path: PathBuf,
    include_paths: Vec<PathBuf>,
    target: Target,
    version: Version,
    available_models: usize,
    selected: Vec<Selection>,
    package_authenticated: bool,
    installation_executed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    registry_hive_export: Option<HiveExport>,
    limitations: &'static [&'static str],
}

fn read_inf(path: &std::path::Path) -> Result<Inf, String> {
    // Bound the read itself; do not allocate an unbounded file before parsing.
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| format!("open INF: {e}"))?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read INF: {e}"))?;
    Inf::parse(&bytes).map_err(|e| format!("parse INF {}: {e:?}", path.display()))
}

fn inspect(args: Args) -> Result<Report, String> {
    if args.include_infs.len() > 63 {
        return Err("too many included INFs".into());
    }
    let inf = read_inf(&args.inf)?;
    let mut included = Vec::new();
    for path in &args.include_infs {
        included.push(read_inf(path)?);
    }
    let name = |path: &std::path::Path| {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(str::to_owned)
            .ok_or_else(|| "INF needs a Unicode filename".to_owned())
    };
    let root_name = name(&args.inf)?;
    let include_names = args
        .include_infs
        .iter()
        .map(|p| name(p))
        .collect::<Result<Vec<_>, _>>()?;
    let mut documents = vec![(root_name.as_str(), &inf)];
    documents.extend(
        include_names
            .iter()
            .zip(&included)
            .map(|(n, i)| (n.as_str(), i)),
    );
    let set = InfSet::new(&documents).map_err(|e| format!("INF namespace: {e:?}"))?;
    let target = Target {
        architecture: if args.arch == "amd64" {
            Architecture::Amd64
        } else {
            Architecture::X86
        },
        product_type: args.product_type,
        suite_mask: args.suite_mask,
    };
    let version = inf
        .version(target)
        .map_err(|e| format!("INF Version: {e:?}"))?;
    let available = inf
        .models(target)
        .map_err(|e| format!("INF Models: {e:?}"))?;
    let ids: Vec<_> = args.hardware_ids.iter().map(String::as_str).collect();
    let models = if args.all_models {
        available.clone()
    } else {
        inf.matching_models(target, &ids)
            .map_err(|e| format!("device matching: {e:?}"))?
    };
    if !ids.is_empty() && models.is_empty() {
        return Err("no matching INF model".into());
    }
    let mut selected = Vec::new();
    for model in models {
        let model_ids: Vec<_> = model.ids.iter().map(String::as_str).collect();
        let plan = inf
            .plan(
                &model,
                target,
                if args.all_models { &model_ids } else { &ids },
            )
            .map_err(|e| format!("DDInstall {}: {e:?}", model.install_section))?;
        let dependency_plan = if args.resolve_dependencies {
            Some(
                set.resolve(
                    &root_name,
                    &model,
                    target,
                    if args.all_models { &model_ids } else { &ids },
                )
                .map_err(|e| format!("dependencies {}: {e:?}", model.install_section))?,
            )
        } else {
            None
        };
        let registry_plan = if args.resolve_registry {
            Some(
                set.resolve_registry(
                    &root_name,
                    &model,
                    target,
                    if args.all_models { &model_ids } else { &ids },
                )
                .map_err(|e| format!("registry {}: {e:?}", model.install_section))?,
            )
        } else {
            None
        };
        selected.push(Selection {
            model,
            plan,
            dependency_plan,
            registry_plan,
        });
    }
    let registry_hive_export = if let Some(path) = args.export_registry_hive {
        if selected.len() != 1 {
            return Err("registry export requires exactly one selected model".into());
        }
        let config_path = args.registry_bindings.ok_or("missing registry bindings")?;
        let mut bytes = Vec::new();
        std::fs::File::open(config_path)
            .map_err(|e| format!("open bindings: {e}"))?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("read bindings: {e}"))?;
        if bytes.len() > 1024 * 1024 {
            return Err("bindings exceed 1 MiB".into());
        }
        let config: BindingConfig =
            serde_json::from_slice(&bytes).map_err(|e| format!("parse bindings: {e}"))?;
        let directories: Vec<_> = config
            .directories
            .iter()
            .map(|d| (d.id, d.path.as_str()))
            .collect();
        let plan = selected[0]
            .registry_plan
            .as_ref()
            .ok_or("registry plan required")?;
        let mut image = RegistryImage::default();
        let summary = image
            .apply(plan, &config.bindings, &directories)
            .map_err(|e| format!("apply offline registry: {e:?}"))?;
        let bytes = nt61_tools::registry_hive::write_registry_hive(&image, &path)
            .map_err(|e| format!("export offline hive: {e}"))?;
        Some(HiveExport {
            path,
            bytes,
            keys: image.keys().count(),
            values: image.keys().map(|(_, v)| v.len()).sum(),
            changed_operations: summary.changed,
            unchanged_operations: summary.unchanged,
            complete_installation: false,
        })
    } else {
        None
    };
    Ok(Report {
        schema: 4, inf_path: args.inf, include_paths: args.include_infs, target, version, available_models: available.len(), selected,
        package_authenticated: false, installation_executed: false,
        registry_hive_export,
        limitations: &[
            "Partial INF resolver; pending directives require implementation before installation.",
            "Missing CatalogFile in an in-box INF is unresolved trust, not permission to install unsigned.",
            "Include/Needs graph resolution is optional and descriptive; no SetupAPI execution order is claimed.",
            "No Windows PnP ranking, source media resolution, live registry mutation or installation rollback.",
            "Optional registry plan is supplemental; AddReg/DelReg and AddInterface remain pending in the dependency plan. No SDDL validation or runtime interface creation; omitted data and DIRIDs remain explicit.",
            "No INF/CAT membership, signature-policy decision, driver image loading or WDM execution.",
            "--all-models inspects each model separately; actual device selection must supply all hardware/compatible IDs.",
            "Optional REGF export starts from an empty image with explicit bindings; it cannot modify an existing hive, preserve its ACLs or represent a complete installation. Unsupported security, interfaces, omitted data or names fail the whole export.",
        ],
    })
}

fn main() {
    match inspect(Args::parse()) {
        Ok(report) => {
            if let Err(error) = serde_json::to_writer_pretty(std::io::stdout().lock(), &report) {
                eprintln!("write report: {error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
