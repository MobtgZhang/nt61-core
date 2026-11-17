//! Authorized session and Task Manager service boundary.
use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use nt61_abi::desktop_bridge::*;
static SECURE:AtomicBool=AtomicBool::new(false);
static LOCKED: AtomicBool = AtomicBool::new(false);
fn desktop() -> Result<(),u32> {
    let thread=crate::ps::thread::get_current_ethread();
    if thread.is_null(){return Err(STATUS_ACCESS_DENIED);}
    // SAFETY: the executing ETHREAD retains its owning process.
    let valid=unsafe{!(*thread).threads_process.is_null() && (*(*thread).threads_process).unique_process_id==crate::servers::smss::PID_EXPLORER};
    if valid{Ok(())}else{Err(STATUS_ACCESS_DENIED)}
}
fn user()->Result<String,u32>{
    let process=crate::ps::process::get_by_pid(crate::servers::smss::PID_EXPLORER).ok_or(STATUS_NOT_FOUND)?;
    // SAFETY: Explorer retains its primary token.
    let token=unsafe{process.get_token().as_ref()}.ok_or(STATUS_ACCESS_DENIED)?;
    crate::se::users::account_name(&token.user).ok_or(STATUS_ACCESS_DENIED)
}
fn admin()->bool {
    crate::ps::process::get_by_pid(crate::servers::smss::PID_EXPLORER)
        .and_then(|p|unsafe{p.get_token().as_ref()}).is_some_and(|t|t.is_admin())
}
pub fn session(action:&str, input:&[u8])->Result<Vec<u8>,u32>{
    let thread=crate::ps::thread::get_current_ethread();
    let logonui=unsafe{!thread.is_null()&&!(*thread).threads_process.is_null()&&(*(*thread).threads_process).unique_process_id==crate::servers::smss::PID_LOGONUI};
    if !logonui {desktop()?;} else if !matches!(action,"shutdown"|"restart"|"sleep"){return Err(STATUS_ACCESS_DENIED);}
    match action {
        "user"=>Ok(user()?.into_bytes()),
        "secure"=>{SECURE.store(true,Ordering::Release);Ok(Vec::new())},
        "unsecure"=>{if LOCKED.load(Ordering::Acquire){return Err(STATUS_ACCESS_DENIED);}SECURE.store(false,Ordering::Release);Ok(Vec::new())},
        "lock"=>{LOCKED.store(true,Ordering::Release);Ok(Vec::new())},
        "logoff"=>{
            let caller=user()?;
            crate::ps::process::iterate_processes(|id,p|{
                let process=unsafe{&*p};
                let owner=unsafe{process.get_token().as_ref()}.and_then(|t|crate::se::users::account_name(&t.user));
                if process.session_id!=0&&id>1024&&id!=crate::servers::smss::PID_EXPLORER&&id!=crate::servers::smss::PID_LOGONUI&&owner.as_deref()==Some(caller.as_str()){
                    unsafe{(*p).exit_status=0;(*p).exit_time=crate::hal::pit::get_system_time_ms().max(1);crate::ps::process::request_process_members_termination(p);}
                }true
            });
            let _ = crate::drivers::audio::system_sounds::play("logoff");
            LOCKED.store(true,Ordering::Release);Ok(Vec::new())
        },
        "unlock"=>{
            let name=user()?;
            if !crate::se::users::verify_interactive_password(&name,input){return Err(STATUS_ACCESS_DENIED);}
            LOCKED.store(false,Ordering::Release);Ok(Vec::new())
        },
        "switch"=>{
            let split=input.iter().position(|b|*b==0).ok_or(STATUS_INVALID_PARAMETER)?;
            let name=core::str::from_utf8(&input[..split]).map_err(|_|STATUS_INVALID_PARAMETER)?;
            let token=crate::se::logon::interactive_logon(name,&input[split+1..],1).ok_or(STATUS_ACCESS_DENIED)?;
            let process=crate::ps::process::get_by_pid(crate::servers::smss::PID_EXPLORER).ok_or(STATUS_NOT_FOUND)?;
            process.set_token(token);crate::drivers::audio::system_sounds::load_for(name);LOCKED.store(false,Ordering::Release);
            let _ = crate::drivers::audio::system_sounds::play("logon");Ok(Vec::new())
        },
        "password"=>{
            let split=input.iter().position(|b|*b==0).ok_or(STATUS_INVALID_PARAMETER)?;
            if crate::se::users::change_interactive_password(&user()?,&input[..split],&input[split+1..]) {Ok(Vec::new())}else{Err(STATUS_ACCESS_DENIED)}
        },
        "sleep"=>Err(STATUS_NOT_SUPPORTED), // Never call the unimplemented S3 save/restore path.
        "shutdown"|"restart"=>{
            if !logonui&&!admin(){return Err(STATUS_ACCESS_DENIED);}
            crate::hal::serial::write_string_always(if action=="restart"{"[SESSION] restarting\r\n"}else{"[SESSION] shutting down\r\n"});
            let _=crate::registry::flush::flush_sam_hive();
            // Let the real DMA drain before cutting power. A missing output
            // device must not prevent a shutdown.
            crate::drivers::audio::hda_pcm::stop();
            if let Ok(ms) = crate::drivers::audio::system_sounds::play("shutdown") {
                let start = crate::hal::pit::get_system_time_ms();
                while crate::hal::pit::get_system_time_ms().saturating_sub(start) < ms + 200 {
                    crate::drivers::audio::hda_pcm::poll(); core::hint::spin_loop();
                }
            }
            crate::hal::common::power::return_to_firmware(if action=="restart"{crate::hal::common::power::FirmwareAction::Reboot}else{crate::hal::common::power::FirmwareAction::Shutdown})
        },
        _=>Err(STATUS_INVALID_PARAMETER)
    }
}
pub fn hotkeys()->Result<Vec<u8>,u32>{desktop()?;Ok(crate::hal::common::desktop_hotkeys::take().to_le_bytes().to_vec())}
pub fn monitor()->Result<Vec<u8>,u32>{
    let (total,idle)=crate::ke::scheduler::activity_ticks();
    let values=[crate::hal::pit::get_system_time_ms(),crate::mm::get_total_physical(),crate::mm::get_free_physical(),crate::ps::thread::THREAD_COUNT.load(Ordering::Relaxed) as u64,crate::mm::pool::get_stats(crate::mm::pool::PoolType::NonPaged).total_bytes,total,idle];
    let mut bytes:Vec<u8>=values.into_iter().flat_map(u64::to_le_bytes).collect();
    let interfaces=crate::drivers::net::tcpip::get_interfaces();
    bytes.extend_from_slice(&(interfaces.len()as u32).to_le_bytes());
    for n in interfaces {
        for v in [u64::from(n.interface_index),u64::from(n.up),n.link_speed,n.bytes_sent,n.bytes_received] {bytes.extend_from_slice(&v.to_le_bytes());}
    }
    Ok(bytes)
}
pub fn services()->Result<Vec<u8>,u32>{
    let records=crate::servers::services::service_snapshot();
    let mut bytes=b"SVC2".to_vec();
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    for (name,display,group,status,pid,start) in records {
        // Indirect resource names remain in the SCM baseline. Until the resource
        // loader resolves them, show the stable service name in this UI.
        let display=if display.starts_with('@'){name.clone()}else{display};
        if [name.len(),display.len(),group.len()].iter().any(|n|*n>MAX_PATH)||bytes.len()+32+name.len()+display.len()+group.len()>MAX_TRANSFER{return Err(STATUS_BUFFER_TOO_SMALL);}
        for word in [name.len() as u32, display.len() as u32, group.len() as u32, status.current_state as u32, pid, start as u32, status.controls_accepted, status.win32_exit_code] {bytes.extend_from_slice(&word.to_le_bytes());}
        for text in [&name,&display,&group] {bytes.extend_from_slice(text.as_bytes());}
    }
    Ok(bytes)
}
pub fn service_control(name:&str,input:&[u8])->Result<Vec<u8>,u32>{
    desktop()?;if LOCKED.load(Ordering::Acquire)||!admin(){return Err(STATUS_ACCESS_DENIED);}
    let result=match input {b"start"=>crate::servers::services::start_service_result(name),b"stop"=>crate::servers::services::stop_service_result(name),_=>return Err(STATUS_INVALID_PARAMETER)};
    result.map(|()|Vec::new()).map_err(|code|0xC0070000|code)
}
pub fn task_control(pid:&str,input:&[u8])->Result<Vec<u8>,u32>{
    desktop()?;if LOCKED.load(Ordering::Acquire){return Err(STATUS_ACCESS_DENIED);}
    let pid=pid.parse::<u64>().map_err(|_|STATUS_INVALID_PARAMETER)?;
    let command=core::str::from_utf8(input).map_err(|_|STATUS_INVALID_PARAMETER)?;
    let priority=if let Some(p)=command.strip_prefix("priority:"){let p=p.parse::<i8>().map_err(|_|STATUS_INVALID_PARAMETER)?;if ![4,6,8,10,13].contains(&p){return Err(STATUS_INVALID_PARAMETER);}Some(p)}else{None};
    let cpus=crate::ke::scheduler::get_cpu_count().clamp(1,64);
    let all=u64::MAX.checked_shr(64-cpus).unwrap_or(u64::MAX);
    let affinity=match command {"affinity:all"=>Some(all),"affinity:1"=>Some(1),_=>None};
    if !matches!(command,"end"|"tree")&&priority.is_none()&&affinity.is_none(){return Err(STATUS_INVALID_PARAMETER);}
    let caller=user()?;let administrator=admin();
    let mut targets=alloc::vec![pid];
    if command=="tree" {
        let mut parents=Vec::new();
        crate::ps::process::iterate_processes(|id,p|{parents.push((id,unsafe{(*p).spare5}));true});
        loop {let before=targets.len();for (child,parent)in &parents{if targets.contains(parent)&&!targets.contains(child){targets.push(*child);}}if before==targets.len(){break;}}
    }
    // Validate every tree member before changing any target.
    let mut found=0;let mut denied=false;
    crate::ps::process::iterate_processes(|id,p|{
        if !targets.contains(&id){return true;}
        found+=1;let process=unsafe{&*p};
        let owner=unsafe{process.get_token().as_ref()}.and_then(|t|crate::se::users::account_name(&t.user));
        if process.session_id==0||id<=1024||id==crate::servers::smss::PID_EXPLORER||id==crate::servers::smss::PID_LOGONUI||(!administrator&&owner.as_deref()!=Some(caller.as_str())){denied=true;}true
    });
    if denied{return Err(STATUS_ACCESS_DENIED);}if found!=targets.len(){return Err(STATUS_NOT_FOUND);}
    crate::ps::process::iterate_processes(|id,p|{
        if !targets.contains(&id){return true;}
        // SAFETY: registry ownership pins all process and member allocations.
        unsafe{
            if matches!(command,"end"|"tree"){
                (*p).exit_status=1;(*p).exit_time=crate::hal::pit::get_system_time_ms().max(1);
                crate::ps::process::request_process_members_termination(p);
            }else{
                if let Some(priority)=priority{(*p).base_priority=priority;}
                crate::ps::process::configure_process_members(p,priority,affinity);
            }
        }true
    });
    Ok(Vec::new())
}

