use nt61_driver_package::{
    registry_apply::{ApplyError, RegistryBinding, RegistryImage},
    Architecture, Inf, InfSet, RegistryAction, RegistryPlan, RegistryRoot, RegistryScope,
    RegistryView, Target,
};

fn plan(body: &str) -> RegistryPlan {
    let text = format!(
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
{body}
"#
    );
    let inf = Inf::parse(text.as_bytes()).unwrap();
    InfSet::new(&[("root.inf", &inf)])
        .unwrap()
        .resolve_registry(
            "root.inf",
            &inf.models(Target::SP1_AMD64).unwrap()[0],
            Target::SP1_AMD64,
            &["ROOT\\TEST"],
        )
        .unwrap()
}

fn binding(scope: RegistryScope, destination: &str) -> RegistryBinding {
    RegistryBinding {
        root: RegistryRoot::Relative,
        scope: Some(scope),
        view: RegistryView::Native,
        source_prefix: String::new(),
        destination: destination.into(),
    }
}
fn bindings() -> Vec<RegistryBinding> {
    vec![binding(RegistryScope::DeviceSoftware, "Device\\Software")]
}
fn text(data: &[u8]) -> String {
    String::from_utf16(
        &data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

#[test]
fn conditional_writes_observe_earlier_operations_in_the_same_batch() {
    let plan = plan("AddReg=r\n[r]\nHKR,,name,,first\nHKR,,name,2,ignored\nHKR,,missing,0x20,no\nHKR,,name,0x20,last\nHKR,,empty,,\"\"\nHKR,Child,,0x10\n");
    let mut image = RegistryImage::default();
    let summary = image.apply(&plan, &bindings(), &[]).unwrap();
    assert_eq!((summary.changed, summary.unchanged), (4, 2));
    assert_eq!(
        text(
            &image
                .value("DEVICE\\software", "NAME")
                .unwrap()
                .unwrap()
                .data
        ),
        "last\0"
    );
    assert!(image
        .value("device\\software", "missing")
        .unwrap()
        .is_none());
    assert_eq!(
        image
            .value("device\\software", "empty")
            .unwrap()
            .unwrap()
            .data,
        [0, 0]
    );
    assert!(image.contains_key("device\\software\\child").unwrap());
}

#[test]
fn append_deduplicates_and_delete_preserves_order_and_empty_termination() {
    let mut image = RegistryImage::default();
    let plan = plan("AddReg=r\nDelReg=d\n[r]\nHKR,,list,0x10000,One,two\nHKR,,list,0x10008,ONE,three,THREE\n[d]\nHKR,,list,0x18002,TWO\n");
    image.apply(&plan, &bindings(), &[]).unwrap();
    assert_eq!(
        text(
            &image
                .value("device\\software", "list")
                .unwrap()
                .unwrap()
                .data
        ),
        "One\0three\0\0"
    );
    let deletion = plan_from_action(RegistryAction::DeleteMultiString {
        name: "list".into(),
        value: "one".into(),
    });
    image.apply(&deletion, &bindings(), &[]).unwrap();
    let deletion = plan_from_action(RegistryAction::DeleteMultiString {
        name: "list".into(),
        value: "three".into(),
    });
    image.apply(&deletion, &bindings(), &[]).unwrap();
    assert_eq!(
        image
            .value("device\\software", "list")
            .unwrap()
            .unwrap()
            .data,
        [0; 4]
    );
}

fn plan_from_action(action: RegistryAction) -> RegistryPlan {
    let mut result = plan("AddReg=r\n[r]\nHKR,,value,,seed\n");
    result.operations[0].action = action;
    result
}

#[test]
fn failure_after_a_successful_write_keeps_all_prior_state() {
    let mut image = RegistryImage::default();
    image
        .apply(
            &plan("AddReg=r\n[r]\nHKR,,seed,,original\n"),
            &bindings(),
            &[],
        )
        .unwrap();
    let before = image.clone();
    let cases = [
        ("HKR,,ambiguous", ApplyError::UnsupportedOmittedData),
        ("HKLM,Other,value,,outside", ApplyError::MissingBinding),
        ("HKR,,path,,%99%\\x.sys", ApplyError::MissingDirectory),
        ("HKR,,seed,0x10008,multi", ApplyError::TypeMismatch),
    ];
    for (last, expected) in cases {
        let p = plan(&format!(
            "AddReg=r\n[r]\nHKR,,seed,,changed\nHKR,NewKey,new,,new\n{last}\n"
        ));
        assert_eq!(image.apply(&p, &bindings(), &[]), Err(expected));
        assert_eq!(image, before);
    }
}

#[test]
fn deleting_a_subtree_uses_component_boundaries_and_does_not_create_keys() {
    let mut image = RegistryImage::default();
    image
        .apply(
            &plan("AddReg=r\n[r]\nHKR,A\\Child,v,,a\nHKR,AB,v,,b\n"),
            &bindings(),
            &[],
        )
        .unwrap();
    image
        .apply(
            &plan("DelReg=r\n[r]\nHKR,A\nHKR,Missing,v\n"),
            &bindings(),
            &[],
        )
        .unwrap();
    assert!(!image.contains_key("Device\\Software\\A").unwrap());
    assert!(!image.contains_key("Device\\Software\\A\\Child").unwrap());
    assert!(image.value("Device\\Software\\AB", "v").unwrap().is_some());
    assert!(!image.contains_key("Device\\Software\\Missing").unwrap());
}

#[test]
fn bindings_isolate_views_scopes_and_hive_prefixes() {
    let mut image = RegistryImage::default();
    let p =
        plan("AddReg=r\n[Install.NT.HW]\nAddReg=r\n[r]\nHKR,,v,,data\nHKR,,v32,0x4000,data32\n");
    let mut b = bindings();
    assert_eq!(image.apply(&p, &b, &[]), Err(ApplyError::MissingBinding));
    let mut b32 = b[0].clone();
    b32.view = RegistryView::Bits32;
    b32.destination = "Device\\Software32".into();
    b.push(b32.clone());
    b.push(binding(RegistryScope::DeviceHardware, "Device\\Hardware"));
    b32.scope = Some(RegistryScope::DeviceHardware);
    b32.destination = "Device\\Hardware32".into();
    b.push(b32);
    image.apply(&p, &b, &[]).unwrap();
    for key in ["Software", "Hardware", "Software32", "Hardware32"] {
        assert!(image.contains_key(&format!("Device\\{key}")).unwrap());
    }
    let mapped = RegistryBinding {
        root: RegistryRoot::LocalMachine,
        scope: None,
        view: RegistryView::Native,
        source_prefix: "SYSTEM\\CurrentControlSet".into(),
        destination: "ControlSet001".into(),
    };
    let p = plan("AddReg=r\n[r]\nHKLM,SYSTEM\\CurrentControlSet\\Services\\test,v,,mapped\n");
    image.apply(&p, std::slice::from_ref(&mapped), &[]).unwrap();
    assert!(image.contains_key("ControlSet001\\Services\\test").unwrap());
    let p = plan("AddReg=r\n[r]\nHKLM,SYSTEM\\CurrentControlSetOther,v,,escape\n");
    assert_eq!(
        image.apply(&p, &[mapped], &[]),
        Err(ApplyError::MissingBinding)
    );
}

#[test]
fn repeated_separators_and_registry_names_are_not_filesystem_paths() {
    let mut image = RegistryImage::default();
    let p = plan("AddReg=r\n[r]\nHKR,A\\\\B,*FlowControl,0x10001,3\nHKR,.,value,,dot-is-a-key\n");
    image.apply(&p, &bindings(), &[]).unwrap();
    assert_eq!(
        image
            .value("device\\software\\a\\b", "*flowcontrol")
            .unwrap()
            .unwrap()
            .data,
        3u32.to_le_bytes()
    );
    assert!(image.contains_key("Device\\Software\\.").unwrap());
}

#[test]
fn dirids_are_explicit_but_environment_variables_and_unicode_data_survive() {
    let p = plan(
        "AddReg=r\n[r]\nHKR,,path,0x20000,%11%\\file.dll\nHKR,,env,0x20000,%%SystemRoot%%\\x\n",
    );
    let mut image = RegistryImage::default();
    image
        .apply(&p, &bindings(), &[(11, "C:\\Windows\\System32")])
        .unwrap();
    assert_eq!(
        text(
            &image
                .value("device\\software", "path")
                .unwrap()
                .unwrap()
                .data
        ),
        "C:\\Windows\\System32\\file.dll\0"
    );
    assert_eq!(
        text(
            &image
                .value("device\\software", "env")
                .unwrap()
                .unwrap()
                .data
        ),
        "%SystemRoot%\\x\0"
    );
    let mut p = p.clone();
    if let RegistryAction::SetValue { value, .. } = &mut p.operations[0].action {
        value.requires_directory_resolution = false;
    }
    assert_eq!(
        image.apply(&p, &bindings(), &[]),
        Err(ApplyError::MissingDirectory)
    );
    let mut p = plan_from_action(RegistryAction::CreateKey);
    p.operations[0].action = RegistryAction::SetValue {
        name: "unicode".into(),
        value: nt61_driver_package::RegistryValue {
            kind: 1,
            data: "中文\0".encode_utf16().flat_map(u16::to_le_bytes).collect(),
            requires_directory_resolution: false,
        },
        mode: nt61_driver_package::RegistryWriteMode::Replace,
    };
    image.apply(&p, &bindings(), &[]).unwrap();
    assert_eq!(
        text(
            &image
                .value("device\\software", "unicode")
                .unwrap()
                .unwrap()
                .data
        ),
        "中文\0"
    );
}

#[test]
fn ambiguous_mappings_unsupported_security_and_interfaces_never_partially_apply() {
    let mut image = RegistryImage::default();
    let p = plan("AddReg=r\n[r]\nHKR,,v,,data\n");
    let mut b = bindings();
    b.push(b[0].clone());
    assert_eq!(image.apply(&p, &b, &[]), Err(ApplyError::AmbiguousBinding));
    let secured = plan("AddReg=r\n[r]\nHKR,,v,,data\n[r.security]\n\"D:P(A;;GA;;;SY)\"\n");
    assert_eq!(
        image.apply(&secured, &bindings(), &[]),
        Err(ApplyError::UnsupportedSecurity)
    );
    let interfaced = plan("AddReg=r\n[r]\nHKR,,v,,data\n[Install.NT.Interfaces]\nAddInterface={4d36e967-e325-11ce-bfc1-08002be10318}\n");
    assert_eq!(
        image.apply(&interfaced, &bindings(), &[]),
        Err(ApplyError::UnsupportedInterfaces)
    );
    assert_eq!(image, RegistryImage::default());
}

#[test]
fn malformed_public_plans_are_validated_even_without_the_inf_parser() {
    let p = plan("AddReg=r\n[r]\nHKR,,v,,data\n");
    let mut image = RegistryImage::default();
    for data in [
        vec![0],
        vec![],
        vec![0xff, 0xd8, 0, 0],
        vec![0, 0, 1, 0, 0, 0],
    ] {
        let mut bad = p.clone();
        if let RegistryAction::SetValue { value, .. } = &mut bad.operations[0].action {
            value.data = data;
        }
        assert_eq!(
            image.apply(&bad, &bindings(), &[]),
            Err(ApplyError::InvalidData)
        );
    }
    assert_eq!(
        image.value("valid", "非ASCII"),
        Err(ApplyError::InvalidName)
    );
    let mut too_many = p.clone();
    too_many.operations = vec![p.operations[0].clone(); 8193];
    assert_eq!(
        image.apply(&too_many, &bindings(), &[]),
        Err(ApplyError::Limit)
    );
    assert_eq!(image, RegistryImage::default());
    assert_eq!(p.target_architecture, Architecture::Amd64);
}

#[test]
fn delete_root_and_unsupported_unicode_comparison_leave_state_unchanged() {
    let mut image = RegistryImage::default();
    let mut b = bindings();
    b[0].destination.clear();
    assert_eq!(
        image.apply(&plan_from_action(RegistryAction::DeleteKey), &b, &[]),
        Err(ApplyError::DeleteRoot)
    );
    let p = plan("AddReg=r\n[r]\nHKR,,list,0x10008,one\n");
    let mut bad = p.clone();
    if let RegistryAction::SetValue { value, .. } = &mut bad.operations[0].action {
        value.data = "Ä\0\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
    }
    assert_eq!(
        image.apply(&bad, &bindings(), &[]),
        Err(ApplyError::UnsupportedComparison)
    );
    assert_eq!(image, RegistryImage::default());
}
