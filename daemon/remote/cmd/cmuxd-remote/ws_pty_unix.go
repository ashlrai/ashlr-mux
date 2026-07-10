//go:build !windows

package main

import (
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"

	"github.com/creack/pty"
)

type unixPTYProcess struct {
	cmd *exec.Cmd
}

func (p *unixPTYProcess) Wait() error {
	return p.cmd.Wait()
}

func (p *unixPTYProcess) Kill() error {
	if p.cmd.Process == nil {
		return nil
	}
	return p.cmd.Process.Kill()
}

func startPlatformPTY(shellPath string, command string, cols int, rows int, env []string) (io.ReadWriteCloser, wsPTYProcess, string, error) {
	cmd, tmpScript, err := unixPTYCommand(shellPath, command)
	if err != nil {
		return nil, nil, "", err
	}
	cleanupScript := true
	defer func() {
		if cleanupScript && tmpScript != "" {
			_ = os.Remove(tmpScript)
		}
	}()

	ptyFile, ttyFile, err := pty.Open()
	if err != nil {
		return nil, nil, "", newPTYAllocationError(err)
	}
	closeFiles := true
	defer func() {
		if closeFiles {
			_ = ptyFile.Close()
			_ = ttyFile.Close()
		}
	}()

	if err := pty.Setsize(ttyFile, &pty.Winsize{Cols: uint16(cols), Rows: uint16(rows)}); err != nil {
		return nil, nil, "", err
	}
	cmd.Env = env
	cmd.Stdout = ttyFile
	cmd.Stderr = ttyFile
	cmd.Stdin = ttyFile
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true, Setctty: true}
	if err := cmd.Start(); err != nil {
		return nil, nil, "", err
	}

	closeFiles = false
	cleanupScript = false
	_ = ttyFile.Close()
	return ptyFile, &unixPTYProcess{cmd: cmd}, tmpScript, nil
}

func unixPTYCommand(shellPath string, command string) (*exec.Cmd, string, error) {
	if command == "" {
		return exec.Command(shellPath), "", nil
	}
	if len(command) <= 120*1024 {
		return exec.Command("/bin/sh", "-c", command), "", nil
	}

	f, err := os.CreateTemp("", "cmuxd-startup-*.sh")
	if err != nil {
		return nil, "", fmt.Errorf("could not create startup script temp file: %w", err)
	}
	tmpScript := f.Name()
	if _, err := f.WriteString(command); err != nil {
		_ = f.Close()
		_ = os.Remove(tmpScript)
		return nil, "", fmt.Errorf("could not write startup script: %w", err)
	}
	_ = f.Chmod(0o400)
	_ = f.Close()
	return exec.Command("/bin/sh", tmpScript), tmpScript, nil
}

func resizePlatformPTY(file io.ReadWriteCloser, cols int, rows int) error {
	ptyFile, ok := file.(*os.File)
	if !ok {
		return fmt.Errorf("unsupported Unix PTY type %T", file)
	}
	return pty.Setsize(ptyFile, &pty.Winsize{Cols: uint16(cols), Rows: uint16(rows)})
}

func sizePlatformPTY(file io.ReadWriteCloser) (int, int, error) {
	ptyFile, ok := file.(*os.File)
	if !ok {
		return 0, 0, fmt.Errorf("unsupported Unix PTY type %T", file)
	}
	size, err := pty.GetsizeFull(ptyFile)
	if err != nil {
		return 0, 0, err
	}
	return int(size.Cols), int(size.Rows), nil
}

// newPTYAllocationError wraps a raw PTY-allocation failure with actionable
// diagnostics about the remote devpts. See issue #5185.
func newPTYAllocationError(err error) error {
	suffix := ""
	if detail := describeDevPTS(); detail != "" {
		suffix = "; " + detail
	}
	hint := ""
	if isPermissionDeniedErr(err) {
		hint = "; the remote devpts denies /dev/ptmx (e.g. mounted ptmxmode=000): remount it writable with `sudo mount -o remount,ptmxmode=0666 /dev/pts` or expose a writable /dev/ptmx so the cmux daemon can open a terminal"
	}
	return fmt.Errorf("could not allocate a remote PTY: %w%s%s", err, suffix, hint)
}

func describeDevPTS() string {
	var parts []string
	if info, statErr := os.Stat("/dev/ptmx"); statErr == nil {
		parts = append(parts, fmt.Sprintf("/dev/ptmx mode=%04o", info.Mode().Perm()))
	} else {
		parts = append(parts, fmt.Sprintf("/dev/ptmx stat error: %v", statErr))
	}
	if opts := devptsMountOptions(); opts != "" {
		parts = append(parts, "devpts ("+opts+")")
	}
	return strings.Join(parts, "; ")
}

func devptsMountOptions() string {
	data, err := os.ReadFile("/proc/self/mountinfo")
	if err != nil {
		return ""
	}
	for _, line := range strings.Split(string(data), "\n") {
		fields := strings.Fields(line)
		if len(fields) < 5 || fields[4] != "/dev/pts" {
			continue
		}
		sep := -1
		for i, field := range fields {
			if field == "-" {
				sep = i
				break
			}
		}
		if sep >= 0 && sep+3 < len(fields) && fields[sep+1] == "devpts" {
			return fields[sep+3]
		}
	}
	return ""
}

func isPermissionDeniedErr(err error) bool {
	return errors.Is(err, os.ErrPermission) || errors.Is(err, syscall.EACCES) || errors.Is(err, syscall.EPERM)
}

func resolvePTYShell(explicit string) string {
	if strings.TrimSpace(explicit) != "" {
		return explicit
	}
	if shell := strings.TrimSpace(os.Getenv("SHELL")); shell != "" {
		if _, err := os.Stat(shell); err == nil {
			return shell
		}
	}
	for _, candidate := range []string{"/bin/bash", "/usr/bin/bash", "/bin/sh"} {
		if _, err := os.Stat(candidate); err == nil {
			return candidate
		}
	}
	return filepath.Clean("/bin/sh")
}