/// The trusted shell owns the framebuffer/input while credentials or security
/// options are visible. Foreign native applications cannot steal those events.
pub fn desktop_io_allowed()->bool {
 let t=crate::ps::thread::get_current_ethread();
 if t.is_null() { return false; }
 let p=unsafe { (*t).threads_process };
 if p.is_null() { return false; }
 let process=unsafe { &*p };
 if matches!(process.unique_process_id,crate::servers::smss::PID_EXPLORER|crate::servers::smss::PID_LOGONUI) {return true;}
 if LOCKED.load(Ordering::Acquire)||SECURE.load(Ordering::Acquire) {return false;}
 // Disconnected users keep their processes but cannot paint the active desktop
 // or consume another user's keyboard/mouse events after a switch.
 let owner=unsafe { process.get_token().as_ref() }.and_then(|token|crate::se::users::account_name(&token.user));
 owner.is_some() && owner==user().ok()
}

/// Modesetting is restricted to the unlocked desktop session.
pub fn display_set(path:&str)->Result<Vec<u8>,u32>{
    desktop()?;if LOCKED.load(Ordering::Acquire){return Err(STATUS_ACCESS_DENIED);}
    const SAVED:&str="C:\\Windows\\System32\\config\\display-mode.bin";
    if path=="keep" {
        let info=crate::hal::common::framebuffer_impl::info();
        let bytes=[info.width.to_le_bytes(),info.height.to_le_bytes()].concat();
        match crate::fs::system_volume::read(SAVED) {
            Ok(_)=>crate::fs::system_volume::write(SAVED,&bytes)?,
            Err(_)=>crate::fs::system_volume::create(SAVED,&bytes)?,
        }
        crate::fs::system_volume::flush()?;
        crate::hal::serial::write_string_always("[DISPLAY] saved mode persisted\r\n");
        return Ok(Vec::new());
    }
    if path=="restore" {
        let bytes=crate::fs::system_volume::read(SAVED)?;
        if bytes.len()!=8{return Err(STATUS_INVALID_PARAMETER);}
        let w=u32::from_le_bytes(bytes[..4].try_into().unwrap());
        let h=u32::from_le_bytes(bytes[4..].try_into().unwrap());
        return if crate::drivers::bootvid::win7::change_display_mode(w,h){
            crate::hal::serial::write_string_always("[DISPLAY] saved mode restored\r\n");
            Ok(Vec::new())
        }else{Err(nt61_abi::desktop_bridge::STATUS_NOT_SUPPORTED)};
    }
    let (w,h)=path.split_once('x').ok_or(STATUS_INVALID_PARAMETER)?;
    let w=w.parse().map_err(|_|STATUS_INVALID_PARAMETER)?;
    let h=h.parse().map_err(|_|STATUS_INVALID_PARAMETER)?;
    if !crate::drivers::bootvid::win7::change_display_mode(w,h){return Err(nt61_abi::desktop_bridge::STATUS_NOT_SUPPORTED);}
    Ok(Vec::new())
}

