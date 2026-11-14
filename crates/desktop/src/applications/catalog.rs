//! Canonical application identities, filenames and availability. Shared with the image builder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum AppId {
    Calculator = 1, Paint, StickyNotes, WordPad, CharacterMap, SystemInformation,
    TaskManager, Run, OnScreenKeyboard, Magnifier, SnippingTool, Minesweeper,
    GettingStarted, Help, DefaultPrograms, GamesExplorer, Notepad, Explorer, CommandPrompt,
    ResourceMonitor, EaseOfAccessCenter, DiskPart, BcdEdit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Application {
    pub id: AppId,
    pub title: &'static str,
    pub chinese: &'static str,
    pub executable: &'static str,
    pub category: &'static str,
    pub icon: &'static str,
}
macro_rules! apps {
    ($(($id:ident,$title:literal,$cn:literal,$exe:literal,$category:literal,$icon:literal)),* $(,)?) => {
        pub const APPLICATIONS: &[Application] = &[$(Application { id: AppId::$id, title:$title,
            chinese:$cn, executable:$exe, category:$category, icon:$icon }),*];
    };
}
apps! {
    (BcdEdit,"Boot Configuration Data Editor","启动配置数据编辑器","bcdedit.exe","System Tools","run"),
    (DiskPart,"DiskPart","磁盘分区","diskpart.exe","System Tools","run"),
    (Calculator,"Calculator","计算器","calc.exe","Accessories","calculator"),
    (Paint,"Paint","画图","mspaint.exe","Accessories","paint"),
    (StickyNotes,"Sticky Notes","便笺","StikyNot.exe","Accessories","notes"),
    (WordPad,"WordPad","写字板","wordpad.exe","Accessories","editor"),
    (CharacterMap,"Character Map","字符映射表","charmap.exe","System Tools","characters"),
    (SystemInformation,"System Information","系统信息","msinfo32.exe","System Tools","information"),
    (TaskManager,"Windows Task Manager","任务管理器","taskmgr.exe","System Tools","tasks"),
    (Run,"Run","运行","explorer.exe","Accessories","run"),
    (OnScreenKeyboard,"On-Screen Keyboard","屏幕键盘","osk.exe","Ease of Access","keyboard"),
    (Magnifier,"Magnifier","放大镜","Magnify.exe","Ease of Access","magnifier"),
    (SnippingTool,"Snipping Tool","截图工具","SnippingTool.exe","Accessories","screenshot"),
    (Minesweeper,"Minesweeper","扫雷","Minesweeper.exe","Games","mines"),
    (GettingStarted,"Getting Started","入门","control.exe","Accessories","help"),
    (Help,"Help and Support","帮助和支持","HelpPane.exe","Maintenance","help"),
    (DefaultPrograms,"Default Programs","默认程序","control.exe","Programs","settings"),
    (GamesExplorer,"Games Explorer","游戏资源管理器","explorer.exe","Games","games"),
    (Notepad,"Notepad","记事本","notepad.exe","Accessories","editor"),
    (CommandPrompt,"Command Prompt","命令提示符","cmd.exe","Accessories","run"),
    (Explorer,"Windows Explorer","资源管理器","explorer.exe","Accessories","folder"),
    (ResourceMonitor,"Resource Monitor","资源监视器","resmon.exe","System Tools","tasks"),
    (EaseOfAccessCenter,"Ease of Access Center","轻松访问中心","control.exe","Ease of Access","settings"),
}
impl Application {
    /// PE subsystem identities follow the Windows console/GUI distinction.
    pub fn pe_subsystem(&self) -> u16 {
        match self.id {
            AppId::CommandPrompt | AppId::DiskPart | AppId::BcdEdit => 3,
            _ => 2,
        }
    }

