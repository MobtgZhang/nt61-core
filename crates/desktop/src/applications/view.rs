//! Shared application chrome, rendering and hit geometry. Text is supplied by the host font renderer.
use alloc::{format,string::String,vec::Vec};
use crate::{compositor::aero,graphics::{Rect,Surface}};
use super::{AppAction,AppId,AppState,ApplicationSession,Environment,paint::{COLORS,Tool},minesweeper::Outcome};
#[derive(Clone,Debug,PartialEq,Eq)]pub enum Command {Key(u8),File,EditorMenu(u8),WordWrap,FindNext,About,Open,Save,SaveAs,NewDocument,Exit,Redo,Cut,Find,Replace,PaintTab(bool),Rotate,Flip(bool),BrushSize,PaintZoom(i32),Grid,ResizeHalf,New,Undo,Bold,Italic,Copy,Paste,Color(usize),Tool(Tool),Character(u32),Page(i32),Run,SelectTask(u32),TaskPage(usize),ProcessPage(i32),ActivateTask,EndTask,Shift,Type(u8),Zoom(i32),Capture,Game(usize),Flag(usize),Launch(AppId),Topic(usize)}
#[derive(Clone,Debug,PartialEq,Eq)]pub struct Button {pub bounds:Rect,pub label:String,pub command:Command}
fn button(out:&mut Vec<Button>,body:Rect,x:i32,y:i32,w:u32,h:u32,label:&str,command:Command){let bounds=Rect::new(body.x+x,body.y+y,w,h);if bounds.intersection(&body)==Some(bounds){out.push(Button{bounds,label:label.into(),command});}}
pub fn buttons(app:&ApplicationSession,b:Rect,env:&Environment)->Vec<Button>{let mut out=Vec::new();match &app.state{
    AppState::CommandPrompt(_)=>{},
    AppState::Calculator(_)=>{
        let rows:[&[(&str,u8)];6]=[
            &[("MC",b'x'),("MR",b'p'),("MS",b'm'),("M+",b'a')],
            &[("Back",8),("CE",b'e'),("C",b'c'),("+/-",b'n')],
            &[("7",b'7'),("8",b'8'),("9",b'9'),("/",b'/')],
            &[("4",b'4'),("5",b'5'),("6",b'6'),("*",b'*')],
            &[("1",b'1'),("2",b'2'),("3",b'3'),("-",b'-')],
            &[("0",b'0'),(".",b'.'),("=",b'='),("+",b'+')]];
        for(y,row)in rows.iter().enumerate(){for(x,(label,key))in row.iter().enumerate(){button(&mut out,b,16+x as i32*68,88+y as i32*43,62,36,label,Command::Key(*key));}}
    },
    AppState::Paint(p)=>{
        for(i,(label,command))in [("File",Command::File),("Home",Command::PaintTab(false)),("View",Command::PaintTab(true)),("Save",Command::Save),("Undo",Command::Undo),("Redo",Command::Redo)].into_iter().enumerate(){button(&mut out,b,6+i as i32*65,4,61,25,label,command);}
        if p.view_tab{
            for(i,(label,command))in [("Zoom in",Command::PaintZoom(50)),("Zoom out",Command::PaintZoom(-50)),("100%",Command::PaintZoom(0)),("Gridlines",Command::Grid)].into_iter().enumerate(){button(&mut out,b,8+i as i32*100,42,94,44,label,command);}
        }else{
            for(i,(label,command))in [("Pencil",Command::Tool(Tool::Pencil)),("Fill",Command::Tool(Tool::Fill)),("Picker",Command::Tool(Tool::Picker)),("Eraser",Command::Tool(Tool::Eraser)),("Brush",Command::Tool(Tool::Brush)),("Size",Command::BrushSize)].into_iter().enumerate(){button(&mut out,b,8+(i%3)as i32*58,38+(i/3)as i32*30,54,27,label,command);}
            for(i,(label,command))in [("Line",Command::Tool(Tool::Line)),("Rect",Command::Tool(Tool::Rectangle)),("Oval",Command::Tool(Tool::Ellipse)),("Rotate",Command::Rotate),("Flip H",Command::Flip(true)),("Flip V",Command::Flip(false))].into_iter().enumerate(){button(&mut out,b,190+(i%3)as i32*55,38+(i/3)as i32*30,51,27,label,command);}
            for i in 0..COLORS.len(){button(&mut out,b,365+(i%10)as i32*23,38+(i/10)as i32*29,21,26,"",Command::Color(i));}
            button(&mut out,b,8,112,80,25,"Clear",Command::New);
            button(&mut out,b,94,112,110,25,"Resize 50%",Command::ResizeHalf);
        }
    },
    AppState::Document(_) if app.id==AppId::Notepad=>{
        for(i,(label,command))in [("File",Command::File),("Edit",Command::EditorMenu(1)),("Format",Command::EditorMenu(2)),("Help",Command::EditorMenu(3))].into_iter().enumerate(){button(&mut out,b,4+i as i32*62,4,58,26,label,command);}
    },
    AppState::Document(_)=>{
        for(i,(label,command))in [("File",Command::File),("Undo",Command::Undo),("Redo",Command::Redo),("Cut",Command::Cut),("Copy",Command::Copy),("Paste",Command::Paste),("Find",Command::Find),("Replace",Command::Replace)].into_iter().enumerate(){button(&mut out,b,4+i as i32*67,4,63,26,label,command);}
        if app.id==AppId::WordPad{button(&mut out,b,544,4,32,26,"B",Command::Bold);button(&mut out,b,580,4,32,26,"I",Command::Italic);}
    },
    AppState::CharacterMap{page,..}=>{button(&mut out,b,10,8,82,28,"Previous",Command::Page(-1));button(&mut out,b,98,8,82,28,"Next",Command::Page(1));button(&mut out,b,186,8,110,28,"Copy character",Command::Copy);for i in 0..96{let code=page+i;if let Some(ch)=char::from_u32(code){button(&mut out,b,12+(i%16)as i32*32,52+(i/16)as i32*38,30,34,&ch.to_string(),Command::Character(code));}}},
    AppState::TaskManager{page,offset,..}=>{
        for (i,label) in ["Applications","Processes","Performance"].iter().enumerate() {
            button(&mut out,b,12+i as i32*130,8,124,27,label,Command::TaskPage(i));
        }
        let y=b.height.saturating_sub(68)as i32;
        if *page==0 {
            for(i,t)in env.tasks.iter().take(10).enumerate(){button(&mut out,b,12,65+i as i32*27,b.width.saturating_sub(24),25,&format!("{}    {}",t.title,if t.minimized{"Minimized"}else{"Running"}),Command::SelectTask(t.id));}
            button(&mut out,b,12,y,110,28,"Switch To",Command::ActivateTask);
            button(&mut out,b,132,y,110,28,"End Task",Command::EndTask);
        } else if *page==1 {
            let count=env.processes.as_ref().map_or(0,|s|s.processes.len());
            if *offset>0 {button(&mut out,b,12,y,110,28,"Previous",Command::ProcessPage(-1));}
            if offset.saturating_add(8)<count {button(&mut out,b,132,y,110,28,"Next",Command::ProcessPage(1));}
        }
    },
    AppState::ResourceMonitor(monitor)=>{
        for (i,label) in ["Overview","CPU","Memory"].iter().enumerate() {
            button(&mut out,b,12+i as i32*130,8,124,27,label,Command::TaskPage(i));
        }
        if monitor.page!=0 {
            let count=env.processes.as_ref().map_or(0,|s|s.processes.len());
            let offset=monitor.visible_offset(count);
            let y=b.height.saturating_sub(68)as i32;
            if offset>0 {button(&mut out,b,12,y,110,28,"Previous",Command::ProcessPage(-1));}
            if offset.saturating_add(super::resource_monitor::PAGE_SIZE)<count {
                button(&mut out,b,132,y,110,28,"Next",Command::ProcessPage(1));
            }
        }
    },
    AppState::Run{..}=>{button(&mut out,b,12,112,90,28,"OK",Command::Run);},
    AppState::Keyboard{shift}=>{for(row,keys)in ["1234567890","qwertyuiop","asdfghjkl","zxcvbnm"].iter().enumerate(){for(col,key)in keys.bytes().enumerate(){let key=if *shift{key.to_ascii_uppercase()}else{key};button(&mut out,b,12+col as i32*43,36+row as i32*42,39,36,&(key as char).to_string(),Command::Type(key));}}for(i,(label,command))in [("Shift",Command::Shift),("Space",Command::Type(b' ')),("Back",Command::Type(8)),("Enter",Command::Type(13))].into_iter().enumerate(){button(&mut out,b,12+i as i32*105,212,98,32,label,command);}},
    AppState::Magnifier{..}=>{button(&mut out,b,10,8,64,28,"-",Command::Zoom(-1));button(&mut out,b,82,8,64,28,"+",Command::Zoom(1));button(&mut out,b,154,8,150,28,"Refresh capture",Command::Capture);},
    AppState::Snipping{..}=>{button(&mut out,b,10,8,100,28,"New capture",Command::Capture);button(&mut out,b,120,8,90,28,"Save BMP",Command::Save);},
    AppState::Minesweeper(_)=>{button(&mut out,b,12,8,100,28,"New game",Command::New);for i in 0..81{button(&mut out,b,18+(i%9)as i32*29,60+(i/9)as i32*29,28,28,"",Command::Game(i));}},
    AppState::Information{..}=>match app.id{
        AppId::GamesExplorer=>button(&mut out,b,20,80,220,42,"Play Minesweeper",Command::Launch(AppId::Minesweeper)),
        AppId::DefaultPrograms=>{for(i,(label,id))in [("Text documents: Notepad",AppId::Notepad),("RTF documents: WordPad",AppId::WordPad),("BMP images: Paint",AppId::Paint)].into_iter().enumerate(){button(&mut out,b,20,70+i as i32*44,300,36,label,Command::Launch(id));}},
        AppId::EaseOfAccessCenter=>{
            button(&mut out,b,16,148,244,36,"Start Magnifier",Command::Launch(AppId::Magnifier));
            button(&mut out,b,16,196,244,36,"Start On-Screen Keyboard",Command::Launch(AppId::OnScreenKeyboard));
        },
        AppId::GettingStarted|AppId::Help=>{for(i,label)in ["Desktop and applications","Files and documents","Keyboard shortcuts","System and compatibility"].iter().enumerate(){button(&mut out,b,12,55+i as i32*36,218,30,label,Command::Topic(i));}},
        _=>{}
    }
}
if app.menu{
    let items:Vec<(&str,Command)>=match app.menu_kind{
        1=>alloc::vec![("Undo    Ctrl+Z",Command::Undo),("Redo    Ctrl+Y",Command::Redo),("Cut    Ctrl+X",Command::Cut),("Copy    Ctrl+C",Command::Copy),("Paste    Ctrl+V",Command::Paste),("Delete    Del",Command::Key(super::explorer::input::DELETE)),("Find...    Ctrl+F",Command::Find),("Find Next",Command::FindNext),("Replace...",Command::Replace),("Select All    Ctrl+A",Command::Key(1))],
        2=>alloc::vec![("Word Wrap",Command::WordWrap)],
        3=>alloc::vec![("About Notepad",Command::About)],
        _=>alloc::vec![("New    Ctrl+N",Command::NewDocument),("Open...    Ctrl+O",Command::Open),("Save    Ctrl+S",Command::Save),("Save As...",Command::SaveAs),("Exit",Command::Exit)],
    };
    for(i,(label,command))in items.into_iter().enumerate(){button(&mut out,b,6+app.menu_kind as i32*62,33+i as i32*29,218,29,label,command);}
}
out}
use alloc::string::ToString;
pub fn click(app:&mut ApplicationSession,body:Rect,env:&Environment,x:i32,y:i32,right:bool)->AppAction{
    let Some(button)=buttons(app,body,env).into_iter().rev().find(|button|button.bounds.contains(x,y))else{return AppAction::None;};
    if right {if let Command::Color(i)=button.command{if let AppState::Paint(p)=&mut app.state{p.background=COLORS[i];}}if let Command::Game(index)=button.command{if let AppState::Minesweeper(g)=&mut app.state{g.flag(index);}}return AppAction::None;}
    if !matches!(button.command,Command::File|Command::EditorMenu(_)){app.menu=false;}
    match button.command {
        Command::File=>{app.menu=if app.menu_kind==0{!app.menu}else{true};app.menu_kind=0;},
        Command::EditorMenu(kind)=>{app.menu=if app.menu_kind==kind{!app.menu}else{true};app.menu_kind=kind;},
        Command::WordWrap=>if let AppState::Document(d)=&mut app.state{d.word_wrap=!d.word_wrap;},
        Command::FindNext=>{if let(Some(s),AppState::Document(d))=(&mut app.search,&mut app.state){s.key(d,13);}else{app.search=Some(super::editor_view::Search::new(false));}},
        Command::About=>app.status="Notepad — NT61, version 6.1.7601".into(),
        Command::Open=>return AppAction::Open,Command::SaveAs=>return AppAction::SaveAs,Command::NewDocument=>return AppAction::NewDocument,Command::Exit=>return AppAction::RequestClose,
        Command::Cut=>if let AppState::Document(d)=&mut app.state{return AppAction::Copy(d.cut());},
        Command::Find|Command::Replace=>app.search=Some(super::editor_view::Search::new(button.command==Command::Replace)),
        Command::Redo=>match &mut app.state{AppState::Document(d)=>d.redo(),AppState::Paint(p)=>p.redo(),_=>{}},
        Command::PaintTab(view)=>if let AppState::Paint(p)=&mut app.state{p.view_tab=view;},
        Command::Rotate=>if let AppState::Paint(p)=&mut app.state{p.rotate();},
        Command::Flip(horizontal)=>if let AppState::Paint(p)=&mut app.state{p.flip(horizontal);},
        Command::BrushSize=>if let AppState::Paint(p)=&mut app.state{p.size=if p.size>=8{1}else{p.size*2};},
        Command::Grid=>if let AppState::Paint(p)=&mut app.state{p.grid=!p.grid;},
        Command::ResizeHalf=>if let AppState::Paint(p)=&mut app.state{if let Err(message)=p.resize((p.canvas.width()/2).max(1),(p.canvas.height()/2).max(1)){app.status=message.into();}},
        Command::PaintZoom(delta)=>if let AppState::Paint(p)=&mut app.state{p.zoom=if delta==0{100}else{(p.zoom as i32+delta).clamp(25,400)as u32};},
        Command::Key(key)=>return app.key(key),Command::Save=>return app.save(),Command::Launch(id)=>return AppAction::Launch(id),
        Command::Run=>return app.run(),Command::Capture=>return AppAction::Capture,
        Command::Copy=>match &app.state{AppState::Document(d)=>return AppAction::Copy(d.selected_text()),AppState::CharacterMap{selected,..}=>return AppAction::Copy(char::from_u32(*selected).unwrap_or(' ').to_string()),_=>{}},
        Command::Paste=>return AppAction::Paste,
        Command::New=>match &mut app.state{AppState::Paint(p)=>p.clear(),AppState::Minesweeper(g)=>**g=super::minesweeper::Minesweeper::new(73+g.revealed.iter().filter(|v|**v).count()as u64),_=>{}},
        Command::Undo=>match &mut app.state{AppState::Paint(p)=>p.undo(),AppState::Document(d)=>d.undo(),_=>{}},
        Command::Bold=>if let AppState::Document(d)=&mut app.state{d.bold=!d.bold;d.dirty=true;},
        Command::Italic=>if let AppState::Document(d)=&mut app.state{d.italic=!d.italic;d.dirty=true;},
        Command::Color(i)=>if let AppState::Paint(p)=&mut app.state{p.color=COLORS[i];},
        Command::Tool(tool)=>if let AppState::Paint(p)=&mut app.state{p.tool=tool;},
        Command::Character(code)=>if let AppState::CharacterMap{selected,..}=&mut app.state{*selected=code;},
        Command::Page(direction)=>if let AppState::CharacterMap{page,..}=&mut app.state{*page=(*page as i32+direction*96).clamp(32,0xFF60)as u32;},
        Command::TaskPage(value)=>match &mut app.state {
            AppState::TaskManager{page,..}=>*page=value,
            AppState::ResourceMonitor(monitor)=>monitor.select_page(value),
            _=>{},
        },
        Command::ProcessPage(direction)=>{
            let count=env.processes.as_ref().map_or(0,|s|s.processes.len());
            match &mut app.state {
                AppState::TaskManager{offset,..}=>*offset=if direction<0 {offset.saturating_sub(8)}else{offset.saturating_add(8).min(count.saturating_sub(1)/8*8)},
                AppState::ResourceMonitor(monitor)=>monitor.scroll(direction,count),
                _=>{},
            }
        },
        Command::SelectTask(id)=>if let AppState::TaskManager{selected,..}=&mut app.state{*selected=Some(id);},
        Command::ActivateTask|Command::EndTask=>if let AppState::TaskManager{selected:Some(id),..}=&app.state{return if button.command==Command::EndTask{AppAction::EndTask(*id)}else{AppAction::Activate(*id)};},
        Command::Shift=>if let AppState::Keyboard{shift}=&mut app.state{*shift=!*shift;},Command::Type(key)=>return AppAction::Type(key),
        Command::Zoom(delta)=>if let AppState::Magnifier{zoom,..}=&mut app.state{*zoom=(*zoom as i32+delta).clamp(1,8)as u32;},
        Command::Game(index)=>if let AppState::Minesweeper(g)=&mut app.state{g.reveal(index);},
        Command::Topic(value)=>if let AppState::Information{page}=&mut app.state{*page=value;},
        Command::Flag(_)=>{},
    }AppAction::None
}
pub fn paint_bounds(body:Rect)->Rect{Rect::new(body.x+8,body.y+146,body.width.saturating_sub(16),body.height.saturating_sub(180))}
pub fn confirm_buttons(b:Rect)->[Rect;3]{[Rect::new(b.x+18,b.y+105,100,30),Rect::new(b.x+130,b.y+105,110,30),Rect::new(b.x+252,b.y+105,100,30)]}
fn preview(frame:&mut Surface,image:&Surface,b:Rect,zoom:u32){for y in 0..b.height{for x in 0..b.width{let sx=x/zoom;let sy=y/zoom;if let Some(pixel)=image.pixel(sx,sy){frame.set_pixel((b.x+x as i32)as u32,(b.y+y as i32)as u32,pixel);}}}}
pub fn draw<F:Fn(&mut Surface,&str,Rect,u32)>(app:&ApplicationSession,frame:&mut Surface,b:Rect,env:&Environment,text:F){
    let paper=if app.id==AppId::StickyNotes{0xFFFFF3A6}else{0xFFF3F5F7};frame.fill_rect(b.x,b.y,b.width,b.height,paper);
    match &app.state {
        AppState::CommandPrompt(c)=>{frame.fill_rect(b.x,b.y,b.width,b.height,0xFF0C0C0C);let count=(b.height.saturating_sub(40)/21).max(1)as usize;let lines:Vec<_>=c.lines.iter().rev().take(count.saturating_sub(1)).collect();for (i,line)in lines.into_iter().rev().enumerate(){text(frame,line,Rect::new(b.x+10,b.y+10+i as i32*21,b.width.saturating_sub(20),20),0xFFF4F4F4);}let row=c.lines.len().min(count.saturating_sub(1));text(frame,&format!("{}>{}",c.display_prompt(),c.display_input()),Rect::new(b.x+10,b.y+10+row as i32*21,b.width.saturating_sub(20),20),0xFFF4F4F4);},
        AppState::Calculator(c)=>{let r=Rect::new(b.x+14,b.y+18,272,54);aero::menu_item(frame,r,true);text(frame,&c.display,Rect::new(r.x+8,r.y+18,r.width-16,24),0xFF132A42);if c.memory!=0.0{text(frame,"M",Rect::new(b.x+292,b.y+30,30,18),0xFF243A52);}},
        AppState::Document(d)=>super::editor_view::draw(d,frame,b,&text),
        AppState::Paint(p)=>{
            frame.fill_rect(b.x,b.y+30,b.width,112,0xFFDDE7F3);
            let dest=paint_bounds(b);frame.fill_rect(dest.x,dest.y,dest.width,dest.height,0xFF8B9BAD);
            for y in 0..dest.height{for x in 0..dest.width{let sx=x*100/p.zoom+p.scroll.0;let sy=y*100/p.zoom+p.scroll.1;if let Some(pixel)=p.canvas.pixel(sx,sy){let color=if p.grid&&p.zoom>=200&&((x*100%p.zoom==0)||(y*100%p.zoom==0)){0xFFB0B0B0}else{pixel};frame.set_pixel((dest.x+x as i32)as u32,(dest.y+y as i32)as u32,color);}}}
            for(x,label)in [(12,"Tools / Brushes"),(194,"Shapes / Image"),(370,"Colors")]{text(frame,label,Rect::new(b.x+x,b.y+98,170,20),0xFF4B6180);}
            text(frame,&format!("{} x {} px    {}%    Size {}    Color 1 / Color 2",p.canvas.width(),p.canvas.height(),p.zoom,p.size),Rect::new(b.x+8,b.y+b.height as i32-27,b.width.saturating_sub(16),22),0xFF526578);
        },
        AppState::CharacterMap{selected,..}=>text(frame,&format!("U+{selected:04X}  {}",char::from_u32(*selected).unwrap_or(' ')),Rect::new(b.x+12,b.y+290,b.width-24,24),0xFF182534),
        AppState::TaskManager{page,offset,..}=>{
            if *page==0 {
                text(frame,&format!("{} desktop tasks (Explorer session)",env.tasks.len()),Rect::new(b.x+12,b.y+40,b.width.saturating_sub(24),18),0xFF243A52);
            }else if let Some(snapshot)=&env.processes {
                if *page==1 {
                    text(frame,"Image / PID / Session / Working set / User",Rect::new(b.x+12,b.y+40,b.width.saturating_sub(24),20),0xFF175E99);
                    for (i,p) in snapshot.processes.iter().skip(*offset).take(8).enumerate() {
                        text(frame,&format!("{}  |  {}  |  {}  |  {} K  |  {}",p.name,p.pid,p.session,p.working_set/1024,p.user),
                            Rect::new(b.x+12,b.y+68+i as i32*29,b.width.saturating_sub(24),24),0xFF182534);
                    }
                }else{
                    let working_set=snapshot.processes.iter().fold(0u64,|sum,p|sum.saturating_add(p.working_set));
                    for (i,line) in [format!("Logical processors: {}",snapshot.cpu_count),
                        format!("Kernel processes: {}",snapshot.processes.len()),
                        format!("Sum of process working sets: {} K",working_set/1024),
                        "Counters refresh once per second while visible.".into()].iter().enumerate() {
                        text(frame,line,Rect::new(b.x+16,b.y+65+i as i32*34,b.width.saturating_sub(32),26),0xFF182534);
                    }
                }
            }else{text(frame,"Kernel process information is unavailable.",Rect::new(b.x+12,b.y+65,b.width.saturating_sub(24),24),0xFF182534);}
        },
        AppState::ResourceMonitor(monitor)=>{
            if let Some(snapshot)=&env.processes {
                if monitor.page==0 {
                    let sum=snapshot.processes.iter().fold(0u64,|sum,p|sum.saturating_add(p.working_set));
                    let lines=[format!("Logical processors: {}",snapshot.cpu_count),
                        format!("Kernel processes: {}",snapshot.processes.len()),
                        format!("Sum of process working sets: {} K",sum/1024),
                        "CPU: cumulative scheduler accounting ticks.".into(),
                        "Memory: process working sets, sorted by size.".into(),
                        "Disk / network I/O counters are unavailable.".into()];
                    for(i,line)in lines.iter().enumerate(){text(frame,line,Rect::new(b.x+16,b.y+65+i as i32*34,b.width.saturating_sub(32),26),0xFF182534);}
                } else {
                    text(frame,if monitor.page==1 {"Image / PID / CPU accounting ticks"}else{"Image / PID / Working set (K) / User"},
                        Rect::new(b.x+12,b.y+40,b.width.saturating_sub(24),20),0xFF175E99);
                    for(i,p)in monitor.sorted(snapshot).into_iter().skip(monitor.visible_offset(snapshot.processes.len())).take(super::resource_monitor::PAGE_SIZE).enumerate(){
                        let line=if monitor.page==1 {format!("{}  |  {}  |  {}",p.name,p.pid,p.cpu_ticks)}
                            else{format!("{}  |  {}  |  {} K  |  {}",p.name,p.pid,p.working_set/1024,p.user)};
                        text(frame,&line,Rect::new(b.x+12,b.y+68+i as i32*29,b.width.saturating_sub(24),24),0xFF182534);
                    }
                }
            } else {text(frame,"Kernel process information is unavailable.",Rect::new(b.x+12,b.y+65,b.width.saturating_sub(24),24),0xFF182534);}
        },
        AppState::Run{command}=>{text(frame,"Type the name of a program to open it.",Rect::new(b.x+12,b.y+18,b.width-24,24),0xFF182534);let r=Rect::new(b.x+12,b.y+58,b.width.saturating_sub(24),32);frame.fill_rect(r.x,r.y,r.width,r.height,0xFFFFFFFF);text(frame,&format!("{command}|"),Rect::new(r.x+6,r.y+7,r.width-12,22),0xFF182534);},
        AppState::Keyboard{..}=>text(frame,"Keys go to the last active text window.",Rect::new(b.x+12,b.y+8,b.width-24,22),0xFF182534),
        AppState::Magnifier{zoom,image}=>{text(frame,&format!("{}%",zoom*100),Rect::new(b.x+320,b.y+12,80,22),0xFF182534);if let Some(image)=image{preview(frame,image,Rect::new(b.x+12,b.y+48,b.width.saturating_sub(24),b.height.saturating_sub(90)),*zoom);}},
        AppState::Snipping{image}=>if let Some(image)=image{preview(frame,image,Rect::new(b.x+12,b.y+48,b.width.saturating_sub(24),b.height.saturating_sub(90)),1);}else{text(frame,"Capture the desktop beneath this window; save as BMP.",Rect::new(b.x+12,b.y+65,b.width-24,40),0xFF182534);},
        AppState::Minesweeper(g)=>{text(frame,&format!("Mines: 10    Flags: {}    {:?}",g.flags.iter().filter(|f|**f).count(),g.outcome),Rect::new(b.x+130,b.y+15,b.width.saturating_sub(142),22),0xFF182534);},
        AppState::Information{page}=>{let lines:Vec<String>=match app.id{
            AppId::SystemInformation=>alloc::vec!["System Summary".into(),"OS version: 6.1.7601 (native NT61)".into(),format!("System type: {}",architecture()),format!("Display: {} x {}, True Color (32 bit)",env.width,env.height),"Display stack: VirtIO GPU / boot framebuffer".into(),format!("Network adapters detected: {}",env.network_adapters),format!("Audio device detected: {}",env.audio),format!("Desktop tasks: {}",env.tasks.len()),env.processes.as_ref().map_or_else(||"Kernel process information unavailable".into(),|s|format!("Logical processors: {}   Processes: {}",s.cpu_count,s.processes.len())),"Win32 binary compatibility: incomplete".into()],
            AppId::GamesExplorer=>alloc::vec!["Games".into(),"Minesweeper: Beginner, 9 x 9, 10 mines".into(),"Additional games are unavailable.".into()],
            AppId::EaseOfAccessCenter=>alloc::vec!["Ease of Access Center".into(),"Quick access to common tools".into(),"Magnify the desktop or type with the mouse.".into(),"Narration and system-wide settings are unavailable.".into()],
            AppId::DefaultPrograms=>core::iter::once(String::from("Default file associations")).chain(super::associations::ASSOCIATIONS.iter().map(|(ext,app)|format!(".{} -> {}",ext,app.application().title))).collect(),
            _=>{let topics:[&[&str];4]=[&["Welcome to NT61","Start: search by app name or executable.","Taskbar: switch, minimize and restore tasks.","Right-click the taskbar for Task Manager."],&["Files and documents","Explorer: Back, Up, New Folder, Copy, Paste, Delete.","Editors: Save, Undo, Copy, Paste.","Files currently live in this desktop session."],&["Keyboard shortcuts","Ctrl+S: save document   Ctrl+Z: undo","Ctrl+C / Ctrl+V: copy and paste text","Run: calc, mspaint, taskmgr, notepad, explorer"],&["System and compatibility","Native binaries target NT version 6.1.7601.","ARM64, RISC-V and LoongArch are NT61 ports.","Full Microsoft Win32/COM/.NET is not available.","Help content is local; no network is required."]];topics[*page%4].iter().map(|s|String::from(*s)).collect()}
        };let x=if matches!(app.id,AppId::GettingStarted|AppId::Help){250}else{16};for(i,line)in lines.iter().enumerate(){text(frame,line,Rect::new(b.x+x,b.y+12+i as i32*27,b.width.saturating_sub(x as u32+12),24),if i==0{0xFF175E99}else{0xFF182534});}},
    }
    for button in buttons(app,b,env){let selected=match (&app.state,&button.command){(AppState::CharacterMap{selected,..},Command::Character(c))=>selected==c,(AppState::TaskManager{selected,..},Command::SelectTask(id))=>*selected==Some(*id),(AppState::TaskManager{page,..},Command::TaskPage(id))=>page==id,(AppState::ResourceMonitor(monitor),Command::TaskPage(id))=>monitor.page==*id,(AppState::Paint(p),Command::Tool(tool))=>p.tool==*tool,(AppState::Paint(p),Command::Color(i))=>p.color==COLORS[*i],(AppState::Document(d),Command::Bold)=>d.bold,(AppState::Document(d),Command::Italic)=>d.italic,(AppState::Document(d),Command::WordWrap)=>d.word_wrap,_=>false};aero::menu_item(frame,button.bounds,selected||app.hot==Some(button.bounds));let mut label=button.label;
        if let Command::Game(i)=button.command{if let AppState::Minesweeper(g)=&app.state{if g.flags[i]{label="F".into();}else if g.mines[i]&&(g.revealed[i]||g.outcome==Outcome::Lost){label="*".into();}else if g.revealed[i]{frame.fill_rect(button.bounds.x,button.bounds.y,button.bounds.width,button.bounds.height,0xFFE0E5EA);let n=g.adjacent(i);if n!=0{label=format!("{n}");}}}}
        if let Command::Color(i)=button.command{frame.fill_rect(button.bounds.x+3,button.bounds.y+3,button.bounds.width-6,button.bounds.height-6,COLORS[i]);}
        text(frame,&label,Rect::new(button.bounds.x+6,button.bounds.y+7,button.bounds.width.saturating_sub(10),button.bounds.height.saturating_sub(7)),0xFF182534);
    }
    let status=if app.status.is_empty(){match app.id{AppId::Minesweeper=>"Left click: reveal   Right click: flag   N: new game",AppId::Paint=>"Pencil / Eraser / Fill   |   BMP export",AppId::WordPad=>"RTF export with document-wide bold and italic",AppId::SnippingTool=>"Desktop capture; rectangular selection is pending",_=>"Ready"}}else{&app.status};if !matches!(app.state,AppState::Document(_)|AppState::Paint(_))||!app.status.is_empty(){text(frame,status,Rect::new(b.x+12,b.y+b.height.saturating_sub(27)as i32,b.width.saturating_sub(24),22),0xFF526578);}
    if let Some(search)=&app.search{super::editor_view::search_draw(search,frame,b,&text);}
    if let Some(dialog)=&app.dialog{super::file_dialog::draw(dialog,frame,b,&text);}
    if app.confirm{
        frame.fill_rect(b.x+8,b.y+38,b.width.saturating_sub(16),112,0xFFF0F0F0);
        text(frame,&format!("Save changes to {}?",app.file_name()),Rect::new(b.x+18,b.y+56,b.width.saturating_sub(36),40),0xFF163B60);
        for(r,label)in confirm_buttons(b).into_iter().zip(["Save","Don't Save","Cancel"]){aero::menu_item(frame,r,true);text(frame,label,Rect::new(r.x+6,r.y+5,r.width-12,24),0xFF182534);}
    }
}

fn architecture()->&'static str { if cfg!(target_arch="x86_64") {"x86_64"} else if cfg!(target_arch="aarch64") {"aarch64"} else if cfg!(target_arch="riscv64") {"riscv64"} else {"loongarch64"} }
