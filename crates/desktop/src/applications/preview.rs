//! Host-side pixel oracle for application UI integration tests (same font and drawing code).
use nt61_desktop::{
    applications::{view, AppId, ApplicationSession, Environment},
    graphics::{font::FontLib, Rect, Surface},
};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.len() >= 4,
        "application-preview <id> <keys> <output.ppm>"
    );
    if matches!(
        args[1].as_str(),
        "--text" | "--label" | "--status" | "--status-note"
    ) {
        let label = args[1] == "--label";
        let status = args[1] != "--text";
        let mut frame = Surface::new(640, 460);
        frame.fill_rect(
            0,
            0,
            640,
            460,
            if status {
                if args[1] == "--status-note" {
                    0xFFFFF3A6
                } else {
                    0xFFF3F5F7
                }
            } else {
                0xFF0C0C0C
            },
        );
        let font = FontLib::ui().unwrap();
        font.draw_text(
            &mut frame,
            &args[2],
            10,
            10,
            args.get(4).and_then(|size| size.parse().ok()).unwrap_or(nt61_desktop::shell::layout::UI_FONT_SIZE),
            if label {
                0xFF182534
            } else if status {
                0xFF526578
            } else {
                0xFFF4F4F4
            },
        )
        .unwrap();
        let mut bytes = b"P6\n640 460\n255\n".to_vec();
        for pixel in frame.pixels() {
            bytes.extend([(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8]);
        }
        std::fs::write(&args[3], bytes).unwrap();
        return;
    }
    let width = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(640);
    let height = args.get(6).and_then(|v| v.parse().ok()).unwrap_or(460);
    let framed=std::env::var_os("NT61_PREVIEW_FRAME").is_some() && !args.get(4).is_some_and(|s|s=="slideshow");
    let body = if framed {Rect::new(6,30,width-12,height-36)} else {Rect::new(0, 0, width, height)};
    let id = AppId::from_raw(args[1].parse().unwrap()).unwrap();
    let mut app = ApplicationSession::new(1, id, 7919);
    if args.get(4).is_none_or(|s| s != "import") {
        for key in args[2].bytes() {
            app.key(key);
        }
    }
    if id == AppId::Notepad {
        use nt61_desktop::applications::notepad::{self, Command};
        if let Some(scenario) = args.get(4) {
            match scenario.as_str() {
                "open" => {
                    use nt61_desktop::{
                        applications::file_dialog::{FileDialog, Filter, Mode},
                        file::FileSystem,
                    };
                    app.dialog = Some(FileDialog::new(
                        &mut FileSystem::new(),
                        Mode::Open,
                        None,
                        "",
                        Filter::Text,
                    ));
                }
                "font-16" => app.notepad.font.points = 16,
                "font-bold-dialog" => {
                    notepad::execute(&mut app, Command::Font);
                    if let Some(nt61_desktop::applications::common_dialogs::Dialog::Font(draft)) =
                        &mut app.common_dialog
                    {
                        draft.font.style = 1;
                    }
                }
                "cursor-start" | "cursor-one" => {
                    if let nt61_desktop::applications::AppState::Document(d) = &mut app.state {
                        d.editor.cursor = usize::from(scenario == "cursor-one");
                    }
                }
                "file" | "edit" | "format" | "view" | "help-menu" => {
                    app.menu = true;
                    app.menu_kind = match scenario.as_str() {
                        "file" => 0,
                        "edit" => 1,
                        "format" => 2,
                        "view" => 3,
                        _ => 4,
                    };
                }
                "find" | "replace" | "font" | "goto" | "page" | "about" | "help" | "print"
                | "status" => {
                    notepad::execute(
                        &mut app,
                        match scenario.as_str() {
                            "find" => Command::Find,
                            "replace" => Command::Replace,
                            "font" => Command::Font,
                            "goto" => Command::GoTo,
                            "page" => Command::Page,
                            "about" => Command::About,
                            "help" => Command::Help,
                            "print" => Command::Print,
                            _ => Command::Status,
                        },
                    );
                }
                _ => {}
            }
        }
    }
    if id == AppId::WordPad {
        use nt61_desktop::applications::{
            wordpad::{self, Command},
            AppState,
        };
        if let Some(scenario) = args.get(4) {
            match scenario.as_str() {
                "sample" => {
                    if let AppState::Document(d) = &mut app.state {
                        d.insert("Windows 7 WordPad\nA lightweight editor for everyday documents.\n\nRich text, a familiar ribbon and your own files.\nRTF and DOCX documents\nLocal file and font services");
                        d.anchor = Some(0);
                        d.editor.cursor = 17;
                        d.format_selection(false, |s| {
                            s.bold = true;
                            s.size = 36;
                            s.color = 0xff2058b0;
                        });
                        d.anchor = None;
                        d.editor.cursor = 18;
                        d.format_selection(false, |s| {
                            s.bold = false;
                            s.size = 22;
                            s.color = 0xff000000;
                        });
                        d.editor.cursor = d.editor.text.chars().count();
                        d.format_selection(true, |s| s.bullet = true);
                        d.mark_saved();
                    }
                }
                "bold" => {
                    if let AppState::Document(d) = &mut app.state {
                        d.select_all();
                    }
                    wordpad::execute(&mut app, Command::Bold);
                }
                "font" => {
                    wordpad::execute(&mut app, Command::FontMenu);
                }
                "size" => {
                    wordpad::execute(&mut app, Command::SizeMenu);
                }
                "color" => {
                    wordpad::execute(&mut app, Command::ColorMenu);
                }
                "view" => {
                    wordpad::execute(&mut app, Command::Tab(true));
                }
                "zoom-max" => {
                    app.wordpad.zoom = 400;
                }
                "cursor-one" => {
                    if let AppState::Document(d) = &mut app.state {
                        d.editor.cursor = 1;
                    }
                }
                "open" => {
                    use nt61_desktop::{
                        applications::file_dialog::{FileDialog, Filter, Mode},
                        file::FileSystem,
                    };
                    app.dialog = Some(FileDialog::new(
                        &mut FileSystem::new(),
                        Mode::Open,
                        None,
                        "",
                        Filter::Documents,
                    ));
                }
                "import" => {
                    let bytes = std::fs::read(&args[2]).unwrap();
                    if let AppState::Document(d) = &mut app.state {
                        d.load(&bytes, true).unwrap();
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(scenario) = args.get(4) {
        use nt61_desktop::applications::common_dialogs::{self, ColorTarget};
        match scenario.as_str() {
            "save" => {
                use nt61_desktop::{
                    applications::file_dialog::{FileDialog, Filter, Mode},
                    file::FileSystem,
                };
                app.dialog = Some(FileDialog::new(
                    &mut FileSystem::new(), Mode::SaveAs, None, "", Filter::All,
                ));
            }
            "common-color" => common_dialogs::open_color(
                &mut app,
                if id == AppId::Paint {
                    ColorTarget::Paint
                } else {
                    ColorTarget::Text
                },
            ),
            "common-font" => common_dialogs::open_font(&mut app),
            "common-print" => common_dialogs::open_print(&mut app),
            "common-page" | "common-page-popup" | "common-page-paper" | "common-page-footer" => {
                common_dialogs::open_page(&mut app);
                if matches!(scenario.as_str(), "common-page-popup" | "common-page-paper") {
                    let r = common_dialogs::bounds(app.common_dialog.as_ref().unwrap(), body);
                    common_dialogs::pointer(&mut app, body, r.x + 30, r.y + 70);
                    if scenario == "common-page-paper" {
                        common_dialogs::key_at(&mut app, 27, body);
                    }
                } else if scenario == "common-page-footer" {
                    for _ in 0..5 {
                        common_dialogs::key_at(&mut app, 9, body);
                    }
                }
            }
            "open-types" | "save-encoding" => {
                use nt61_desktop::{
                    applications::{
                        document::Encoding,
                        file_dialog::{FileDialog, Filter, Focus, Mode},
                    },
                    file::FileSystem,
                };
                let mut files = FileSystem::new();
                files.touch(1, "Example.txt", b"sample");
                let save = scenario == "save-encoding";
                let mut dialog = FileDialog::new(
                    &mut files,
                    if save { Mode::SaveAs } else { Mode::Open },
                    None,
                    "Example.txt",
                    if id == AppId::WordPad {
                        Filter::Documents
                    } else {
                        Filter::Text
                    },
                );
                dialog.text_encoding = id == AppId::Notepad;
                dialog.encoding = Some(Encoding::Ansi);
                dialog.focus = if save { Focus::Encoding } else { Focus::Filter };
                dialog.key_at(&mut files, b' ', body);
                app.dialog = Some(dialog);
            }
            _ => {}
        }
    }
    if let Some(directory) = std::env::var_os("NT61_PREVIEW_EXPORT") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        if let nt61_desktop::applications::AppState::Document(d) = &app.state {
            if let Some(rich) = &d.rich {
                for (name, format) in [
                    (
                        "preview.docx",
                        nt61_desktop::applications::rich_text::Format::Docx,
                    ),
                    (
                        "preview.rtf",
                        nt61_desktop::applications::rich_text::Format::Rtf,
                    ),
                ] {
                    std::fs::write(
                        directory.join(name),
                        rich.encode(&d.editor.text, format).unwrap(),
                    )
                    .unwrap();
                }
            }
        }
    }
    if id == AppId::PhotoViewer {
        if let nt61_desktop::applications::AppState::PhotoViewer(v)=&mut app.state {
            if let Some(path)=std::env::var_os("NT61_PREVIEW_PICTURE") {
                let bytes=std::fs::read(&path).unwrap();
                let name=std::path::Path::new(&path).file_name().unwrap().to_string_lossy().into_owned();
                v.load(1,name,&bytes,vec![1],0).unwrap();
            }
            match args.get(4).map(String::as_str) {
                Some("slideshow")=>v.slideshow=true,
                Some("zoom")=>v.menu=Some(6),
                Some("file")=>v.menu=Some(0),
                Some("print")=>v.print=Some(Default::default()),
                _=>{},
            }
        }
    }
    if id == AppId::DeviceManager && args.get(4).is_some_and(|s|s=="sample"||s.starts_with("properties")) {
        use nt61_desktop::applications::{AppState,device_manager::{Device,Row}};
        if let AppState::DeviceManager(m)=&mut app.state {
            // Host-only fixture: the production manager always reads the kernel snapshot.
            for (address,hardware,compatible) in [(2,"PCI\\VEN_1234&DEV_1111","PCI\\CC_030000"),(3,"PCI\\VEN_8086&DEV_100E","PCI\\CC_020000"),(4,"PCI\\VEN_8086&DEV_2415","PCI\\CC_040100"),(5,"PCI\\VEN_8086&DEV_2922","PCI\\CC_010601")] {
                m.devices.push(Device{bus:1,address,function:0,state:4,bound:true,hardware:hardware.into(),compatible:compatible.into()});
            }
            m.refresh=false;m.selected=Row::Root;
            if args[4].starts_with("properties") {m.expanded[1]=true;m.properties=Some(0);m.tab=match args[4].as_str(){"properties-driver"=>1,"properties-details"=>2,_=>0};}
        }
    }
    let mut env = Environment {
        width: 1024,
        height: 768,
        network_adapters: 0,
        audio: false,
        processes: None,
        monitor: None,
        services: vec![],
        users: vec![],
        tasks: vec![],
    };
    if id == AppId::TaskManager {
        use nt61_desktop::applications::{Task, processes::{Snapshot, Process},system_monitor::{Sample,Service}};
        env.users=vec![("Administrator".into(),true)];
        env.tasks=vec![Task{id:10,title:"Documents".into(),icon:Some(AppId::Explorer),minimized:false},Task{id:11,title:"Untitled - Notepad".into(),icon:Some(AppId::Notepad),minimized:true},Task{id:12,title:"Windows Task Manager".into(),icon:Some(AppId::TaskManager),minimized:false}];
        env.processes=Some(Snapshot{cpu_count:4,processes:["System","smss.exe","winlogon.exe","explorer.exe","notepad.exe"].iter().enumerate().map(|(i,name)|Process{pid:4+i as u64*32,working_set:(i as u64+1)*1024*1024,cpu_ticks:i as u64*12,session:1,name:String::from(*name),user:if i<3{"SYSTEM"}else{"Administrator"}.into()}).collect()});
        env.services=vec![Service{name:"Dnscache".into(),description:"DNS Client".into(),group:"NetworkService".into(),state:4,pid:0,start_type:2,accepted:1,exit_code:0},Service{name:"EventLog".into(),description:"Windows Event Log".into(),group:"LocalServiceNetworkRestricted".into(),state:4,pid:0,start_type:2,accepted:0,exit_code:0}];
        app.manager.last=Some(Sample{ticks:100,idle:25,uptime:543000,total:1024*1024*1024,available:600*1024*1024,threads:27,pool:12*1024*1024,..Sample::default()});
        app.manager.cpu=Some(12);app.manager.show_all=true;
        app.manager.page=match args.get(4).map(String::as_str){Some("processes")=>1,Some("services")=>2,Some("performance")=>3,_=>0};
        if args.get(4).is_some_and(|v|v=="menu"){app.manager.menu=Some(0);}
    }
    let mut frame = Surface::new(width, height);
    if framed {
        use nt61_desktop::compositor::aero::{self,WindowGlassCache};
        for y in 0..height {for x in 0..width {frame.set_pixel(x,y,0xff000000|((20+x*35/width)<<16)|((90+y*70/height)<<8)|(155+x*70/width));}}
        let mut glass=WindowGlassCache::default();
        let r=Rect::new(0,0,width,height);
        glass.prepare_extended(&frame,1,r,0,if id==AppId::PhotoViewer{62}else{0},true);
        aero::window_body(&mut frame,body);glass.paint(&mut frame,1,true);
        aero::window_border(&mut frame,r,6);
    }
    let font = FontLib::ui().unwrap();
    if framed {
        use nt61_desktop::{compositor::aero,applications::icons};
        icons::draw(&mut frame,id,Rect::new(8,8,16,16));
        let title=if matches!(id,AppId::Notepad|AppId::WordPad|AppId::Paint|AppId::PhotoViewer) && !app.file_name().is_empty(){format!("{} - {}",app.file_name(),id.application().title)}else{id.application().title.to_owned()};
        font.draw_text(&mut frame,&title,29,8,12,0xff000000).unwrap();
        for (i,kind) in [aero::WindowControlButtonKind::Minimize,aero::WindowControlButtonKind::Maximize,aero::WindowControlButtonKind::Close].into_iter().enumerate(){aero::window_control_button(&mut frame,Rect::new(width as i32-105+i as i32*32,1,32,20),kind,false);}
    }
    let size = nt61_desktop::shell::layout::UI_FONT_SIZE;
    view::draw(
        &app,
        &mut frame,
        body,
        &env,
        |frame, text, b, color| {
            let mut short = String::new();
            for c in text.chars() {
                short.push(c);
                if font
                    .measure(&short, size)
                    .is_some_and(|m| m.width > b.width)
                {
                    short.pop();
                    break;
                }
            }
            let _ = font.draw_text(frame, &short, b.x, b.y, size, color);
        },
    );
    let mut bytes = format!("P6\n{} {}\n255\n", frame.width(), frame.height()).into_bytes();
    for pixel in frame.pixels() {
        bytes.extend([(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8]);
    }
    std::fs::write(&args[3], bytes).unwrap();
}
