//go:build windows

package main

import (
	"os/exec"
	"syscall"
	"testing"
)

// TestDetachedDaemonCreationFlagsBreakaway verifies the daemon-survival creation
// flags without spawning a process (Windows Application Control blocks
// standalone spawned test executables — os error 4551 — so lifecycle behavior is
// verified at the flag-computation level only).
func TestDetachedDaemonCreationFlagsBreakaway(t *testing.T) {
	flags := detachedDaemonCreationFlags(true)

	if flags&syscall.CREATE_NEW_PROCESS_GROUP == 0 {
		t.Errorf("expected CREATE_NEW_PROCESS_GROUP to be set, got %#x", flags)
	}
	if flags&_DETACHED_PROCESS == 0 {
		t.Errorf("expected DETACHED_PROCESS to be set, got %#x", flags)
	}
	if flags&_CREATE_BREAKAWAY_FROM_JOB == 0 {
		t.Errorf("expected CREATE_BREAKAWAY_FROM_JOB to be set when breakaway=true, got %#x", flags)
	}
}

// TestDetachedDaemonCreationFlagsNoBreakaway verifies the fallback flag set used
// when the launcher's Job Object denies breakaway. The daemon must still detach
// from the console/process group even though it can no longer escape the job.
func TestDetachedDaemonCreationFlagsNoBreakaway(t *testing.T) {
	flags := detachedDaemonCreationFlags(false)

	if flags&syscall.CREATE_NEW_PROCESS_GROUP == 0 {
		t.Errorf("expected CREATE_NEW_PROCESS_GROUP to be set, got %#x", flags)
	}
	if flags&_DETACHED_PROCESS == 0 {
		t.Errorf("expected DETACHED_PROCESS to be set, got %#x", flags)
	}
	if flags&_CREATE_BREAKAWAY_FROM_JOB != 0 {
		t.Errorf("expected CREATE_BREAKAWAY_FROM_JOB to be clear when breakaway=false, got %#x", flags)
	}
}

// TestDetachedDaemonBreakawayFlagValue guards the locally-defined
// CREATE_BREAKAWAY_FROM_JOB constant against accidental edits; the value is fixed
// by the Windows API (winbase.h).
func TestDetachedDaemonBreakawayFlagValue(t *testing.T) {
	if _CREATE_BREAKAWAY_FROM_JOB != 0x01000000 {
		t.Errorf("CREATE_BREAKAWAY_FROM_JOB must be 0x01000000, got %#x", _CREATE_BREAKAWAY_FROM_JOB)
	}
	if _DETACHED_PROCESS != 0x00000008 {
		t.Errorf("DETACHED_PROCESS must be 0x00000008, got %#x", _DETACHED_PROCESS)
	}
}

// TestConfigureDetachedProcessSetsBreakaway verifies that the default detach
// configuration requests Job Object breakaway so the daemon survives the
// launcher's job handle closing.
func TestConfigureDetachedProcessSetsBreakaway(t *testing.T) {
	cmd := &exec.Cmd{}
	configureDetachedProcess(cmd)

	if cmd.SysProcAttr == nil {
		t.Fatal("expected SysProcAttr to be set")
	}
	if cmd.SysProcAttr.CreationFlags != detachedDaemonCreationFlags(true) {
		t.Errorf(
			"expected CreationFlags %#x, got %#x",
			detachedDaemonCreationFlags(true),
			cmd.SysProcAttr.CreationFlags,
		)
	}
}
