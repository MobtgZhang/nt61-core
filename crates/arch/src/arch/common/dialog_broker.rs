//! Copied desktop requests. Never hold the queue lock across a scheduler yield.
use alloc::{boxed::Box, string::String, vec::Vec};
use nt61_abi::{desktop_bridge::*, dialog_bridge as wire};
use crate::ke::sync::Spinlock;

struct Pending { id:u64, packet:Box<wire::Packet>, dispatched:bool, done:bool }
struct Broker { next:u64, pending:Vec<Pending>, heartbeat:u64 }
static BROKER:Spinlock<Broker>=Spinlock::new(Broker{next:1,pending:Vec::new(),heartbeat:0});
fn clock()->u64 { crate::ke::time::get_interrupt_time() }
fn desktop_caller()->bool {
    let thread=crate::ps::thread::get_current_ethread();
    if thread.is_null(){return false;}
    // SAFETY: the executing thread owns a live process for the duration of this call.
    unsafe { !(*thread).threads_process.is_null() && (*(*thread).threads_process).unique_process_id==crate::servers::smss::PID_EXPLORER }
}
pub fn bytes(packet:&wire::Packet)->&[u8] {
    // SAFETY: Packet is repr(C), has no padding, references or uninitialized fields.
    unsafe {core::slice::from_raw_parts(core::ptr::from_ref(packet).cast(),wire::PACKET_SIZE)}
}
pub fn decode(data: &[u8]) -> Result<Box<wire::Packet>, u32> {
    if data.len() != wire::PACKET_SIZE {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let mut packet = Box::<wire::Packet>::new_uninit();
    unsafe {
        core::ptr::copy_nonoverlapping(data.as_ptr(), packet.as_mut_ptr().cast(), data.len());
        Ok(packet.assume_init())
    }
}
pub fn request(packet: Box<wire::Packet>) -> Result<Box<wire::Packet>, u32> {
    let id={let mut broker=BROKER.lock();
        if clock().saturating_sub(broker.heartbeat)>50_000_000 || broker.heartbeat==0{return Err(STATUS_NOT_SUPPORTED);}
        if broker.pending.len()>=32{return Err(0xc000009a);}
        let id=broker.next;broker.next=broker.next.checked_add(1).ok_or(0xc000009au32)?;
        broker.pending.push(Pending{id,packet,dispatched:false,done:false});id};
    loop {
        {let mut broker=BROKER.lock();
            let index=broker.pending.iter().position(|p|p.id==id).ok_or(STATUS_NOT_FOUND)?;
            if broker.pending[index].done{return Ok(broker.pending.remove(index).packet);}
            if clock().saturating_sub(broker.heartbeat)>100_000_000 {
                broker.pending.remove(index);return Err(STATUS_NOT_SUPPORTED);
            }
        }
        crate::ke::scheduler::yield_();
    }
}
pub fn poll()->Result<Vec<u8>,u32>{
    if !desktop_caller(){return Err(STATUS_ACCESS_DENIED);}
    let mut broker=BROKER.lock();broker.heartbeat=clock();
    let Some(p)=broker.pending.iter_mut().find(|p|!p.dispatched)else{return Ok(Vec::new());};
    p.dispatched=true;
    let mut output=p.id.to_le_bytes().to_vec();output.extend_from_slice(bytes(&p.packet));Ok(output)
}
pub fn complete(id: u64, packet: Box<wire::Packet>) -> Result<(), u32> {
    if !desktop_caller(){return Err(STATUS_ACCESS_DENIED);}
    if !matches!(packet.values[1],wire::ACCEPTED|wire::CANCELLED|wire::FAILED){return Err(STATUS_INVALID_PARAMETER);}
    let mut broker=BROKER.lock();
    let p=broker.pending.iter_mut().find(|p|p.id==id&&p.dispatched&&!p.done).ok_or(STATUS_NOT_FOUND)?;
    if p.packet.values[0]!=packet.values[0]{return Err(STATUS_INVALID_PARAMETER);}
    p.packet=packet;p.done=true;Ok(())
}

struct Command { id:u64, line:String, directory:Vec<u8>, identity:crate::ps::impersonation::ImpersonationReference, result:Option<Result<Vec<u8>,u32>>, exit:bool }
static COMMANDS:Spinlock<Vec<Command>>=Spinlock::new(Vec::new());
static NEXT:core::sync::atomic::AtomicU64=core::sync::atomic::AtomicU64::new(1);
pub fn command_start(line:&str,directory:&[u8])->Result<u64,u32>{
    if !desktop_caller(){return Err(STATUS_ACCESS_DENIED);}
    let identity=crate::ps::process::get_current_thread_token().snapshot_for_worker()
        .map_err(|_|STATUS_ACCESS_DENIED)?;
    let id=NEXT.fetch_add(1,core::sync::atomic::Ordering::Relaxed);
    {let mut commands=COMMANDS.lock();if commands.len()>=16{return Err(0xc000009a);}
    commands.push(Command{id,line:line.into(),directory:directory.to_vec(),identity,result:None,exit:false});}
    let context=Box::into_raw(Box::new(id));
    let created=crate::ps::thread::ps_create_system_thread(command_worker as *const () as u64,context as u64);
    if !created.success {
        // SAFETY: no worker was started; the unique Box allocation is still ours.
        unsafe{drop(Box::from_raw(context));}
        COMMANDS.lock().retain(|c|c.id!=id);return Err(0xc000009a);
    }
    Ok(id)
}
extern "C" fn command_worker(context:u64){
    // SAFETY: context is the unique Box<u64> transferred by command_start.
    let id=unsafe{*Box::from_raw(context as *mut u64)};
    let input=COMMANDS.lock().iter().find(|c|c.id==id).map(|c|(c.line.clone(),c.directory.clone(),c.identity.clone()));
    if let Some((line,directory,identity))=input {
        let result=crate::ps::impersonation::with_current_thread_token(identity, ||
                crate::servers::cmd::execute_captured_command(&line,&directory)
                    .map_err(|_|STATUS_ACCESS_DENIED))
            .map_err(|_|STATUS_ACCESS_DENIED).and_then(|result|result);
        if let Some(command)=COMMANDS.lock().iter_mut().find(|c|c.id==id){
            command.result=Some(result.map(|r|{command.exit=r.exit;
                let mut bytes=alloc::vec![0;CWD_BYTES];let n=r.directory.len().min(CWD_BYTES-1);
                bytes[..n].copy_from_slice(&r.directory[..n]);bytes.extend_from_slice(&r.output);bytes}));
        }
    }
    crate::ps::thread::ps_exit_system_thread(0)
}
pub fn command_poll(id:u64)->Result<(Vec<u8>,bool),u32>{
    if !desktop_caller() {
        return Err(STATUS_ACCESS_DENIED);
    }
    let command = {
        let mut commands = COMMANDS.lock();
        let index = commands.iter().position(|command| command.id == id).ok_or(STATUS_NOT_FOUND)?;
        commands[index].result.is_some().then(|| commands.remove(index))
    };
    if let Some(command) = command {
        return Ok((command.result.ok_or(STATUS_NOT_FOUND)??, command.exit));
    }
    crate::ke::scheduler::yield_();
    Ok((Vec::new(), false))
}

/// Publish a complete PostScript job in the system-volume RAM mirror before
/// acknowledging EndDoc. A unique create never overwrites another document.
/// This does not flush the mirror to the backing disk or survive a reboot.
pub fn spool(data:&[u8])->Result<u32,u32>{
    static NEXT_JOB:core::sync::atomic::AtomicU32=core::sync::atomic::AtomicU32::new(1);
    if data.len()>1024*1024 || !data.starts_with(b"%!PS-Adobe-3.0\n") || !data.ends_with(b"%%EOF\n") {return Err(STATUS_INVALID_PARAMETER);}
    for _ in 0..100 {
        let id=NEXT_JOB.fetch_add(1,core::sync::atomic::Ordering::Relaxed);
        let path=alloc::format!("C:\\Users\\Administrator\\Documents\\Print-{id:08}.ps");
        match crate::fs::system_volume::create(&path,data){Ok(())=>return Ok(id),Err(0xc0000035)=>continue,Err(status)=>return Err(status)}
    }Err(0xc000009a)
}
