//! Exercise the production SCM with only platform I/O replaced by host adapters.
#![allow(dead_code)]
extern crate alloc;
mod ke { pub mod sync {
    pub struct Spinlock<T>(std::sync::Mutex<T>);
    impl<T> Spinlock<T> {pub const fn new(v:T)->Self{Self(std::sync::Mutex::new(v))}pub fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()}}
}}
mod registry {pub mod cm {
    pub struct Value{pub data:Vec<u8>}
    pub static OVERLAY:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
    pub fn enumerate_subkeys(_: &str)->Option<Vec<String>>{
        OVERLAY.load(std::sync::atomic::Ordering::SeqCst).then(||vec!["overlay-test".into(), "configuration-only".into(), "missing-start".into()])
    }
    pub fn query_dword(path: &str,name:&str)->Option<u32>{
        if path.ends_with("\\configuration-only") || path.ends_with("\\missing-start") && name == "Start" { return None; }
        match name {"Type"=>Some(16),"Start"=>Some(3),"ErrorControl"=>Some(1),"DelayedAutoStart"=>Some(1),_=>None}
    }
    pub fn query_string(path: &str,name:&str)->Option<String>{
        match name {
            "ServiceDll" if path.ends_with("\\Parameters")=>Some("C:\\Windows\\System32\\test.dll".into()),
            "ObjectName"=>Some("NT AUTHORITY\\LocalService".into()),
            "ImagePath"=>Some("C:\\Windows\\System32\\svchost.exe -k test-hosts".into()),
            _=>None,
        }
    }
    pub fn query_value(_: &str,name:&str)->Option<Value>{
        (name=="DependOnGroup").then(||Value{data:"test-hosts\0\0".encode_utf16().flat_map(u16::to_le_bytes).collect()})
    }
}}