pub fn accounts()->Result<Vec<u8>,u32>{
    let t=crate::ps::thread::get_current_ethread();
    let trusted=unsafe{!t.is_null()&&!(*t).threads_process.is_null()&&matches!((*(*t).threads_process).unique_process_id,crate::servers::smss::PID_EXPLORER|crate::servers::smss::PID_LOGONUI)};
    if !trusted{return Err(STATUS_ACCESS_DENIED);}
    let rows=crate::se::users::interactive_accounts();
    let mut out=b"USR1".to_vec();out.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    for (rid,name,enabled,admin,picture) in rows {
        if out.len()+16+name.len()>MAX_TRANSFER{return Err(STATUS_BUFFER_TOO_SMALL);}
        for v in [rid,u32::from(enabled)|(u32::from(admin)<<1),picture,name.len()as u32] {out.extend_from_slice(&v.to_le_bytes());}
        out.extend_from_slice(name.as_bytes());
    }Ok(out)
}
pub fn account_control(name:&str,input:&[u8])->Result<Vec<u8>,u32>{
    desktop()?;
    if LOCKED.load(Ordering::Acquire)||SECURE.load(Ordering::Acquire){return Err(STATUS_ACCESS_DENIED);}
    let caller=user()?;
    let (command,value)=input.split_at(input.iter().position(|b|*b==0).unwrap_or(input.len()));
    let value=if value.is_empty(){value}else{&value[1..]};
    let own=caller.eq_ignore_ascii_case(name);
    if !admin() && !(own&&command==b"picture") {return Err(STATUS_ACCESS_DENIED);}
    match command {
        b"create"=>{
            if !crate::se::users::valid_account_name(name)||value.len()>32{return Err(STATUS_INVALID_PARAMETER);}
            if crate::se::users::lookup_by_name(name).is_some(){return Err(STATUS_INVALID_PARAMETER);}
            // A user profile has actual directories, shared by shell namespaces and desktop.
            for suffix in ["", "\\Desktop", "\\Documents", "\\Downloads", "\\Music", "\\Pictures", "\\Videos", "\\Recent"] {
                let path=alloc::format!("C:\\Users\\{name}{suffix}");
                if crate::fs::system_volume::list(&path).is_err(){crate::fs::system_volume::mkdir(&path)?;}
            }
            crate::fs::system_volume::flush()?;
            crate::se::users::create_user(name,value,true).map_err(|error| {
                use crate::se::users::CreateUserError;
                match error {
                    CreateUserError::DuplicateName => 0xC000_0063, // STATUS_USER_EXISTS
                    CreateUserError::RidExhausted | CreateUserError::OutOfMemory => 0xC000_009A,
                    CreateUserError::PersistenceFailure => 0xC000_0185, // STATUS_IO_DEVICE_ERROR
                    _ => STATUS_INVALID_PARAMETER,
                }
            })?;
        },
        b"picture"|b"type"|b"enabled"=>{
            let n=core::str::from_utf8(value).map_err(|_|STATUS_INVALID_PARAMETER)?.parse::<u32>().map_err(|_|STATUS_INVALID_PARAMETER)?;
            if command!=b"picture"&&n>1{return Err(STATUS_INVALID_PARAMETER);}
            if command==b"enabled"&&n==0&&own{return Err(STATUS_ACCESS_DENIED);}
            if !crate::se::users::configure_account(name,if command==b"type"{Some(n!=0)}else{None},if command==b"enabled"{Some(n!=0)}else{None},if command==b"picture"{Some(n)}else{None}){return Err(STATUS_ACCESS_DENIED);}
        },
        _=>return Err(STATUS_NOT_SUPPORTED)
    }Ok(Vec::new())
}

