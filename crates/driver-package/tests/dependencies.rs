//! Cross-INF behavior tests. No test grants trust or mutates installed state.
use nt61_driver_package::{Error, Inf, InfSet, InstallContext, Target};
const TARGET: Target = Target::SP1_AMD64;

fn root(extra: &str) -> Inf {
    Inf::parse(
        format!(
            r#"[Version]
Signature="$Windows NT$"
Class=Test
ClassGuid={{4d36e967-e325-11ce-bfc1-08002be10318}}
Provider=Test
DriverVer=11/19/2010,6.1.7601.17514
[Manufacturer]
v=Models,NTamd64
[Models.NTamd64]
x=Install,ROOT\TEST
[Install.NT]
{extra}
"#
        )
        .as_bytes(),
    )
    .unwrap()
}
fn fragment(text: &str) -> Inf {
    Inf::parse(text.as_bytes()).unwrap()
}

#[test]
fn nested_needs_preserve_origin_strings_destinations_and_hkr_context() {
    let root =
        root("Include=base.inf\nNeeds=Base\n[Install.NT.HW]\nInclude=base.inf\nNeeds=Base\n");
    let base = fragment(
        r#"[Base]
Include=leaf.inf
Needs=Leaf
CopyFiles=files
AddReg=base.reg
[Strings]
name=base.sys
[files]
%name%
[DestinationDirs]
files=12,base
[Base.Services]
AddService=MustNotBeImplicitlyIncluded
"#,
    );
    let leaf = fragment("[Leaf]\nCopyFiles=@leaf.sys\n[DestinationDirs]\nDefaultDestDir=11\n");
    let documents = [
        ("root.inf", &root),
        ("base.inf", &base),
        ("leaf.inf", &leaf),
    ];
    let set = InfSet::new(&documents).unwrap();
    let plan = set
        .resolve(
            "ROOT.INF",
            &root.models(TARGET).unwrap()[0],
            TARGET,
            &["ROOT\\TEST"],
        )
        .unwrap();
    assert_eq!(plan.included_infs, ["base.inf", "leaf.inf"]);
    assert_eq!(plan.sections.len(), 6);
    for (offset, context) in [
        (0, InstallContext::DeviceSoftware),
        (3, InstallContext::DeviceHardware),
    ] {
        assert_eq!(plan.sections[offset].inf, "leaf.inf");
        assert_eq!(plan.sections[offset].plan.copies[0].directory_id, 11);
        let base = &plan.sections[offset + 1];
        assert_eq!(base.inf, "base.inf");
        assert_eq!(base.context, context);
        assert_eq!(base.plan.copies[0].source, "base.sys");
        assert_eq!(base.plan.copies[0].subdirectory, "base");
        assert_eq!(base.plan.pending[0].fields, ["base.reg"]);
    }
    assert!(plan.sections.iter().all(|s| s.plan.services.is_empty()));
    assert!(plan
        .sections
        .iter()
        .flat_map(|s| &s.plan.pending)
        .all(|p| !matches!(p.key.as_str(), "Include" | "Needs")));
}

#[test]
fn missing_include_and_missing_needed_section_are_not_partial_success() {
    for (extra, expected) in [
        ("Include=missing.inf\n", Error::MissingInclude),
        ("Needs=missing\n", Error::MissingSection),
        ("Include=../escape.inf\n", Error::InvalidPath),
        ("Include=base.sys\n", Error::InvalidPath),
        ("Needs=\n", Error::Syntax),
    ] {
        let root = root(extra);
        let documents = [("root.inf", &root)];
        let set = InfSet::new(&documents).unwrap();
        assert_eq!(
            set.resolve(
                "root.inf",
                &root.models(TARGET).unwrap()[0],
                TARGET,
                &["ROOT\\TEST"]
            ),
            Err(expected)
        );
    }
}

#[test]
fn namespace_validation_is_case_insensitive_and_bounded() {
    let inf = fragment("");
    assert!(matches!(
        InfSet::new(&[("base.inf", &inf), ("BASE.INF", &inf)]),
        Err(Error::Ambiguous)
    ));
    for name in [
        "../base.inf",
        "dir\\base.inf",
        "NUL.inf",
        "base.sys",
        "base.inf.",
    ] {
        assert!(matches!(
            InfSet::new(&[(name, &inf)]),
            Err(Error::InvalidPath)
        ));
    }
    assert!(matches!(InfSet::new(&[]), Err(Error::Limit)));
    assert!(matches!(
        InfSet::new(&[("base.inf", &inf); 65]),
        Err(Error::Limit)
    ));
}

