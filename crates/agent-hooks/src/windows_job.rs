//! Windows Job Object containment for command hooks.

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use tokio::process::{Child, Command};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtResumeProcess(process_handle: HANDLE) -> i32;
}

/// Owns a Job Object that kills every contained process when dropped.
#[derive(Debug)]
pub(crate) struct WindowsJobObject {
    handle: OwnedHandle,
}

impl WindowsJobObject {
    pub(crate) fn create() -> io::Result<Self> {
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self {
            handle: unsafe { OwnedHandle::from_raw_handle(handle.cast()) },
        };
        job.set_limit_flags(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK)?;
        Ok(job)
    }

    /// Starts the process suspended so no descendant can escape before assignment.
    pub(crate) fn spawn_contained(&self, command: &mut Command) -> io::Result<Child> {
        command.creation_flags(CREATE_SUSPENDED).kill_on_drop(true);
        let child = command.spawn()?;
        let process_handle = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("missing child process handle"))?
            .cast();

        let assigned = unsafe { AssignProcessToJobObject(self.raw_handle(), process_handle) };
        if assigned == 0 {
            return Err(io::Error::last_os_error());
        }

        let status = unsafe { NtResumeProcess(process_handle) };
        if status < 0 {
            return Err(io::Error::other(format!(
                "failed to resume contained process: NTSTATUS {status:#x}"
            )));
        }
        Ok(child)
    }

    /// Successful hooks may intentionally leave detached helper processes alive.
    pub(crate) fn preserve_descendants(&self) -> io::Result<()> {
        self.set_limit_flags(JOB_OBJECT_LIMIT_BREAKAWAY_OK)
    }

    pub(crate) fn terminate(&self) -> io::Result<()> {
        let terminated = unsafe { TerminateJobObject(self.raw_handle(), 1) };
        if terminated == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn set_limit_flags(&self, flags: u32) -> io::Result<()> {
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = flags;
        let configured = unsafe {
            SetInformationJobObject(
                self.raw_handle(),
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of_mut!(limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn raw_handle(&self) -> HANDLE {
        self.handle.as_raw_handle().cast()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn contained_process_can_complete_and_be_released() {
        let job = WindowsJobObject::create().expect("create job object");
        let mut command = Command::new("cmd.exe");
        command.arg("/C").raw_arg(r#""exit 0""#);

        let mut child = job
            .spawn_contained(&mut command)
            .expect("spawn contained process");
        let status = child.wait().await.expect("wait for contained process");
        assert!(status.success());
        job.preserve_descendants()
            .expect("preserve successful descendants");
    }
}
