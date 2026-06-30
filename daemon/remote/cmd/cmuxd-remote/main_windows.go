//go:build windows

package main

import (
	"os"
	"os/exec"
	"syscall"
	"unsafe"
)

// Windows process creation flags. DETACHED_PROCESS is not exported by the
// standard syscall package, so it is defined locally.
const (
	_DETACHED_PROCESS = 0x00000008
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

// configureDetachedProcess detaches the spawned persistent daemon from the
// caller's console and process group so it survives the parent exiting.
func configureDetachedProcess(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{
		CreationFlags: syscall.CREATE_NEW_PROCESS_GROUP | _DETACHED_PROCESS,
	}
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
