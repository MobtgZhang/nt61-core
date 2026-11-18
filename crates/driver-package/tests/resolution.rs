//! Behavioral rejection/selection tests over production INF resolution.
use nt61_driver_package as package;
use package::{Architecture, Error, Inf, Target};
const TARGET: Target = Target::SP1_AMD64;

fn fixture(extra: &str) -> Vec<u8> {
    format!(
        r#"[Version]
Signature="$Windows NT$"
Class=Test
ClassGuid={{4d36e967-e325-11ce-bfc1-08002be10318}}
Provider=%Vendor%
DriverVer=11/19/2010,6.1.7601.17514
[Strings]
Vendor="Example, Inc.; test"
Device="Test ""Device"""
{extra}
"#
    )
    .into_bytes()
}
fn inf(extra: &str) -> Inf {
    Inf::parse(&fixture(extra)).unwrap()
}
fn basic() -> &'static str {
    r#"
[Manufacturer]
%Vendor%=Models,NTamd64
[Models.NTamd64]
%Device%=Install,ROOT\TEST
[Install.NT]
CopyFiles=@driver.sys
[DestinationDirs]
DefaultDestDir=12
"#
}

#[test]
fn bom_encoding_and_invalid_input() {
    let bytes = fixture("");
    for little in [true, false] {
        let mut utf16 = if little {
            vec![255, 254]
        } else {
            vec![254, 255]
        };
        for unit in std::str::from_utf8(&bytes).unwrap().encode_utf16() {
            utf16.extend(if little {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        assert_eq!(
            Inf::parse(&utf16)
                .unwrap()
                .version(TARGET)
                .unwrap()
                .provider,
            "Example, Inc.; test"
        );
        utf16.push(1);
        assert!(matches!(Inf::parse(&utf16), Err(Error::Encoding)));
    }
    for bad in [
        b"\xff".as_slice(),
        &[255, 254, 0, 216],
        b"[a]\nx=\0",
        b"[a]\nx=\x01",
        b"\xef\xbb\xbf[a]",
    ] {
        assert!(Inf::parse(bad).is_err());
    }
}

#[test]
fn strings_cannot_inject_directives_or_fields() {
    let inf = inf(r#"
[Strings]
file="driver.sys,evil.sys"
escape="%%SystemRoot%%"
dir="%12%\driver.sys"
[Version]
CatalogFile="%Vendor%"
"#);
    assert_eq!(inf.expand("%Vendor%").unwrap(), "Example, Inc.; test");
    assert_eq!(inf.expand("%Device%").unwrap(), "Test \"Device\"");
    assert_eq!(inf.expand("%escape%").unwrap(), "%SystemRoot%");
    assert_eq!(inf.expand("%dir%").unwrap(), "%12%\\driver.sys");
    assert_eq!(inf.version(TARGET), Err(Error::InvalidPath));
    let entry = package::Entry {
        key: None,
        value: "%file%".into(),
        line: 0,
    };
    assert_eq!(inf.fields(&entry).unwrap(), vec!["driver.sys,evil.sys"]);
}

#[test]
fn limits_cycles_and_duplicate_security_keys() {
    let inf = inf("[Strings]\na=%b%\nb=%a%\n[Version]\nCatalogFile=x.cat\nCatalogFile=y.cat\n");
    assert_eq!(inf.expand("%a%"), Err(Error::StringCycle));
    assert_eq!(inf.expand("%unknown%"), Err(Error::UndefinedString));
    assert_eq!(inf.version(TARGET), Err(Error::Ambiguous));
    assert!(matches!(
        Inf::parse(&vec![b' '; 1024 * 1024 + 1]),
        Err(Error::Limit)
    ));
    assert!(matches!(
        Inf::parse(format!("[a]\nx={}\n", "a".repeat(4097)).as_bytes()),
        Err(Error::Limit)
    ));
    let nested = (0..40)
        .map(|n| format!("k{n}=%k{}%\n", n + 1))
        .collect::<String>();
    let bomb = inf_with_extra_strings(&nested);
    assert_eq!(bomb.expand("%k0%"), Err(Error::StringCycle));
}
fn inf_with_extra_strings(extra: &str) -> Inf {
    inf(&format!("[Strings]\n{extra}"))
}

#[test]
fn catalog_precedence_and_absence_never_imply_trust() {
    let inf =
        inf("[Version]\nCatalogFile=x.cat\nCatalogFile.NT=y.cat\nCatalogFile.NTamd64=z.cat\n");
    assert_eq!(
        inf.version(TARGET).unwrap().catalog.as_deref(),
        Some("z.cat")
    );
    assert_eq!(
        inf.version(Target {
            architecture: Architecture::X86,
            ..TARGET
        })
        .unwrap()
        .catalog
        .as_deref(),
        Some("y.cat")
    );
    assert!(self::inf("").version(TARGET).unwrap().catalog.is_none());
    for name in [
        "../x.cat",
        "C:\\x.cat",
        "sub\\x.cat",
        "%12%\\x.cat",
        "NUL.cat",
        "con.cat",
        "LPT1.cat",
        "x.cat.",
        "x.sys",
    ] {
        assert_eq!(
            self::inf(&format!("[Version]\nCatalogFile=\"{name}\"\n")).version(TARGET),
            Err(Error::InvalidPath)
        );
    }
}

#[test]
fn manufacturer_arch_os_product_and_suite_selection() {
    let inf = inf(r#"
[Manufacturer]
%Vendor%=Models,NTx86,NTamd64,NTamd64.6.0,NTamd64.6.1,NTamd64.6.1.3,NTamd64.6.1.1.0x100,NTamd64.6.2
[Models.NTx86]
x=x,ROOT\X86
[Models.NTamd64]
x=x,ROOT\OLD
[Models.NTamd64.6.0]
x=x,ROOT\VISTA
[Models.NTamd64.6.1]
x=x,ROOT\SP1
[Models.NTamd64.6.1.3]
x=x,ROOT\SERVER
[Models.NTamd64.6.1.1.0x100]
x=x,ROOT\SUITE
[Models.NTamd64.6.2]
x=x,ROOT\WIN8
"#);
    assert_eq!(inf.models(TARGET).unwrap()[0].ids, ["ROOT\\SP1"]);
    assert_eq!(
        inf.models(Target {
            product_type: 3,
            ..TARGET
        })
        .unwrap()[0]
            .ids,
        ["ROOT\\SERVER"]
    );
    assert_eq!(
        inf.models(Target {
            suite_mask: 0x100,
            ..TARGET
        })
        .unwrap()[0]
            .ids,
        ["ROOT\\SUITE"]
    );
    assert_eq!(
        inf.models(Target {
            architecture: Architecture::X86,
            ..TARGET
        })
        .unwrap()[0]
            .ids,
        ["ROOT\\X86"]
    );
    assert!(inf
        .matching_models(TARGET, &["ROOT\\SP"])
        .unwrap()
        .is_empty());
}

#[test]
fn empty_hardware_slot_retains_compatible_id() {
    let inf = inf("[Manufacturer]\nv=m,NTamd64\n[m.NTamd64]\nx=install,,HDAUDIO\\FUNC_01\n");
    let models = inf.matching_models(TARGET, &["hdaudio\\func_01"]).unwrap();
    assert_eq!(models[0].ids, ["", "HDAUDIO\\FUNC_01"]);
    assert!(inf.matching_models(TARGET, &[""]).unwrap().is_empty());
}

#[test]
fn copy_rename_destination_flags_and_pending_operations() {
    let extra = format!(
        r#"{}
[Install.NT]
CopyFiles=files
AddReg=registry
Include=base.inf
Needs=base.install
[files]
renamed.sys,original.sys,,0x100
[DestinationDirs]
files=10,system32\drivers
[Install.NT.Services]
AddService=svc,2,service,event
[service]
ServiceType=1
StartType=0
ErrorControl=3
ServiceBinary=%12%\renamed.sys
AddReg=service.registry
[event]
AddReg=event.registry
"#,
        basic()
    );
    let inf = inf(&extra);
    let models = inf.matching_models(TARGET, &["root\\test"]).unwrap();
    let plan = inf.plan(&models[0], TARGET, &["root\\test"]).unwrap();
    assert_eq!(plan.copies.len(), 2);
    assert_eq!(plan.copies[0].directory_id, 12);
    assert_eq!(plan.copies[1].source, "original.sys");
    assert_eq!(plan.copies[1].destination, "renamed.sys");
    assert_eq!(plan.copies[1].subdirectory, "system32\\drivers");
    assert_eq!(plan.copies[1].flags, 256);
    assert_eq!(plan.services[0].binary, "%12%\\renamed.sys");
    assert!(plan
        .pending
        .iter()
        .any(|p| p.key == "Include" && p.fields == ["base.inf"]));
    assert!(plan
        .pending
        .iter()
        .any(|p| p.section == "event" && p.key == "AddReg"));
    assert!(plan
        .pending
        .iter()
        .any(|p| p.section == "service" && p.key == "AddReg"));
    assert_eq!(
        inf.plan(&models[0], TARGET, &["OTHER"]),
        Err(Error::NoMatch)
    );
}

#[test]
fn exclusion_and_unresolved_files_reject_plan() {
    let extra = format!("{}\n[Install.NT]\nExcludeId=ROOT\\TEST\n", basic());
    let inf = inf(&extra);
    let model = &inf.models(TARGET).unwrap()[0];
    assert_eq!(
        inf.plan(model, TARGET, &["ROOT\\TEST"]),
        Err(Error::Excluded)
    );
    for bad in ["..\\escape.sys", "COM1.sys", "a/b.sys", "bad.sys."] {
        let input = basic().replace("@driver.sys", &format!("@{bad}"));
        let inf = self::inf(&input);
        assert_eq!(
            inf.plan(&inf.models(TARGET).unwrap()[0], TARGET, &["ROOT\\TEST"]),
            Err(Error::InvalidPath)
        );
    }
    let inf = self::inf(&basic().replace("DefaultDestDir=12", "other=12"));
    assert_eq!(
        inf.plan(&inf.models(TARGET).unwrap()[0], TARGET, &["ROOT\\TEST"]),
        Err(Error::MissingDestination)
    );
}

#[test]
fn repeated_sections_and_continuations_keep_order() {
    let inf = inf("[list]\nx=a,\\\n b,\"c;d\" ;comment\n[LIST]\nx=z\n");
    let section = inf.section("List").unwrap();
    assert_eq!(section.entries.len(), 2);
    assert_eq!(inf.fields(&section.entries[0]).unwrap(), ["a", "b", "c;d"]);
    assert_eq!(inf.single("list", "x"), Err(Error::Ambiguous));
    for malformed in [
        "x=orphan",
        "[a]\nx=\"unterminated",
        "[a]\nx=bad\"quote\"",
        "[a]\n=bad",
        "[a] extra",
        "[a]\nx=a,\\",
        "[a]\nx=a,\\\n",
        "[a]\nx=a,\\\n;comment\n",
        "[a]\n\\",
    ] {
        assert!(Inf::parse(malformed.as_bytes()).is_err());
    }
}

#[test]
fn terminal_dos_eof_matches_original_ks_inf_without_hiding_directives() {
    for text in ["[a]\nx=y\n\u{1a}\n", "[a]\nx=y\n\u{1a}\r\n \t"] {
        for utf16 in [false, true] {
            let bytes = if utf16 {
                let mut bytes = vec![255, 254];
                for unit in text.encode_utf16() {
                    bytes.extend(unit.to_le_bytes());
                }
                bytes
            } else {
                text.as_bytes().to_vec()
            };
            assert_eq!(Inf::parse(&bytes).unwrap().required("a", "x").unwrap(), "y");
        }
    }
    for text in [
        "[a]\nx=y\n\u{1a}\n[a]\nx=z",
        "[a]\nx=y\n\u{1a}\u{1a}\n",
        "[a]\nx=\u{1a}y",
    ] {
        assert!(matches!(Inf::parse(text.as_bytes()), Err(Error::Syntax)));
    }
}

#[test]
fn invalid_version_and_missing_sections_reject() {
    for (from, to) in [
        ("11/19/2010", "02/29/2011"),
        ("6.1.7601.17514", "6.1.7601.99999"),
        ("$Windows NT$", "$Chicago$"),
        ("4d36e967", "Gd36e967"),
    ] {
        let bytes = String::from_utf8(fixture("")).unwrap().replace(from, to);
        assert_eq!(
            Inf::parse(bytes.as_bytes()).unwrap().version(TARGET),
            Err(Error::InvalidVersion)
        );
    }
    assert_eq!(
        inf("[Manufacturer]\nx=missing,NTamd64\n").models(TARGET),
        Err(Error::MissingSection)
    );
}

#[test]
fn untrusted_syntax_corpus_never_panics() {
    // Mix arbitrary encoded data with INF punctuation, including incomplete
    // UTF-16 surrogates/quotes/continuations. Result is allowed to reject.
    let alphabet = b"[a]\r\n=,;\"%\\012 \t";
    let mut state = 0x61_7601_u64;
    for sample in 0..4000 {
        let mut bytes = Vec::new();
        if sample % 3 == 0 {
            bytes.extend_from_slice(&[255, 254]);
        }
        for _ in 0..sample % 512 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            bytes.push(if sample % 2 == 0 {
                state as u8
            } else {
                alphabet[state as usize % alphabet.len()]
            });
        }
        if let Ok(inf) = Inf::parse(&bytes) {
            let _ = inf.version(TARGET);
            let _ = inf.models(TARGET);
            let _ = inf.expand("%a%");
        }
    }
}
