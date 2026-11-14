// =============================================================================
// Process Management API for Commands Module
// =============================================================================

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub memory_usage: u64,
    pub cpu_time: u64,
    pub session_id: u32,
    pub status: String,
    pub user_name: String,
    pub window_title: String,
    pub services: Vec<String>,
}

pub fn get_process_list() -> Vec<ProcessInfo> {
    use crate::ps::process_mgmt::get_all_processes;

    let mut processes = Vec::new();

    match get_all_processes() {
        Ok(kernel_processes) => {
            for kproc in kernel_processes {
                processes.push(ProcessInfo {
                    pid: kproc.pid as u32,
                    name: kproc.name,
                    memory_usage: kproc.memory_usage,
                    cpu_time: kproc.cpu_time,
                    session_id: kproc.session_id,
                    status: kproc.status,
                    user_name: kproc.user_name,
                    window_title: kproc.window_title,
                    services: kproc.services,
                });
            }
        }
        Err(_) => {}
    }

    processes
}

pub fn find_processes_by_name(name: &str) -> Vec<u32> {
    let processes = get_process_list();
    let name_lower = name.to_lowercase();

    processes
        .iter()
        .filter(|p| p.name.to_lowercase() == name_lower)
        .map(|p| p.pid)
        .collect()
}

pub fn terminate_process(pid: u32) -> bool {
    use crate::ps::process_mgmt::terminate_process_by_pid;

    // NT blocks termination of the idle and system processes, not
    // a magic PID cutoff. The process manager checks per-process
    // critical flags (EPROCESS::BreakOnTermination) internally.
    if pid == 0 || pid == 4 {
        return false;
    }

    terminate_process_by_pid(pid as u64, false).is_ok()
}

pub fn kill_process_forced(pid: u32) -> bool {
    use crate::ps::process_mgmt::terminate_process_by_pid;

    if pid == 0 || pid == 4 {
        return false;
    }

    terminate_process_by_pid(pid as u64, true).is_ok()
}

pub fn create_process(
    command: &str,
    work_dir: Option<&str>,
    priority: u8,
    _minimized: bool,
    _maximized: bool,
) -> Option<u32> {
    use crate::ps::process_mgmt::create_user_process;

    match create_user_process(command, work_dir, priority) {
        Ok(pid) => Some(pid as u32),
        Err(_) => None,
    }
}

pub fn wait_for_process(pid: u32) {
    use crate::ps::process_mgmt::wait_for_process_exit;

    let _ = wait_for_process_exit(pid as u64, None);
}

pub fn get_child_processes(parent_pid: u32) -> Vec<u32> {
    // The process table does not expose parent_pid through the public API,
    // so add a public enumeration helper to ProcessTable.
    crate::ps::process_mgmt::get_child_pids(parent_pid as u64)
        .into_iter()
        .map(|pid| pid as u32)
        .collect()
}
