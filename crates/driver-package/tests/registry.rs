//! Registry dependencies must preserve Windows data representation and HKR
//! provenance. These tests do not execute installation or grant package trust.
use nt61_driver_package::{
    Error, Inf, InfSet, RegistryAction, RegistryOperation, RegistryRoot, RegistryScope,
    RegistryView, RegistryWriteMode, Target,
};

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

fn resolve(inf: &Inf) -> Result<Vec<RegistryOperation>, Error> {
    let documents = [("root.inf", inf)];
    Ok(InfSet::new(&documents)?
        .resolve_registry(
            "root.inf",
            &inf.models(Target::SP1_AMD64)?[0],
            Target::SP1_AMD64,
            &["ROOT\\TEST"],
        )?
        .operations)
}

fn encoded(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[test]
fn strings_binary_dword_and_none_use_windows_data_encodings() {
    let inf = root(
        r#"AddReg=reg
[reg]
HKR,,string,,"comma,equals=and;semicolon"
HKLM,Software\Test,expand,0x20000,"%%SystemRoot%%\x.dll"
HKR,,binary,1,00,1e,FF,a,0
HKR,,dword,0x10001,4294967295
HKR,,none,0x20001,1,2
HKR,,multi,0x10000,one,two
HKR,,empty,,""
HKR,,empty_multi,0x10000,""
"#,
    );
    let ops = resolve(&inf).unwrap();
    let expected = [
        (1, encoded("comma,equals=and;semicolon")),
        (2, encoded("%SystemRoot%\\x.dll")),
        (3, vec![0, 0x1e, 255, 10, 0]),
        (4, vec![255; 4]),
        (0, vec![1, 2]),
        (7, [encoded("one"), encoded("two"), vec![0, 0]].concat()),
        (1, vec![0, 0]),
        (7, vec![0; 4]),
    ];
    assert_eq!(ops.len(), expected.len());
    for (op, (kind, bytes)) in ops.iter().zip(expected) {
        let RegistryAction::SetValue { value, mode, .. } = &op.action else {
            panic!("expected set value")
        };
        assert_eq!(value.kind, kind);
        assert_eq!(value.data, bytes);
        assert!(!value.requires_directory_resolution);
        assert_eq!(*mode, RegistryWriteMode::Replace);
        assert!(op.line > 1);
    }
    assert_eq!(ops[1].root, RegistryRoot::LocalMachine);
}

#[test]
fn conditional_write_key_delete_and_registry_views_remain_distinct() {
    let inf = root(
        r#"AddReg=reg
DelReg=del
[reg]
HKR,,absent,2,a
HKR,,present,0x20,b
HKR,,append,0x10008,c,d
HKR,Key,ignored,0x10,ignored,extra
HKR,Other,ignored,0x2000
HKR,,old,4
HKR,oldkey,,4
HKR,,v32,0x4000,x
HKR,,v64,0x1000,y
[del]
HKCR,OldClass
HKR,,OldValue
HKR,Key,,0x6000
HKR,,Multi,0x18002,OLD
"#,
    );
    let ops = resolve(&inf).unwrap();
    for (op, expected) in ops.iter().zip([
        RegistryWriteMode::IfAbsent,
        RegistryWriteMode::IfPresent,
        RegistryWriteMode::AppendUnique,
    ]) {
        assert!(matches!(&op.action, RegistryAction::SetValue { mode, .. } if *mode == expected));
    }
    assert_eq!(ops[3].action, RegistryAction::CreateKey);
    assert_eq!(ops[4].action, RegistryAction::CreateKey);
    assert_eq!(ops[5].action, RegistryAction::DeleteValue("old".into()));
    assert_eq!(ops[6].action, RegistryAction::DeleteKey);
    assert_eq!(ops[7].view, RegistryView::Bits32);
    assert_eq!(ops[8].view, RegistryView::Bits64);
    assert_eq!(ops[9].action, RegistryAction::DeleteKey);
    assert_eq!(
        ops[10].action,
        RegistryAction::DeleteValue("OldValue".into())
    );
    assert_eq!(ops[11].action, RegistryAction::DeleteKey);
    assert_eq!(ops[11].view, RegistryView::Bits32);
    assert_eq!(
        ops[12].action,
        RegistryAction::DeleteMultiString {
            name: "Multi".into(),
            value: "OLD".into()
        }
    );
}

#[test]
fn hkr_distinguishes_device_service_event_log_and_shared_lists() {
    let inf = root(
        r#"AddReg=common
[Install.NT.HW]
AddReg=common
[Install.NT.CoInstallers]
AddReg=common
[Install.NT.Services]
AddService=First,2,svc,events
AddService=Second,0,svc,events
[svc]
ServiceType=1
StartType=3
ErrorControl=1
ServiceBinary=%12%\driver.sys
AddReg=common
[events]
AddReg=common
[common]
HKR,,Value,,test
"#,
    );
    let ops = resolve(&inf).unwrap();
    assert_eq!(
        ops.iter().map(|o| o.scope.clone()).collect::<Vec<_>>(),
        [
            RegistryScope::DeviceSoftware,
            RegistryScope::Service("First".into()),
            RegistryScope::Service("Second".into()),
            RegistryScope::EventLog("First".into()),
            RegistryScope::EventLog("Second".into()),
            RegistryScope::DeviceHardware,
            RegistryScope::DeviceSoftware,
        ]
    );
}

#[test]
fn needed_lists_use_own_strings_and_keep_numeric_dirids_unresolved() {
    let inf = root("Include=base.inf\nNeeds=Base\n[Install.NT.HW]\nInclude=base.inf\nNeeds=Base");
    let base = Inf::parse(
        br#"[Base]
AddReg=reg
[reg]
HKR,*Any\Repeated\\Key,Name,,"%text% %11%\file.dll"
[reg.security]
"D:P(A;;GA;;;SY)(A;;GA;;;BA)"
[Strings]
text=own
"#,
    )
    .unwrap();
    let documents = [("root.inf", &inf), ("base.inf", &base)];
    let plan = InfSet::new(&documents)
        .unwrap()
        .resolve_registry(
            "root.inf",
            &inf.models(Target::SP1_AMD64).unwrap()[0],
            Target::SP1_AMD64,
            &["ROOT\\TEST"],
        )
        .unwrap();
    assert_eq!(plan.operations.len(), 2);
    for op in &plan.operations {
        assert_eq!(op.inf, "base.inf");
        assert_eq!(op.subkey, "*Any\\Repeated\\\\Key");
        assert!(op.security_descriptor.is_some());
        let RegistryAction::SetValue { value, .. } = &op.action else {
            panic!("set value")
        };
        assert_eq!(value.data, encoded("own %11%\\file.dll"));
        assert!(value.requires_directory_resolution);
    }
    assert_eq!(plan.operations[0].scope, RegistryScope::DeviceSoftware);
    assert_eq!(plan.operations[1].scope, RegistryScope::DeviceHardware);
}

#[test]
fn omitted_data_is_not_reinterpreted_as_a_flag_or_an_empty_string() {
    let inf = root("AddReg=reg\n[reg]\nHKR,Key,0x00000010\nHKR,Key\nHKR,Key,,\n");
    let ops = resolve(&inf).unwrap();
    assert_eq!(
        ops[0].action,
        RegistryAction::AddWithoutData {
            name: Some("0x00000010".into()),
            flags: 0
        }
    );
    assert_eq!(
        ops[1].action,
        RegistryAction::AddWithoutData {
            name: None,
            flags: 0
        }
    );
    assert_eq!(
        ops[2].action,
        RegistryAction::AddWithoutData {
            name: Some(String::new()),
            flags: 0
        }
    );
}

#[test]
fn missing_malformed_unknown_or_conflicting_entries_fail_without_a_partial_plan() {
    for line in [
        "HKR,,bad,0x10001,4294967296",
        "HKR,,bad,0x10001,",
        "HKR,,bad,1,100",
        "HKR,,bad,1,0x10",
        "HKR,,bad,0x5000,test",
        "HKR,,bad,8,test",
        "HKR,,bad,0x22,test",
        "HKR,,bad,0x80,test",
        "HKR,,bad,0x30000,test",
        "HKR,,bad,0xb0001,1",
        "HKR,,bad,0x10000,a,,b",
        "HKR,,bad,,one,two",
        "UNKNOWN,,bad,,test",
        "HKR",
        "name=value",
    ] {
        let inf = root(&format!("AddReg=reg\n[reg]\nHKR,,valid,,yes\n{line}\n"));
        assert!(resolve(&inf).is_err(), "accepted {line}");
    }
    assert_eq!(resolve(&root("AddReg=missing")), Err(Error::MissingSection));
    assert_eq!(
        resolve(&root("AddReg=reg\n[reg]\nHKR,,v,,x\n[reg.security]\n")),
        Err(Error::Syntax)
    );
}

#[test]
fn service_needs_cannot_hide_unresolved_registry_dependencies() {
    let inf = root(
        r#"[Install.NT.Services]
AddService=First,2,svc
[svc]
ServiceType=1
StartType=3
ErrorControl=1
ServiceBinary=%12%\driver.sys
Needs=Other
[Other]
AddReg=missing
"#,
    );
    assert_eq!(resolve(&inf), Err(Error::UnsupportedDirective));
}

#[test]
fn large_registry_expansions_are_bounded_even_with_reused_sections() {
    let inf = root(&format!(
        "AddReg={}\n[reg]\nHKR,,v,,{}\n",
        vec!["reg"; 200].join(","),
        "x".repeat(2000)
    ));
    assert_eq!(resolve(&inf), Err(Error::Limit));
    let mut text = String::from("AddReg=reg\n[reg]\n");
    for n in 0..8193 {
        text.push_str(&format!("HKR,,v{n},,x\n"));
    }
    assert_eq!(resolve(&root(&text)), Err(Error::Limit));
}

#[test]
fn interface_registry_keys_retain_guid_reference_identity_and_pending_work() {
    let inf = root(
        r#"[Install.NT.Interfaces]
AddInterface={6994ad04-93ef-11d0-a3cc-00a0c9223196},Wave,iface
AddInterface={65e8773e-8f56-11d0-a3b9-00a0c9223196},Wave,iface
AddInterface={6994ad04-93ef-11d0-a3cc-00a0c9223196},Second,iface,0
AddInterface={65e8773d-8f56-11d0-a3b9-00a0c9223196}
[iface]
AddReg=reg
AddProperty=properties
CopyFiles=@interface.dll
[reg]
HKR,,FriendlyName,,"Test interface"
"#,
    );
    let documents = [("root.inf", &inf)];
    let plan = InfSet::new(&documents)
        .unwrap()
        .resolve_registry(
            "root.inf",
            &inf.models(Target::SP1_AMD64).unwrap()[0],
            Target::SP1_AMD64,
            &["ROOT\\TEST"],
        )
        .unwrap();
    assert_eq!(plan.interfaces.len(), 4);
    assert_eq!(plan.operations.len(), 3);
    for (op, interface) in plan.operations.iter().zip(&plan.interfaces) {
        assert_eq!(
            op.scope,
            RegistryScope::Interface {
                class_guid: interface.class_guid.clone(),
                reference: interface.reference.clone(),
            }
        );
        assert_eq!(op.root, RegistryRoot::Relative);
        assert_eq!(
            interface
                .pending
                .iter()
                .map(|p| p.key.as_str())
                .collect::<Vec<_>>(),
            ["AddProperty", "CopyFiles"]
        );
    }
    assert_ne!(plan.operations[0].scope, plan.operations[1].scope);
    assert_ne!(plan.operations[0].scope, plan.operations[2].scope);
    assert_eq!(plan.interfaces[3].install_section, None);
}

#[test]
fn included_interface_uses_its_own_lists_without_global_name_lookup() {
    let inf = root("[Install.NT.Interfaces]\nInclude=base.inf\nNeeds=Base.Interfaces\n");
    let base = Inf::parse(
        br#"[Base.Interfaces]
AddInterface={6994ad04-93ef-11d0-a3cc-00a0c9223196},Wave,iface
[iface]
AddReg=reg
[reg]
HKR,,FriendlyName,,%friendly%
[Strings]
friendly=Base
"#,
    )
    .unwrap();
    let documents = [("root.inf", &inf), ("base.inf", &base)];
    let plan = InfSet::new(&documents)
        .unwrap()
        .resolve_registry(
            "root.inf",
            &inf.models(Target::SP1_AMD64).unwrap()[0],
            Target::SP1_AMD64,
            &["ROOT\\TEST"],
        )
        .unwrap();
    assert_eq!(plan.interfaces[0].inf, "base.inf");
    assert_eq!(plan.operations[0].inf, "base.inf");
    let RegistryAction::SetValue { value, .. } = &plan.operations[0].action else {
        panic!("set")
    };
    assert_eq!(value.data, encoded("Base"));
}

#[test]
fn malformed_or_unresolved_interface_dependencies_do_not_produce_partial_plans() {
    for directive in [
        "AddInterface=invalid",
        "AddInterface={6994ad04-93ef-11d0-a3cc-00a0c9223196},,missing",
        "AddInterface={6994ad04-93ef-11d0-a3cc-00a0c9223196},Bad\\Ref",
        "AddInterface={6994ad04-93ef-11d0-a3cc-00a0c9223196},,,1",
        "AddInterface={6994ad04-93ef-11d0-a3cc-00a0c9223196},,,,",
        "AddReg=reg\n[reg]\nHKR,,value,,bad-scope",
        "AddInterface={6994ad04-93ef-11d0-a3cc-00a0c9223196},,iface\n[iface]\nNeeds=Other",
    ] {
        assert!(
            resolve(&root(&format!("[Install.NT.Interfaces]\n{directive}\n"))).is_err(),
            "accepted {directive}"
        );
    }
}

#[test]
fn excessive_interfaces_are_rejected_even_without_registry_data() {
    let mut extra = String::from("[Install.NT.Interfaces]\n");
    for _ in 0..1025 {
        extra.push_str("AddInterface={6994ad04-93ef-11d0-a3cc-00a0c9223196}\n");
    }
    assert_eq!(resolve(&root(&extra)), Err(Error::Limit));
}
