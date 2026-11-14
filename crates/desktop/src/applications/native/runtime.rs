//! Freestanding native application host. The image builder specializes IDENTITY per PE.
extern crate alloc;
#[allow(dead_code)]
#[path="../../bin/explorer/arch/mod.rs"] mod arch;
use nt61_desktop::{applications::{AppId,view},graphics::{Surface,Rect,font::FontLib},shell::ShellModel,theme::Theme,user_allocator::StaticHeapAllocator};
#[global_allocator] static HEAP:StaticHeapAllocator<{32*1024*1024}>=StaticHeapAllocator::new();
#[panic_handler]fn panic(_: &core::panic::PanicInfo)->!{arch::debug_str("[APPLICATION] panic\r\n");arch::exit_process(1)}
/// Kept in a writable load segment, read volatile: no constant folding after packaging specialization.
#[used]
#[unsafe(no_mangle)]
pub static mut NT61_APP_IDENTITY:[u8;24]=*b"NT61APP_ID_V1___\x11\x00\x00\x00\x61\x07\x01\x00";
#[unsafe(no_mangle)]pub extern "C" fn kernel_entry()->!{
    // SAFETY: immutable after the image loader maps the package-specialized identity.
    let raw=u32::from_le_bytes(unsafe{core::ptr::read_volatile(core::ptr::addr_of!(NT61_APP_IDENTITY).cast::<u8>().add(16).cast::<[u8;4]>())});
    let Some(id)=AppId::from_raw(raw)else{arch::exit_process(2);};
    arch::debug_str("[APPLICATION] ");arch::debug_str(id.application().executable);arch::debug_str("\r\n");
    let mut fb=arch::DesktopFramebufferInfo::default();arch::desktop_info(&mut fb);if fb.width<320||fb.height<240{arch::exit_process(3);}
    let Some(font)=FontLib::ui().ok()else{arch::exit_process(4);};
    let mut model=ShellModel::new(Theme::aero());
    model.files=match nt61_desktop::file::FileSystem::mounted(nt61_desktop::file::Backend(nt61_desktop::platform::services::transfer)) {
        Ok(files)=>files,Err(_)=>{arch::debug_str("[APPLICATION] system volume mount failed\r\n");arch::exit_process(5);}
    };
    model.refresh_controls(fb.width,fb.height);let window=model.launch_application(id);
    let mut frame=Surface::new(fb.width,fb.height);let mut rgba=alloc::vec![0u8;frame.pixels().len()*4];
    let mut dirty=true;let mut mouse=arch::DesktopMouseState::default();let mut previous=0;let mut cursor=(0,0);let mut old_cursor=(0,0);
    let mut next_process_poll=0u64;let mut next_poll=0u64;let mut poll_index=0usize;let mut search_index=0usize;
    loop{
        let ticks=arch::shell_ticks();
        for app in &mut model.applications{app.input_time=ticks;}
        for explorer in &mut model.explorers { explorer.input_time=Some(ticks); }
        if dirty{
            frame.fill_rect(0,0,fb.width,fb.height,0xFF285A7F);
            let env=model.application_environment();for w in &model.taskbar.windows{if w.minimized{continue;}let bounds=Rect::new(w.x,w.y,w.width as u32,w.height as u32);nt61_desktop::compositor::aero::rounded_window(&mut frame,bounds,5,0xFFD3E2EF);
                let _=font.draw_text(&mut frame,&w.title,w.x+12,w.y+7,16,0xFF172A40);let _=font.draw_text(&mut frame,"X",w.x+w.width-28,w.y+7,16,0xFF172A40);
                if let Some(body)=model.application_body(w.id){let draw=|s:&mut Surface,t:&str,b:Rect,c:u32|{let limited=truncate(&font,t,b.width);let _=font.draw_text(s,&limited,b.x,b.y,16,c);};if let Some(app)=model.application(w.id){view::draw(app,&mut frame,body,&env,draw);}else if let Some(state)=model.explorers.iter().find(|e|e.window_id==w.id){nt61_desktop::applications::explorer::draw(state,&model.files,&mut frame,body,draw);}}
            }
            nt61_desktop::graphics::present::update_rgba(&frame,&mut rgba);unsafe{arch::draw_rgba(rgba.as_ptr(),0,0,fb.width,fb.height,fb.width as usize*4);}
            present_cursor(&frame,cursor,cursor);old_cursor=cursor;
            dirty=false;
        }
        let mut key=0;if unsafe{arch::poll_key(&mut key)}==1{model.handle_window_key(key);dirty=true;}
        if unsafe{arch::poll_mouse(&mut mouse)}!=0{cursor=(mouse.x,mouse.y);let down=mouse.buttons&1!=0&&previous&1==0;let right=mouse.buttons&2!=0&&previous&2==0;
            if let Some(target)=model.window_hit_test(mouse.x,mouse.y){if down{model.select_window(target);if model.handle_window_control_click(target,mouse.x,mouse.y,fb.width,fb.height){dirty=true;}else{if model.needs_capture(target,mouse.x,mouse.y){model.capture_application(target,frame.clone());}model.application_pointer(target,mouse.x,mouse.y,true,true,false);dirty=true;}}else if right{model.application_pointer(target,mouse.x,mouse.y,true,false,true);dirty=true;}else if mouse.buttons&1!=0{dirty|=model.application_pointer(target,mouse.x,mouse.y,false,true,false);}else{dirty|=model.application_pointer(target,mouse.x,mouse.y,false,false,false);}}
            if model.taskbar.windows.is_empty(){arch::exit_process(0);}previous=mouse.buttons;
        }
        if cursor!=old_cursor&&!dirty{present_cursor(&frame,old_cursor,cursor);old_cursor=cursor;}
        dirty|=model.flush_application_input(ticks);
        if model.taskbar.windows.is_empty(){arch::exit_process(0);}
        for explorer in &mut model.explorers { dirty|=explorer.flush_input(&mut model.files); }
        let count=model.explorers.len();
        for step in 0..count {
            let index=(search_index+step)%count;
            if model.explorers[index].work_pending(){
                model.explorers[index].work(&mut model.files);search_index=(index+1)%count;dirty=true;break;
            }
        }
        if ticks>=next_poll&&count!=0{
            next_poll=ticks.saturating_add(250);let index=poll_index%count;poll_index=(index+1)%count;
            dirty|=model.explorers[index].poll_directory(&mut model.files);
        }
        if ticks>=next_process_poll {
            next_process_poll=ticks.saturating_add(1000);
            if model.applications.iter().any(|app|matches!(app.id,AppId::TaskManager|AppId::SystemInformation|AppId::ResourceMonitor)) {
                if let Ok(snapshot)=nt61_desktop::platform::services::processes() {
                    if model.process_snapshot.as_ref()!=Some(&snapshot){model.process_snapshot=Some(snapshot);dirty=true;}
                }
            }
        }
        // Keep input polling responsive without allocating a new surface on idle frames.
        let _=window;
        core::hint::spin_loop();
    }
}
fn truncate(font:&FontLib,text:&str,width:u32)->alloc::string::String{let mut out=alloc::string::String::new();for ch in text.chars(){out.push(ch);if font.measure(&out,16).is_some_and(|m|m.width>width){out.pop();break;}}out}
fn present_cursor(frame:&Surface,old:(i32,i32),new:(i32,i32)){
    for ((x,y),cursor) in [(old,false),(new,true)]{let mut pixels=[0u8;12*16*4];for row in 0..16{for col in 0..12{let pixel=if cursor&&col<=row/2&&col<8{if col==0||col==row/2{0xFF000000}else{0xFFFFFFFF}}else{frame.pixel((x+col as i32)as u32,(y+row as i32)as u32).unwrap_or(0)};let i=(row*12+col)*4;pixels[i..i+4].copy_from_slice(&[(pixel>>16)as u8,(pixel>>8)as u8,pixel as u8,(pixel>>24)as u8]);}}if x>=0&&y>=0{unsafe{arch::draw_rgba(pixels.as_ptr(),x as u32,y as u32,12,16,48);}}}
}
