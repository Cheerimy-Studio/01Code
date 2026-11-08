//! Lifetime ownership for a runtime process and its descendants.
use std::process::{Child, Command, ExitStatus};

pub struct ProcessTree {
    pub child: Child,
    #[cfg(windows)]
    job: std::os::windows::io::OwnedHandle,
    finished: bool,
}
impl ProcessTree {
    pub fn spawn(command: &mut Command) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn()?;
        #[cfg(windows)]
        let job = match job_for(&child) {
            Ok(job) => job,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        #[cfg(not(windows))]
        let _ = &mut child;
        Ok(Self {
            child,
            #[cfg(windows)]
            job,
            finished: false,
        })
    }
    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }
    pub fn terminate(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            unsafe {
                windows_sys::Win32::System::JobObjects::TerminateJobObject(
                    self.job.as_raw_handle(),
                    1,
                );
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for ProcessTree {
    fn drop(&mut self) {
        self.terminate();
    }
}
#[cfg(windows)]
fn job_for(child: &Child) -> std::io::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::JobObjects::*;
    unsafe {
        let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if raw.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = OwnedHandle::from_raw_handle(raw);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            raw,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        ) == 0
            || AssignProcessToJobObject(raw, child.as_raw_handle()) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(job)
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn dropping_owner_terminates_process() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 30"]);
        let mut tree = ProcessTree::spawn(&mut cmd).unwrap();
        tree.terminate();
        assert!(tree.try_wait().unwrap().is_some());
    }
}
