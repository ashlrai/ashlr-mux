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
// Setsid places the daemon in a new session with no controlling terminal, which
// is the POSIX analogue of the Windows console + Job Object breakaway used to
// keep the daemon alive after the launcher exits.
func configureDetachedProcess(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
}

// startDetachedDaemon starts the detached daemon process. On Unix, Setsid alone
// is sufficient for the daemon to outlive its launcher, so there is no
// breakaway fallback to perform (unlike the Windows Job Object case).
func startDetachedDaemon(cmd *exec.Cmd) error {
	return cmd.Start()
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