mod io {pub fn find_registered_driver(name: &[u8])->*mut u8{if name.starts_with(b"Tdx\0") { core::ptr::NonNull::<u8>::dangling().as_ptr() } else { core::ptr::null_mut() }}}
mod netstack {pub mod dns {
    pub static ENABLED:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
    pub fn set_cache_service_enabled(enabled:bool){ENABLED.store(enabled,std::sync::atomic::Ordering::SeqCst);}
}}
mod rtl {pub mod eventlog {pub fn ensure_initialized(){}}}
#[path="../../system/services/src/services.rs"] mod scm;
use scm::*;
fn entry(name:&str,deps:&[&str],start:ServiceStartType)->usize {
    let mut state=SCM_STATE.lock();let i=state.service_count;let mut svc=ServiceEntry::new();
    for (target,unit) in svc.name.iter_mut().zip(name.encode_utf16()){*target=unit;}
    svc.start_type=start;svc.dependency_count=deps.len();
    for (target,dep) in svc.dependencies.iter_mut().zip(deps) {for (cell,unit) in target.iter_mut().zip(dep.encode_utf16()){*cell=unit;}}
    state.services[i]=svc;state.service_count+=1;i
}
fn ready()->Result<BackendStarted,u32>{
    // A backend can safely query SCM while it starts: no lock held across callback.
    assert_eq!(query_service_status("base").unwrap().current_state,ServiceState::StartPending);
    Ok(BackendStarted{process_id:77,controls_accepted:3})
}
fn child_ready()->Result<BackendStarted,u32>{Ok(BackendStarted{process_id:78,controls_accepted:1})}
fn control(_:ServiceControl)->Result<(),u32>{assert!(query_service_status("base").is_some());Ok(())}
fn group_control(_:ServiceControl)->Result<(),u32>{assert!(query_service_status("group-host").is_some());Ok(())}
fn reject(_:u32)->u32{5}
fn reregister(_:u32)->u32{register_control_handler(reject);0}
#[test]
fn production_lifecycle_dependencies_callbacks_and_errors(){
    init();start();
    assert_eq!(query_service_status("EventLog").unwrap().current_state,ServiceState::Running);
    assert_eq!(stop_service_result("EventLog"),Err(1052));
    // The imported Windows dependencies must be honored, never bypassed to
    // make the DNS service look Running. Supply the test's NSI host explicitly.
    assert_eq!(query_service_status("Dnscache").unwrap().current_state,ServiceState::Stopped);
    assert_eq!(query_service_status("Dnscache").unwrap().win32_exit_code,1068);
    for name in ["nsi", "Tdx", "Tcpip", "NSIProxy", "Afd"] {
        assert!(register_backend(ServiceBackend{name,start:child_ready,control}));
    }
    assert!(start_service_result("Dnscache").is_ok());
    assert_eq!(service_snapshot().len(),145);
    assert!(query_service_info("W3SVC").is_none());
    assert_eq!(query_service_info("BITS").unwrap().start_type,ServiceStartType::AutoStart);
    assert_eq!(query_service_info("WSearch").unwrap().start_type,ServiceStartType::AutoStart);
    assert_eq!(query_service_info("WinDefend").unwrap().start_type,ServiceStartType::AutoStart);
    assert!(query_service_info("BITS").unwrap().delayed_auto_start);
    let dhcp=query_service_info("Dhcp").unwrap();
    assert!(dhcp.image_path.ends_with("-k LocalServiceNetworkRestricted"));
    let crypto=query_service_info("CryptSvc").unwrap();
    assert!(crypto.image_path.ends_with("-k NetworkService"));
    assert!(crypto.service_dll.ends_with("cryptsvc.dll"));
    assert_eq!(query_service_info("UI0Detect").unwrap().service_type as u32, 0x110);
    assert_eq!(query_service_info("ALG").unwrap().display_name,"Application Layer Gateway Service");

    assert!(netstack::dns::ENABLED.load(std::sync::atomic::Ordering::SeqCst));
    assert!(stop_service_result("Dnscache").is_ok());
    assert!(!netstack::dns::ENABLED.load(std::sync::atomic::Ordering::SeqCst));
    assert!(start_service_result("Dnscache").is_ok());
    assert_eq!(start_service_result("missing"),Err(1060));
    entry("disabled",&[],ServiceStartType::Disabled);
    assert_eq!(start_service_result("disabled"),Err(1058));
    entry("unhosted",&[],ServiceStartType::DemandStart);
    assert_eq!(start_service_result("unhosted"),Err(120));
    let failure=query_service_status("unhosted").unwrap();assert_eq!(failure.current_state,ServiceState::Stopped);assert_eq!(failure.win32_exit_code,120);assert_eq!(failure.wait_hint,0);
    entry("missing-dep",&["absent"],ServiceStartType::DemandStart);
    assert_eq!(start_service_result("missing-dep"),Err(1075));
    entry("cycle-a",&["cycle-b"],ServiceStartType::DemandStart);entry("cycle-b",&["cycle-a"],ServiceStartType::DemandStart);
    assert_eq!(start_service_result("cycle-a"),Err(1059));assert_eq!(query_service_status("cycle-b").unwrap().current_state,ServiceState::Stopped);
    let group_host=entry("group-host",&[],ServiceStartType::DemandStart);
    let group_client=entry("group-client",&[],ServiceStartType::DemandStart);
    {
        let mut state=SCM_STATE.lock();
        for (cell,u) in state.services[group_host].load_order_group.iter_mut().zip("test-hosts".encode_utf16()){*cell=u;}
        state.services[group_client].dependency_group_count=1;
        for (cell,u) in state.services[group_client].dependency_groups[0].iter_mut().zip("test-hosts".encode_utf16()){*cell=u;}
    }
    assert!(!check_dependencies("group-client"));
    assert_eq!(start_service_result("group-client"),Err(1068));
    assert!(register_backend(ServiceBackend{name:"group-host",start:child_ready,control:group_control}));
    assert!(register_backend(ServiceBackend{name:"group-client",start:child_ready,control:group_control}));
    assert_eq!(start_service_result("group-client"),Ok(()));
    assert_eq!(query_service_status("group-host").unwrap().current_state,ServiceState::Running);
    assert!(check_dependencies("group-client"));
    assert_eq!(stop_service_result("group-host"),Err(1051));
    assert_eq!(stop_service_result("group-client"),Ok(()));
    assert_eq!(stop_service_result("group-host"),Ok(()));
    assert!(!check_dependencies("group-client"));
    let base=entry("base",&[],ServiceStartType::DemandStart);entry("child",&["base"],ServiceStartType::DemandStart);
    assert!(register_backend(ServiceBackend{name:"base",start:ready,control}));
    assert!(register_backend(ServiceBackend{name:"child",start:child_ready,control}));
    assert_eq!(start_service_result("child"),Ok(()));assert_eq!(query_service_info("base").unwrap().process_id,77);
    assert_eq!(stop_service_result("base"),Err(1051));
    assert_eq!(start_service_result("base"),Err(1056));
    assert!(control_service(base,ServiceControl::Pause));assert_eq!(query_service_status("base").unwrap().current_state,ServiceState::Paused);
    assert!(control_service(base,ServiceControl::Continue));
    assert_eq!(stop_service_result("child"),Ok(()));assert_eq!(stop_service_result("base"),Ok(()));assert_eq!(query_service_info("base").unwrap().process_id,0);
    let idx=entry("handler-failure",&[],ServiceStartType::DemandStart);
    let handler=register_service_ctrl_handler(&"handler-failure".encode_utf16().collect::<Vec<_>>(),reject).unwrap();
    let mut status=ServiceStatus::new();status.current_state=ServiceState::Running;status.controls_accepted=1;
    assert!(set_service_status(handler,status));assert_eq!(query_service_status("handler-failure").unwrap().current_state,ServiceState::Running);
    assert_eq!(stop_service_result("handler-failure"),Err(5));assert_eq!(query_service_status("handler-failure").unwrap().current_state,ServiceState::Running);
    assert!(!control_service(idx,ServiceControl::Pause));assert!(!control_service(999,ServiceControl::Interrogate));
    registry::cm::OVERLAY.store(true,std::sync::atomic::Ordering::SeqCst);
    start();
    assert!(query_service_info("configuration-only").is_none(), "registry configuration keys must not become invented services");
    assert!(query_service_info("missing-start").is_none(), "a service without startup policy is not a complete definition");
    let overlay=query_service_info("overlay-test").unwrap();
    assert_eq!(overlay.dependency_groups,["test-hosts"]);
    assert_eq!(overlay.account,"NT AUTHORITY\\LocalService");
    assert!(overlay.delayed_auto_start);
    assert_eq!(overlay.service_dll,"C:\\Windows\\System32\\test.dll");
    assert!(overlay.load_order_group.is_empty(),"svchost group is not the load-order group");
    registry::cm::OVERLAY.store(false,std::sync::atomic::Ordering::SeqCst);
    register_control_handler(reregister);
    assert_eq!(dispatch_control(4),0);
    assert_eq!(dispatch_control(4),5);
}