/// Sound playback is limited to the trusted shell / credential UI; callers
/// choose a known event, never an arbitrary filesystem path.
pub fn set_audio_output(volume:u8,muted:bool)->Result<(),u32>{
    desktop()?;
    if LOCKED.load(Ordering::Acquire)||SECURE.load(Ordering::Acquire){return Err(STATUS_ACCESS_DENIED);}
    if volume>100{return Err(STATUS_INVALID_PARAMETER);}
    use crate::drivers::audio::system_sounds;
    let mut config=system_sounds::configuration();
    if config.volume==volume&&config.muted==muted{return Ok(());}
    config.volume=volume;config.muted=muted;
    system_sounds::save_for(&user()?,config)
}
pub fn sound(event: &str) -> Result<Vec<u8>,u32> {
    let thread = crate::ps::thread::get_current_ethread();
    let allowed = unsafe { !thread.is_null() && !(*thread).threads_process.is_null()
        && matches!((*(*thread).threads_process).unique_process_id,
            crate::servers::smss::PID_EXPLORER | crate::servers::smss::PID_LOGONUI) };
    if !allowed { return Err(STATUS_ACCESS_DENIED); }
    use crate::drivers::audio::{hda_pcm,system_sounds};
    if event=="settings"{let mut b=system_sounds::configuration().bytes().to_vec();b.push(u8::from(hda_pcm::ready()));return Ok(b);}
    if let Some((key,value))=event.split_once(':'){
        desktop()?;if LOCKED.load(Ordering::Acquire)||SECURE.load(Ordering::Acquire){return Err(STATUS_ACCESS_DENIED);}
        let n=value.parse::<u8>().map_err(|_|STATUS_INVALID_PARAMETER)?;let mut config=system_sounds::configuration();
        if key=="startup" && n<=1 {
            if !admin(){return Err(STATUS_ACCESS_DENIED);}
            system_sounds::save_startup(n!=0)?;
            return Ok(Vec::new());
        }
        match key {"volume" if n<=100=>config.volume=n,"mute" if n<=1=>config.muted=n!=0,"scheme" if n<=1=>config.enabled=n!=0,_=>return Err(STATUS_INVALID_PARAMETER)}
        system_sounds::save_for(&user()?,config)?;return Ok(Vec::new());
    }
    Ok(system_sounds::play(event)?.to_le_bytes().to_vec())
}