#[test]
fn unrelated_document_cannot_satisfy_needs_or_leak_from_other_sections() {
    let external = fragment("[External]\nAddReg=reg\n");
    for extra in [
        "Needs=External\n",
        "Include=external.inf\n[Install.NT.HW]\nNeeds=External\n",
    ] {
        let root = root(extra);
        let documents = [("root.inf", &root), ("external.inf", &external)];
        let set = InfSet::new(&documents).unwrap();
        assert_eq!(
            set.resolve(
                "root.inf",
                &root.models(TARGET).unwrap()[0],
                TARGET,
                &["ROOT\\TEST"]
            ),
            Err(Error::MissingSection)
        );
    }
}

#[test]
fn duplicate_external_sections_are_rejected_but_local_section_is_authoritative() {
    let a = fragment("[Shared]\nAddReg=a.reg\n");
    let b = fragment("[Shared]\nAddReg=b.reg\n");
    let root = root("Include=a.inf,b.inf\nNeeds=Shared\n");
    let documents = [("root.inf", &root), ("a.inf", &a), ("b.inf", &b)];
    let set = InfSet::new(&documents).unwrap();
    assert_eq!(
        set.resolve(
            "root.inf",
            &root.models(TARGET).unwrap()[0],
            TARGET,
            &["ROOT\\TEST"]
        ),
        Err(Error::Ambiguous)
    );
    let root = self::root("Include=a.inf,b.inf\nNeeds=Shared\n[Shared]\nAddReg=local.reg\n");
    let documents = [("root.inf", &root), ("a.inf", &a), ("b.inf", &b)];
    let set = InfSet::new(&documents).unwrap();
    let plan = set
        .resolve(
            "root.inf",
            &root.models(TARGET).unwrap()[0],
            TARGET,
            &["ROOT\\TEST"],
        )
        .unwrap();
    assert_eq!(plan.sections[0].inf, "root.inf");
    assert_eq!(plan.sections[0].plan.pending[0].fields, ["local.reg"]);
}

#[test]
fn same_file_and_cross_file_cycles_are_rejected() {
    for extra in [
        "Needs=Install.NT\n",
        "Needs=A\n[A]\nNeeds=B\n[B]\nNeeds=A\n",
        "Include=external.inf\nNeeds=A\n",
    ] {
        let root = root(extra);
        let external = fragment("[A]\nInclude=root.inf\nNeeds=Install.NT\n");
        let documents = [("root.inf", &root), ("external.inf", &external)];
        let set = InfSet::new(&documents).unwrap();
        assert_eq!(
            set.resolve(
                "root.inf",
                &root.models(TARGET).unwrap()[0],
                TARGET,
                &["ROOT\\TEST"]
            ),
            Err(Error::DependencyCycle)
        );
    }
}

#[test]
fn diamond_graph_shares_dependencies_within_same_context() {
    let root = root("Needs=A,B\n[A]\nNeeds=Shared\n[B]\nNeeds=Shared\n[Shared]\nCopyFiles=@one.sys\n[DestinationDirs]\nDefaultDestDir=12\n");
    let documents = [("root.inf", &root)];
    let set = InfSet::new(&documents).unwrap();
    let plan = set
        .resolve(
            "root.inf",
            &root.models(TARGET).unwrap()[0],
            TARGET,
            &["ROOT\\TEST"],
        )
        .unwrap();
    assert_eq!(plan.sections.len(), 4);
    assert_eq!(
        plan.sections
            .iter()
            .map(|s| s.plan.copies.len())
            .sum::<usize>(),
        1
    );
}

#[test]
fn depth_and_global_operation_budgets_are_enforced() {
    let chain = (0..40)
        .map(|n| format!("[S{n}]\nNeeds=S{}\n", n + 1))
        .collect::<String>();
    let root = root(&format!("Needs=S0\n{chain}[S40]\n"));
    let documents = [("root.inf", &root)];
    let set = InfSet::new(&documents).unwrap();
    assert_eq!(
        set.resolve(
            "root.inf",
            &root.models(TARGET).unwrap()[0],
            TARGET,
            &["ROOT\\TEST"]
        ),
        Err(Error::Limit)
    );
    let directives = (0..9000).map(|_| "AddReg=reg\n").collect::<String>();
    let root = self::root(&format!(
        "Needs=A,B\n[A]\n{}[B]\n{}",
        &directives[..4500 * 11],
        &directives[4500 * 11..]
    ));
    let documents = [("root.inf", &root)];
    let set = InfSet::new(&documents).unwrap();
    assert_eq!(
        set.resolve(
            "root.inf",
            &root.models(TARGET).unwrap()[0],
            TARGET,
            &["ROOT\\TEST"]
        ),
        Err(Error::Limit)
    );
}

