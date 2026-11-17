//! Poll-driven Intel HDA PCM output (48 kHz, stereo, signed 16 bit).
//! Uses immediate codec verbs and an eight-page cyclic DMA BDL. No reported
//! completion is generated without programming a real stream descriptor.
use alloc::{collections::VecDeque,vec::Vec};
use crate::{hal::common::pci,ke::sync::Spinlock,mm::pool::{self,PoolType}};
const BLOCK:usize=4096;
const BLOCKS:usize=8;
struct Output {mmio:u64,stream:u64,bdl:*mut u8,buffers:[*mut u8;BLOCKS],queue:VecDeque<Vec<i16>>,offset:usize,last:usize,running:bool,volume:u8,muted:bool}
// Hardware pages have one owner; all software access is serialized by OUTPUT.
unsafe impl Send for Output {}
static OUTPUT:Spinlock<Option<Output>>=Spinlock::new(None);
unsafe fn r32(b:u64,o:u64)->u32{unsafe{core::ptr::read_volatile((b+o)as *const u32)}}
unsafe fn w32(b:u64,o:u64,v:u32){unsafe{core::ptr::write_volatile((b+o)as *mut u32,v)}}
unsafe fn r16(b:u64,o:u64)->u16{unsafe{core::ptr::read_volatile((b+o)as *const u16)}}
unsafe fn w16(b:u64,o:u64,v:u16){unsafe{core::ptr::write_volatile((b+o)as *mut u16,v)}}
fn wait(mut condition:impl FnMut()->bool)->bool{for _ in 0..200_000 {if condition(){return true;}core::hint::spin_loop();}false}
fn delay(ms:u64){let start=crate::hal::pit::get_system_time_ms();for _ in 0..2_000_000 {if crate::hal::pit::get_system_time_ms().saturating_sub(start)>=ms{break;}core::hint::spin_loop();}}
fn verb(mmio:u64,codec:u8,nid:u8,payload:u32)->Option<u32>{unsafe{
    if !wait(||r16(mmio,0x68)&1==0){return None;}
    w16(mmio,0x68,2);w32(mmio,0x60,((codec as u32)<<28)|((nid as u32)<<20)|(payload&0xfffff));w16(mmio,0x68,1);
    if !wait(||r16(mmio,0x68)&3==2){return None;}let value=r32(mmio,0x64);w16(mmio,0x68,2);Some(value)
}}
fn get(mmio:u64,c:u8,n:u8,p:u8)->Option<u32>{verb(mmio,c,n,0xf0000|u32::from(p))}
fn route(mmio:u64,codec:u8)->bool{
    let Some(root)=get(mmio,codec,0,4)else{return false;};
    let (start,count)=(((root>>16)&255)as u8,(root&255)as u8);
    for fg in start..start.saturating_add(count) {
        if get(mmio,codec,fg,5).unwrap_or(0)&255!=1{continue;}
        let Some(nodes)=get(mmio,codec,fg,4)else{continue;};
        let _=verb(mmio,codec,fg,0x70500);delay(1);
        let first=((nodes>>16)&255)as u8;let last=first.saturating_add((nodes&255)as u8);
        // Follow each output pin's actual connection list to its DAC. Support
        // direct DAC and selector/mixer routes without assuming codec node IDs.
        for pin in first..last {
            let caps=get(mmio,codec,pin,9).unwrap_or(0);
            if caps>>20&15!=4||get(mmio,codec,pin,12).unwrap_or(0)&16==0{continue;}
            let config=verb(mmio,codec,pin,0xf1c00).unwrap_or(u32::MAX);
            if config>>30==1{continue;}
            let mut path=Vec::new();
            if !find_dac(mmio,codec,pin,&mut path,0){continue;}
            let Some(&(dac,_))=path.last()else{continue;};
            for &(node,connection) in &path {
                let _=verb(mmio,codec,node,0x70500);
                let _=verb(mmio,codec,node,0x70100|u32::from(connection));
                let wc=get(mmio,codec,node,9).unwrap_or(0);
                if wc&4!=0 {let amp=get(mmio,codec,node,18).unwrap_or(0);let gain=(amp&127).min((amp>>8)&127);let _=verb(mmio,codec,node,0x30000|0xb000|gain);}
                if wc&2!=0 {let amp=get(mmio,codec,node,13).unwrap_or(0);let gain=(amp&127).min((amp>>8)&127);let _=verb(mmio,codec,node,0x30000|0x7000|(u32::from(connection)<<8)|gain);}
            }
            let _=verb(mmio,codec,pin,0x70740); // output enable
            let _=verb(mmio,codec,pin,0x70c02); // EAPD
            if verb(mmio,codec,dac,0x20011).is_some()&&verb(mmio,codec,dac,0x70610).is_some(){return true;}
        }
    }false
}
fn find_dac(mmio:u64,c:u8,node:u8,path:&mut Vec<(u8,u8)>,depth:usize)->bool {
    if depth>8||path.iter().any(|&(n,_)|n==node){return false;}
    let caps=get(mmio,c,node,9).unwrap_or(u32::MAX);
    if caps>>20&15==0 {path.push((node,0));return true;}
    let list=get(mmio,c,node,14).unwrap_or(0);let count=(list&127)as usize;let long=list&128!=0;
    if count>64{return false;}
    let per=if long{2}else{4};let mut previous=0u8;let mut selector=0u8;
    for i in 0..count {
        let response=verb(mmio,c,node,0xf0200|((i/per*per)as u32)).unwrap_or(0);
        let raw=if long{(response>>((i%per)*16))&0xffff}else{(response>>((i%per)*8))&255};
        let range=raw&if long{0x8000}else{0x80}!=0;let next=(raw&if long{0x7fff}else{0x7f})as u8;
        let begin=if range{previous.saturating_add(1)}else{next};
        for child in begin..=next {
            path.push((node,selector));if find_dac(mmio,c,child,path,depth+1){return true;}path.pop();selector=selector.saturating_add(1);
        }previous=next;
    }false
}
pub fn init()->bool {
    for dev in pci::enumerate(){if (dev.class_code,dev.subclass)!=(4,3){continue;}
        let bar=pci::read_config_dword(dev.bus,dev.device,dev.function,0x10);if bar&1!=0{continue;}
        let mut physical=u64::from(bar&!15);if bar&6==4{physical|=u64::from(pci::read_config_dword(dev.bus,dev.device,dev.function,0x14))<<32;}
        let Some(mmio)=crate::mm::syspte::map_io_space(physical,4)else{continue;};
        let cmd=pci::read_config_word(dev.bus,dev.device,dev.function,4);pci::write_config_word(dev.bus,dev.device,dev.function,4,cmd|6);
        unsafe{
            // Disable DMA engines before resetting the controller.
            core::ptr::write_volatile((mmio+0x4c)as *mut u8,0);core::ptr::write_volatile((mmio+0x5c)as *mut u8,0);
            w32(mmio,8,0);if !wait(||r32(mmio,8)&1==0){continue;}delay(1);w32(mmio,8,1);if !wait(||r32(mmio,8)&1!=0){continue;}delay(2);
            let gcap=r16(mmio,0);if gcap>>12&15==0{continue;}
            let codecs=r16(mmio,0xe);let mut routed=false;
            for c in 0..15 {if codecs&(1<<c)!=0&&route(mmio,c){routed=true;break;}}
            if !routed{crate::hal::serial::write_string_always("[AUDIO] HDA no supported output route\r\n");continue;}
            let stream=mmio+0x80+u64::from((gcap>>8)&15)*0x20;
            w32(stream,0,1);if !wait(||r32(stream,0)&1!=0){continue;}w32(stream,0,0);if !wait(||r32(stream,0)&1==0){continue;}
            let bdl=pool::allocate_aligned(PoolType::NonPaged,128,128);if bdl.is_null(){continue;}
            let Some(bdl_phys)=crate::mm::virt_to_phys(bdl as u64)else{continue;};
            let mut buffers=[core::ptr::null_mut();BLOCKS];let mut valid=true;
            for (i,slot)in buffers.iter_mut().enumerate(){*slot=pool::allocate_aligned(PoolType::NonPaged,BLOCK,BLOCK);if slot.is_null(){valid=false;break;}
                core::ptr::write_bytes(*slot,0,BLOCK);let Some(physical)=crate::mm::virt_to_phys(*slot as u64)else{valid=false;break;};
                core::ptr::write_volatile(bdl.add(i*16)as *mut u64,physical);core::ptr::write_volatile(bdl.add(i*16+8)as *mut u32,BLOCK as u32);core::ptr::write_volatile(bdl.add(i*16+12)as *mut u32,0);
            }
            if !valid{continue;}
            w32(stream,0x18,bdl_phys as u32);w32(stream,0x1c,(bdl_phys>>32)as u32);w32(stream,8,(BLOCK*BLOCKS)as u32);w16(stream,0xc,(BLOCKS-1)as u16);w16(stream,0x12,0x11);w32(stream,0,1<<20);
            *OUTPUT.lock()=Some(Output{mmio,stream,bdl,buffers,queue:VecDeque::new(),offset:0,last:0,running:false,volume:75,muted:false});
            crate::hal::serial::write_string_always("[AUDIO] HDA PCM DMA ready: 48000 Hz stereo 16-bit\r\n");return true;
        }
    }false
}
impl Output {
    fn fill(&mut self,index:usize){unsafe{
        let target=self.buffers[index]as *mut i16;core::ptr::write_bytes(target,0,BLOCK/2);
        for i in 0..BLOCK/2 {
            let Some(samples)=self.queue.front()else{break;};
            let v=if self.muted{0}else{(i32::from(samples[self.offset])*i32::from(self.volume.min(100))/100)as i16};core::ptr::write_volatile(target.add(i),v);self.offset+=1;
            if self.offset==samples.len(){self.queue.pop_front();self.offset=0;}
        }
    }}
}
pub fn ready()->bool{OUTPUT.lock().is_some()}
pub fn submit(samples:&[i16])->usize{
    if samples.is_empty()||samples.len()%2!=0{return 0;}
    let mut state=OUTPUT.lock();let Some(output)=state.as_mut()else{return 0;};
    let pending:usize=output.queue.iter().map(Vec::len).sum();if pending+samples.len()>48000*2*10{return 0;}
    output.queue.push_back(samples.to_vec());
    if !output.running {for i in 0..BLOCKS{output.fill(i);}unsafe{w32(output.stream,0,(1<<20)|2);}output.running=true;}
    samples.len()
}
pub fn poll(){
    let Some(mut state)=OUTPUT.try_lock()else{return;};let Some(output)=state.as_mut()else{return;};if !output.running{return;}
    let current=unsafe{r32(output.stream,4)as usize/BLOCK};if current>=BLOCKS{return;}
    while output.last!=current {let index=output.last;output.fill(index);output.last=(index+1)%BLOCKS;}
}
pub fn set_volume(volume:u8,muted:bool){if let Some(output)=OUTPUT.lock().as_mut(){output.volume=volume.min(100);output.muted=muted;}}
pub fn volume(volume:u8){if let Some(o)=OUTPUT.lock().as_mut(){o.volume=volume.min(100);}}
pub fn mute(muted:bool){if let Some(o)=OUTPUT.lock().as_mut(){o.muted=muted;}}
pub fn stop(){if let Some(output)=OUTPUT.lock().as_mut(){unsafe{
    w32(output.stream,0,1<<20);
    if wait(||r32(output.stream,0)&2==0){w32(output.stream,0,1);let _=wait(||r32(output.stream,0)&1!=0);w32(output.stream,0,0);let _=wait(||r32(output.stream,0)&1==0);w32(output.stream,0,1<<20);}
}output.running=false;output.queue.clear();output.offset=0;output.last=0;}}