/// Report physical adapters even before DHCP has supplied an IPv4 address.
/// Address configuration is not evidence of Internet reachability.
pub fn network(action:&str)->Result<Vec<u8>,u32>{
    desktop()?;
    if LOCKED.load(Ordering::Acquire)||SECURE.load(Ordering::Acquire){return Err(STATUS_ACCESS_DENIED);}
    if action=="flush-dns" {
        if !admin(){return Err(STATUS_ACCESS_DENIED);}
        crate::netstack::dns::flush_cache();
        return Ok(Vec::new());
    }
    if !action.is_empty(){return Err(STATUS_INVALID_PARAMETER);}
    let adapters=crate::netstack::ethif::get_all_interfaces();
    if adapters.len()>32{return Err(STATUS_BUFFER_TOO_SMALL);}
    let addresses=crate::netstack::ipif::get_all_interfaces();
    let mut bytes=b"NET1".to_vec();bytes.extend_from_slice(&(adapters.len()as u32).to_le_bytes());
    for adapter in adapters {
        let ip=addresses.iter().find(|ip|ip.eth_if==adapter.if_index);
        for value in [adapter.if_index,u32::from(adapter.link_up)|(u32::from(ip.is_some())<<1),adapter.mtu,
            ip.map_or(0,|ip|ip.address),ip.map_or(0,|ip|ip.netmask),ip.map_or(0,|ip|ip.gateway),crate::netstack::dns::get_dns_server()] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&adapter.mac);bytes.extend_from_slice(&adapter.name);
    }
    Ok(bytes)
}
