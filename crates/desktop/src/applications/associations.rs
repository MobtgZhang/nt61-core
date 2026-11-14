//! Built-in associations use canonical application identities, shared by shell
//! file opening and the Default Programs page.
use super::AppId;

pub const ASSOCIATIONS: &[(&str, AppId)] = &[
    ("txt", AppId::Notepad),
    ("log", AppId::Notepad),
    ("ini", AppId::Notepad),
    ("rtf", AppId::WordPad),
    ("bmp", AppId::Paint),
];

pub fn resolve(name: &str) -> Option<AppId> {
    let (_, extension) = name.rsplit_once('.')?;
    ASSOCIATIONS.iter().find(|(ext, _)| ext.eq_ignore_ascii_case(extension)).map(|(_, app)| *app)
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn document_associations_do_not_treat_executables_as_text() {
        assert_eq!(resolve("Read Me.RTF"), Some(AppId::WordPad));
        assert_eq!(resolve("sample.BMP"), Some(AppId::Paint));
        assert_eq!(resolve("boot.ini"), Some(AppId::Notepad));
        assert_eq!(resolve("ntdll.dll"), None);
        assert_eq!(resolve("cmd.exe"), None);
    }
}