    /// Canonical Windows 7 installation path. Run and Games are shell features.
    pub fn install_path(&self) -> Option<&'static str> {
        Some(match self.id {
            AppId::Run | AppId::GamesExplorer | AppId::GettingStarted | AppId::EaseOfAccessCenter => return None,
            AppId::WordPad => "Program Files/Windows NT/Accessories/wordpad.exe",
            AppId::Minesweeper => "Program Files/Microsoft Games/Minesweeper/Minesweeper.exe",
            AppId::Explorer => "Windows/explorer.exe",
            AppId::Calculator => "Windows/System32/calc.exe",
            AppId::Paint => "Windows/System32/mspaint.exe",
            AppId::StickyNotes => "Windows/System32/StikyNot.exe",
            AppId::CharacterMap => "Windows/System32/charmap.exe",
            AppId::SystemInformation => "Windows/System32/msinfo32.exe",
            AppId::TaskManager => "Windows/System32/taskmgr.exe",
            AppId::OnScreenKeyboard => "Windows/System32/osk.exe",
            AppId::Magnifier => "Windows/System32/Magnify.exe",
            AppId::SnippingTool => "Windows/System32/SnippingTool.exe",
            AppId::Help => "Windows/HelpPane.exe",
            AppId::DefaultPrograms => "Windows/System32/control.exe",
            AppId::Notepad => "Windows/System32/notepad.exe",
            AppId::CommandPrompt => "Windows/System32/cmd.exe",
            AppId::ResourceMonitor => "Windows/System32/resmon.exe",
            AppId::BcdEdit => "Windows/System32/bcdedit.exe",
            AppId::DiskPart => "Windows/System32/diskpart.exe",
        })
    }
}
impl AppId {
    pub fn from_raw(value: u32) -> Option<Self> { APPLICATIONS.iter().find(|app| app.id as u32 == value).map(|app| app.id) }
    pub fn application(self) -> &'static Application {
        // Every discriminant is registered; avoid unchecked indexing of external IDs.
        APPLICATIONS.iter().find(|app| app.id == self).expect("registered application")
    }
}
pub fn resolve(command: &str) -> Option<AppId> {
    let command = command.trim().trim_matches('"');
    if command.eq_ignore_ascii_case("run") {return Some(AppId::Run);}
    if command.eq_ignore_ascii_case("games") {return Some(AppId::GamesExplorer);}
    // Windows 7 exposes these pages through Control Panel, not extra EXEs.
    if command.eq_ignore_ascii_case("control.exe /name Microsoft.GettingStarted")
        || command.eq_ignore_ascii_case("control /name Microsoft.GettingStarted") {
        return Some(AppId::GettingStarted);
    }
    if command.eq_ignore_ascii_case("control.exe /name Microsoft.DefaultPrograms")
        || command.eq_ignore_ascii_case("control /name Microsoft.DefaultPrograms") {
        return Some(AppId::DefaultPrograms);
    }
    if command.eq_ignore_ascii_case("control.exe /name Microsoft.EaseOfAccessCenter")
        || command.eq_ignore_ascii_case("control /name Microsoft.EaseOfAccessCenter") {
        return Some(AppId::EaseOfAccessCenter);
    }
    let leaf = command.rsplit(['\\','/']).next()?;
    APPLICATIONS.iter().find(|a| a.title.eq_ignore_ascii_case(command) || a.chinese == command
        || (a.install_path().is_some() && (a.executable.eq_ignore_ascii_case(leaf)
            || a.executable.trim_end_matches(".exe").eq_ignore_ascii_case(leaf)))).map(|a| a.id)
}
/// Explicitly paused components from reports/catalog.md. Never install success stubs for these.
pub const DEFERRED: &[(&str, &str)] = &[
    ("Desktop Gadget Gallery", "gadget sandbox and HTML scripting engine"),
    ("Internet Explorer / No Add-ons", "HTML, JavaScript, TLS and WinINet engines"),
    ("Windows DVD Maker", "codecs, authoring and optical recording"),
    ("Windows Fax and Scan", "fax modem, scanner and WIA services"),
    ("Windows Media Center / Windows Media Player", "media pipeline, codecs and audio playback"),
    ("Windows Update", "servicing stack and signed update packages"),
    ("XPS Viewer", "XPS layout, fonts and printing"),
    ("Connect to a Projector / Network Projector", "display topology and network projection"),
    ("Sound Recorder", "audio capture device and encoding"),
    ("Math Input Panel / Tablet PC Input Panel / Handwriting", "recognition engines"),
    ("Sync Center", "offline files and synchronization services"),
    ("Remote Desktop Connection / Windows Remote Assistance", "RDP transport, authentication and session services"),
    ("Narrator / Windows Speech Recognition", "speech synthesis and recognition"),
    ("Disk Cleanup / Disk Defragmenter", "persistent filesystem allocation and maintenance APIs"),
    ("Task Scheduler", "persistent scheduling service"),
    ("System Restore / Backup and Restore / Create System Repair Disc", "volume snapshots, recovery and backup services"),
    ("Private Character Editor", "font editing and font installation"),
    ("Windows Easy Transfer / Reports", "profile migration and persistent registry"),
    ("Windows Journal", "ink documents and tablet input"),
    ("Windows PowerShell / ISE", "CLR and PowerShell runtime"),
    ("Chess Titans / Mahjong Titans / Purble Place", "game rules, assets and 3D rendering"),
    ("Internet Backgammon / Checkers / Spades", "online game protocol and services"),
    ("Hearts / FreeCell / Solitaire / Spider Solitaire", "card game rules and interaction"),
];
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn identities_are_unique_and_resolve_from_paths() {
        for (i,a) in APPLICATIONS.iter().enumerate() {
            assert_eq!(AppId::from_raw(a.id as u32),Some(a.id));
            if a.install_path().is_some() { assert_eq!(resolve(a.executable),Some(a.id)); }
            assert!(!APPLICATIONS[..i].iter().any(|b| a.id==b.id || (a.install_path().is_some() && a.install_path()==b.install_path())));
        }
        assert_eq!(resolve("C:\\Windows\\System32\\CALC.EXE"),Some(AppId::Calculator));
        assert_eq!(resolve("powershell"),None);
        assert_eq!(resolve("control.exe /name Microsoft.GettingStarted"),Some(AppId::GettingStarted));
        assert_eq!(resolve("control /name Microsoft.DefaultPrograms"),Some(AppId::DefaultPrograms));
        assert_eq!(resolve("control /name Microsoft.EaseOfAccessCenter"),Some(AppId::EaseOfAccessCenter));
        assert_eq!(resolve("resmon.exe"),Some(AppId::ResourceMonitor));
    }
}
