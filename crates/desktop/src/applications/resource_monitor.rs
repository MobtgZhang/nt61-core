//! Process counters from the kernel; unavailable I/O and CPU-rate counters are
//! never replaced with simulated activity or percentages.
use alloc::vec::Vec;
use super::processes::{Process, Snapshot};

pub const PAGE_SIZE: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Monitor {
    pub page: usize,
    pub offset: usize,
}

impl Monitor {
    pub fn select_page(&mut self, page: usize) {
        self.page = page.min(2);
        self.offset = 0;
    }

    pub fn scroll(&mut self, direction: i32, count: usize) {
        let last = count.saturating_sub(1) / PAGE_SIZE * PAGE_SIZE;
        self.offset = if direction < 0 {
            self.offset.saturating_sub(PAGE_SIZE)
        } else {
            self.offset.saturating_add(PAGE_SIZE).min(last)
        };
    }

    /// Clamp an old page when processes exit between two observations.
    pub fn visible_offset(&self, count: usize) -> usize {
        self.offset.min(count.saturating_sub(1) / PAGE_SIZE * PAGE_SIZE)
    }

    pub fn sorted<'a>(&self, snapshot: &'a Snapshot) -> Vec<&'a Process> {
        let mut rows: Vec<_> = snapshot.processes.iter().collect();
        rows.sort_by(|a,b| {
            let counter = if self.page == 1 {
                b.cpu_ticks.cmp(&a.cpu_ticks)
            } else {
                b.working_set.cmp(&a.working_set)
            };
            counter.then(a.pid.cmp(&b.pid))
        });
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn process(pid:u64, working_set:u64, cpu_ticks:u64)->Process {
        Process {pid,working_set,cpu_ticks,session:1,name:"example.exe".into(),user:"Administrator".into()}
    }
    #[test]
    fn pages_sort_actual_counters_and_clamp_after_processes_exit() {
        let snapshot=Snapshot {cpu_count:4,processes:alloc::vec![
            process(8,1024,700),process(4,8192,50),process(5,8192,80)]};
        let mut monitor=Monitor::default();
        assert_eq!(monitor.sorted(&snapshot).iter().map(|p|p.pid).collect::<Vec<_>>(),[4,5,8]);
        monitor.select_page(1);
        assert_eq!(monitor.sorted(&snapshot).iter().map(|p|p.pid).collect::<Vec<_>>(),[8,5,4]);
        monitor.scroll(1,19);monitor.scroll(1,19);assert_eq!(monitor.offset,16);
        assert_eq!(monitor.visible_offset(3),0);
        monitor.scroll(1,0);assert_eq!(monitor.offset,0);
        monitor.select_page(2);assert_eq!(monitor.offset,0);
        assert_eq!(monitor.visible_offset(0),0);
    }
}
