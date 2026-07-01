//go:build windows

package main

import (
	"errors"
	"os"
	"os/exec"
	"syscall"
	"unsafe"
)

// Windows process creation flags. DETACHED_PROCESS and
// CREATE_BREAKAWAY_FROM_JOB are not exported by the standard syscall package, so
// they are defined locally.
const (
	_DETACHED_PROCESS          = 0x00000008
	_CREATE_BREAKAWAY_FROM_JOB = 0x01000000
)

// Windows LockFileEx flags.
const (
	_LOCKFILE_FAIL_IMMEDIATELY = 0x00000001
	_LOCKFILE_EXCLUSIVE_LOCK   = 0x00000002
)

var (
	modkernel32      = syscall.NewLazyDLL("kernel32.dll")
	procLockFileEx   = modkernel32.NewProc("LockFileEx")
	procUnlockFileEx = modkernel32.NewProc("UnlockFileEx")
)

// daemonDirectoryOwnedByCurrentUser reports whether the persistent daemon
// directory is owned by the current user. Windows uses ACLs rather than POSIX
// uid ownership (os.Getuid() returns -1), so directory creation/permissions are
// trusted here.
func daemonDirectoryOwnedByCurrentUser(info os.FileInfo) bool {
	return true
}

// detachedDaemonCreationFlags computes the CreateProcess creation flags used to
// spawn the persistent daemon so it survives the launching process exiting.
//
//   - CREATE_NEW_PROCESS_GROUP + DETACHED_PROCESS detach the daemon from the
//     launcher's console/process group (no inherited Ctrl-C, no console tie).
//   - CREATE_BREAKAWAY_FROM_JOB escapes any Job Object the launcher belongs to.
//     The app-side launcher (and the Rust cmux-process supervisor it reuses)
//     assigns children to a Job Object with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
//     so terminals/agents die with their parent. The daemon needs the OPPOSITE:
//     it must outlive the app. DETACHED_PROCESS alone does NOT leave a Job
//     Object, and a nested standalone job does not shield a process from an
//     ancestor job's KILL_ON_JOB_CLOSE, so breakaway is the only mechanism that
//     lets the daemon survive the launcher's job handle closing.
//
// Breakaway only succeeds when the launcher's job permits it
// (JOB_OBJECT_LIMIT_BREAKAWAY_OK) or the launcher is not in a job at all; when
// it is denied the caller retries without the flag (see startDetachedDaemon).
func detachedDaemonCreationFlags(breakaway bool) uint32 {
	flags := uint32(syscall.CREATE_NEW_PROCESS_GROUP | _DETACHED_PROCESS)
	if breakaway {
		flags |= _CREATE_BREAKAWAY_FROM_JOB
	}
	return flags
}

// configureDetachedProcess detaches the spawned persistent daemon from the
// caller's console, process group, and Job Object so it survives the parent
// exiting.
func configureDetachedProcess(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{
		CreationFlags: detachedDaemonCreationFlags(true),
	}
}

// startDetachedDaemon starts the detached daemon process. If breakaway from the
// launcher's Job Object is denied (ERROR_ACCESS_DENIED — the launcher is in a
// job that lacks JOB_OBJECT_LIMIT_BREAKAWAY_OK), it retries once without the
// breakaway flag. In that fallback the daemon can only outlive the launcher if
// the launcher's job does not set KILL_ON_JOB_CLOSE; retrying is strictly better
// than failing to launch and never worse than the pre-breakaway behavior.
func startDetachedDaemon(cmd *exec.Cmd) error {
	err := cmd.Start()
	if err == nil || !errors.Is(err, syscall.ERROR_ACCESS_DENIED) {
		return err
	}
	if cmd.SysProcAttr == nil {
		return err
	}
	cmd.SysProcAttr.CreationFlags = detachedDaemonCreationFlags(false)
	return cmd.Start()
}

// lockDaemonSlot takes a non-blocking exclusive lock on the slot lock file via
// LockFileEx, returning an error if another instance already holds it.
func lockDaemonSlot(f *os.File) error {
	ol := new(syscall.Overlapped)
	r1, _, err := procLockFileEx.Call(
		f.Fd(),
		uintptr(_LOCKFILE_FAIL_IMMEDIATELY|_LOCKFILE_EXCLUSIVE_LOCK),
		0,
		uintptr(^uint32(0)),
		uintptr(^uint32(0)),
		uintptr(unsafe.Pointer(ol)),
	)
	if r1 == 0 {
		return err
	}
	return nil
}

// unlockDaemonSlot releases the lock taken by lockDaemonSlot.
func unlockDaemonSlot(f *os.File) {
	ol := new(syscall.Overlapped)
	_, _, _ = procUnlockFileEx.Call(
		f.Fd(),
		0,
		uintptr(^uint32(0)),
		uintptr(^uint32(0)),
		uintptr(unsafe.Pointer(ol)),
	)
}
