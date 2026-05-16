use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() as isize - 1
    }
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let tid = current_tid();
    if process_inner.deadlock_detect_enabled
        && process_inner
            .mutex_owner
            .get(mutex_id)
            .copied()
            .flatten()
            == Some(tid)
    {
        return -0xdead;
    }
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    if process_inner.mutex_owner.len() <= mutex_id {
        process_inner.mutex_owner.resize(mutex_id + 1, None);
    }
    drop(process_inner);
    drop(process);
    mutex.lock();
    current_process().inner_exclusive_access().mutex_owner[mutex_id] = Some(tid);
    0
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    if process_inner.mutex_owner.len() > mutex_id {
        process_inner.mutex_owner[mutex_id] = None;
    }
    drop(process_inner);
    drop(process);
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    ensure_deadlock_table(&mut process_inner, current_tid());
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        for row in process_inner.semaphore_alloc.iter_mut() {
            if row.len() <= id {
                row.resize(id + 1, 0);
            }
        }
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.semaphore_list.len() - 1
    };
    for row in process_inner.semaphore_alloc.iter_mut() {
        if row.len() <= id {
            row.resize(id + 1, 0);
        }
    }
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let tid = current_tid();
    ensure_deadlock_table(&mut process_inner, tid);
    if sem_id < process_inner.semaphore_alloc[tid].len()
        && process_inner.semaphore_alloc[tid][sem_id] > 0
    {
        process_inner.semaphore_alloc[tid][sem_id] -= 1;
    }
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    sem.up();
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let tid = current_tid();
    ensure_deadlock_table(&mut process_inner, tid);
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    {
        let mut sem_inner = sem.inner.exclusive_access();
        if sem_inner.count > 0 {
            sem_inner.count -= 1;
            process_inner.semaphore_alloc[tid][sem_id] += 1;
            return 0;
        }
    }
    if process_inner.deadlock_detect_enabled && will_deadlock(&process_inner, tid, sem_id) {
        return -0xdead;
    }
    process_inner.semaphore_request[tid] = Some(sem_id);
    drop(process_inner);
    sem.down();
    let mut process_inner = current_process().inner_exclusive_access();
    ensure_deadlock_table(&mut process_inner, tid);
    process_inner.semaphore_request[tid] = None;
    process_inner.semaphore_alloc[tid][sem_id] += 1;
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
///
/// YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    current_process().inner_exclusive_access().deadlock_detect_enabled = enabled != 0;
    0
}

fn current_tid() -> usize {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid
}

fn ensure_deadlock_table(inner: &mut crate::task::ProcessControlBlockInner, tid: usize) {
    let sem_count = inner.semaphore_list.len();
    while inner.semaphore_alloc.len() <= tid {
        inner.semaphore_alloc.push(alloc::vec![0; sem_count]);
    }
    for row in inner.semaphore_alloc.iter_mut() {
        if row.len() < sem_count {
            row.resize(sem_count, 0);
        }
    }
    while inner.semaphore_request.len() <= tid {
        inner.semaphore_request.push(None);
    }
}

fn will_deadlock(
    inner: &crate::task::ProcessControlBlockInner,
    tid: usize,
    sem_id: usize,
) -> bool {
    let sem_count = inner.semaphore_list.len();
    let task_count = inner.semaphore_alloc.len();
    let mut work = alloc::vec![0usize; sem_count];
    for (id, sem) in inner.semaphore_list.iter().enumerate() {
        if let Some(sem) = sem {
            let count = sem.inner.exclusive_access().count;
            if count > 0 {
                work[id] = count as usize;
            }
        }
    }
    let mut request = inner.semaphore_request.clone();
    if request.len() <= tid {
        request.resize(tid + 1, None);
    }
    request[tid] = Some(sem_id);
    let mut finish = alloc::vec![false; task_count.max(request.len())];
    loop {
        let mut progress = false;
        for i in 0..finish.len() {
            if finish[i] {
                continue;
            }
            let req = request.get(i).copied().flatten();
            if req.map_or(true, |r| work.get(r).copied().unwrap_or(0) > 0) {
                finish[i] = true;
                if let Some(row) = inner.semaphore_alloc.get(i) {
                    for (r, held) in row.iter().enumerate() {
                        work[r] += *held;
                    }
                }
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    tid < finish.len() && !finish[tid]
}