#[test]
fn dependency_exclusions_and_forged_model_selection_cannot_bypass_checks() {
    let root = root("Needs=Check\n[Check]\nExcludeId=ROOT\\BLOCKED\n");
    let documents = [("root.inf", &root)];
    let set = InfSet::new(&documents).unwrap();
    let mut model = root.models(TARGET).unwrap()[0].clone();
    assert_eq!(
        set.resolve("root.inf", &model, TARGET, &["ROOT\\TEST", "ROOT\\BLOCKED"]),
        Err(Error::Excluded)
    );
    model.install_section = "Check".into();
    assert_eq!(
        set.resolve("root.inf", &model, TARGET, &["ROOT\\TEST"]),
        Err(Error::NoMatch)
    );
}

#[test]
fn including_inf_overrides_only_default_destination() {
    let root = root("Include=base.inf\nNeeds=Base\n[DestinationDirs]\nDefaultDestDir=10,parent\n");
    let base = fragment("[Base]\nCopyFiles=@default.sys,explicit\n[explicit]\nexplicit.sys\n[DestinationDirs]\nDefaultDestDir=11,child\nexplicit=12,drivers\n");
    let documents = [("root.inf", &root), ("base.inf", &base)];
    let plan = InfSet::new(&documents)
        .unwrap()
        .resolve(
            "root.inf",
            &root.models(TARGET).unwrap()[0],
            TARGET,
            &["ROOT\\TEST"],
        )
        .unwrap();
    let base = &plan.sections[0];
    assert_eq!(base.default_destination_inf.as_deref(), Some("root.inf"));
    let copies = &base.plan.copies;
    assert!(copies[0].uses_default_destination);
    assert_eq!(
        (copies[0].directory_id, copies[0].subdirectory.as_str()),
        (10, "parent")
    );
    assert!(!copies[1].uses_default_destination);
    assert_eq!(
        (copies[1].directory_id, copies[1].subdirectory.as_str()),
        (12, "drivers")
    );
}

#[test]
fn included_inf_without_a_default_uses_the_callers_destination() {
    let root = root("Include=base.inf\nNeeds=Base\n[DestinationDirs]\nDefaultDestDir=12,parent\n");
    let base = fragment("[Base]\nCopyFiles=@default.sys,explicit\n[explicit]\nexplicit.sys\n[DestinationDirs]\nexplicit=11,own\n");
    let documents = [("root.inf", &root), ("base.inf", &base)];
    let plan = InfSet::new(&documents)
        .unwrap()
        .resolve(
            "root.inf",
            &root.models(TARGET).unwrap()[0],
            TARGET,
            &["ROOT\\TEST"],
        )
        .unwrap();
    assert_eq!(plan.sections[0].plan.copies[0].directory_id, 12);
    assert_eq!(plan.sections[0].plan.copies[0].subdirectory, "parent");
    assert_eq!(plan.sections[0].plan.copies[1].directory_id, 11);
    assert_eq!(plan.sections[0].plan.copies[1].subdirectory, "own");
}

#[test]
fn shared_dependency_retains_distinct_inherited_destinations() {
    let root = root("Include=a.inf,b.inf\nNeeds=A,B\n");
    let a = fragment("[A]\nInclude=leaf.inf\nNeeds=Leaf\n[DestinationDirs]\nDefaultDestDir=10,a\n");
    let b = fragment("[B]\nInclude=leaf.inf\nNeeds=Leaf\n[DestinationDirs]\nDefaultDestDir=11,b\n");
    let leaf = fragment("[Leaf]\nCopyFiles=@leaf.sys\n[DestinationDirs]\nDefaultDestDir=12\n");
    let documents = [
        ("root.inf", &root),
        ("a.inf", &a),
        ("b.inf", &b),
        ("leaf.inf", &leaf),
    ];
    let plan = InfSet::new(&documents)
        .unwrap()
        .resolve(
            "root.inf",
            &root.models(TARGET).unwrap()[0],
            TARGET,
            &["ROOT\\TEST"],
        )
        .unwrap();
    let copies: Vec<_> = plan
        .sections
        .iter()
        .filter(|s| s.inf == "leaf.inf")
        .collect();
    assert_eq!(copies.len(), 2);
    assert_eq!(copies[0].default_destination_inf.as_deref(), Some("a.inf"));
    assert_eq!(copies[0].plan.copies[0].subdirectory, "a");
    assert_eq!(copies[1].default_destination_inf.as_deref(), Some("b.inf"));
    assert_eq!(copies[1].plan.copies[0].subdirectory, "b");
}
