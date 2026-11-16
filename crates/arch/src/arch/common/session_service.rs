//! Authorized session and Task Manager service boundary.
use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use nt61_abi::desktop_bridge::*;
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
    desktop()?;
    match action {
        "user"=>Ok(user()?.into_bytes()),
        "lock"|"logoff"=>{LOCKED.store(true,Ordering::Release);Ok(Vec::new())},
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
            process.set_token(token);LOCKED.store(false,Ordering::Release);Ok(Vec::new())
        },
        "password"=>{
            let split=input.iter().position(|b|*b==0).ok_or(STATUS_INVALID_PARAMETER)?;
            if crate::se::users::change_interactive_password(&user()?,&input[..split],&input[split+1..]) {Ok(Vec::new())}else{Err(STATUS_ACCESS_DENIED)}
        },
        "sleep"=>Err(STATUS_NOT_SUPPORTED), // Never call the unimplemented S3 save/restore path.
        "shutdown"|"restart"=>{
            if LOCKED.load(Ordering::Acquire)||!admin(){return Err(STATUS_ACCESS_DENIED);}
            crate::hal::serial::write_string_always(if action=="restart"{"[SESSION] restarting\r\n"}else{"[SESSION] shutting down\r\n"});
            let _=crate::registry::flush::flush_sam_hive();
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
    let mut bytes=Vec::new();
    for (name,state,pid) in crate::servers::services::query_all_services(){
        if name.len()>MAX_PATH||bytes.len()+12+name.len()>MAX_TRANSFER{return Err(STATUS_BUFFER_TOO_SMALL);}
        bytes.extend_from_slice(&(name.len()as u32).to_le_bytes());bytes.extend_from_slice(&(state as u32).to_le_bytes());bytes.extend_from_slice(&pid.to_le_bytes());bytes.extend_from_slice(name.as_bytes());
    }
    Ok(bytes)
}
pub fn service_control(name:&str,input:&[u8])->Result<Vec<u8>,u32>{
    desktop()?;if LOCKED.load(Ordering::Acquire)||!admin(){return Err(STATUS_ACCESS_DENIED);}
    let success=match input {b"start"=>crate::servers::services::start_service(name),b"stop"=>crate::servers::services::stop_service(name),_=>return Err(STATUS_INVALID_PARAMETER)};
    if success{Ok(Vec::new())}else{Err(STATUS_ACCESS_DENIED)}
}
pub fn task_control(pid:&str,input:&[u8])->Result<Vec<u8>,u32>{
    desktop()?;if LOCKED.load(Ordering::Acquire){return Err(STATUS_ACCESS_DENIED);}
    let pid=pid.parse::<u64>().map_err(|_|STATUS_INVALID_PARAMETER)?;
    if input!=b"end"{return Err(STATUS_NOT_SUPPORTED);}
    let caller=user()?;
    let administrator=admin();
    let mut status=STATUS_NOT_FOUND;
    crate::ps::process::iterate_processes(|id,p|{
        if id!=pid{return true;}
        // SAFETY: iterate_processes retains the registry lock and allocations.
        let process=unsafe{&mut *p};
        let token=unsafe{process.get_token().as_ref()};
        let owner=token.and_then(|t|crate::se::users::account_name(&t.user));
        if process.session_id==0 || matches!(pid,0..=1024) || pid==crate::servers::smss::PID_EXPLORER || pid==crate::servers::smss::PID_LOGONUI || (!administrator && owner.as_deref()!=Some(caller.as_str())) {status=STATUS_ACCESS_DENIED;return false;}
        process.exit_status=1;process.exit_time=crate::hal::pit::get_system_time_ms().max(1);
        // Request dispatcher-owned termination, retaining storage until no CPU
        // can run a member. Never free a foreign CPU's active stack here.
        unsafe{crate::ps::process::request_process_members_termination(p);}
        status=STATUS_SUCCESS;false
    });
    if status==STATUS_SUCCESS{Ok(Vec::new())}else{Err(status)}
}
