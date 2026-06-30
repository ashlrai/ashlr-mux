//go:build !windows

package main

import (
	"os"
	"os/exec"
	"syscall"
)

// daemonDirectoryOwnedByCurrentUser reports whether the persistent daemon
// directory is owned by the current uid. On Unix this is enforced via the
// POSIX stat owner field.
func daemonDirectoryOwnedByCurrentUser(info os.FileInfo) bool {
	stat, ok := info.Sys().(*syscall.Stat_t)
	return !ok || int(stat.Uid) == os.Getuid()
}

// configureDetachedProcess detaches the spawned persistent daemon from the
// caller's controlling terminal/session so it survives the parent exiting.
func configureDetachedProcess(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
}

// lockDaemonSlot takes a non-blocking exclusive advisory lock on the slot lock
// file, returning an error if another instance already holds it.
func lockDaemonSlot(f *os.File) error {
	return syscall.Flock(int(f.Fd()), syscall.LOCK_EX|syscall.LOCK_NB)
}

// unlockDaemonSlot releases the advisory lock taken by lockDaemonSlot.
func unlockDaemonSlot(f *os.File) {
	_ = syscall.Flock(int(f.Fd()), syscall.LOCK_UN)
}
