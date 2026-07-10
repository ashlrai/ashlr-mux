//go:build windows

package main

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"syscall"

	"github.com/charmbracelet/x/conpty"
	"golang.org/x/sys/windows"
)

const windowsPTYCommandLimit = 24 * 1024

type windowsPTYProcess struct {
	handle windows.Handle
	once   sync.Once
}

func (p *windowsPTYProcess) Wait() error {
	_, err := windows.WaitForSingleObject(p.handle, windows.INFINITE)
	p.close()
	return err
}

func (p *windowsPTYProcess) Kill() error {
	err := windows.TerminateProcess(p.handle, 1)
	if err == windows.ERROR_ACCESS_DENIED {
		return nil
	}
	return err
}

func (p *windowsPTYProcess) close() {
	p.once.Do(func() {
		_ = windows.CloseHandle(p.handle)
	})
}

func startPlatformPTY(shellPath string, command string, cols int, rows int, env []string) (io.ReadWriteCloser, wsPTYProcess, string, error) {
	program, args, tmpScript, err := windowsPTYCommand(shellPath, command)
	if err != nil {
		return nil, nil, "", err
	}
	cleanupScript := true
	defer func() {
		if cleanupScript && tmpScript != "" {
			_ = os.Remove(tmpScript)
		}
	}()

	console, err := conpty.New(cols, rows, 0)
	if err != nil {
		return nil, nil, "", fmt.Errorf("could not allocate a Windows ConPTY: %w", err)
	}
	started := false
	defer func() {
		if !started {
			_ = console.Close()
		}
	}()

	_, processHandle, err := console.Spawn(program, args, &syscall.ProcAttr{Env: env})
	if err != nil {
		return nil, nil, "", fmt.Errorf("could not start Windows PTY shell: %w", err)
	}
	started = true
	cleanupScript = false
	return console, &windowsPTYProcess{handle: windows.Handle(processHandle)}, tmpScript, nil
}

func windowsPTYCommand(shellPath string, command string) (string, []string, string, error) {
	base := strings.ToLower(filepath.Base(shellPath))
	isPowerShell := base == "powershell.exe" || base == "powershell" || base == "pwsh.exe" || base == "pwsh"
	isCmd := base == "cmd.exe" || base == "cmd"
	if command == "" {
		if isPowerShell {
			return shellPath, []string{shellPath, "-NoLogo"}, "", nil
		}
		if isCmd {
			return shellPath, []string{shellPath, "/Q"}, "", nil
		}
		return shellPath, []string{shellPath}, "", nil
	}

	if len(command) > windowsPTYCommandLimit {
		ext := ".sh"
		switch {
		case isPowerShell:
			ext = ".ps1"
		case isCmd:
			ext = ".cmd"
		}
		file, err := os.CreateTemp("", "cmuxd-startup-*"+ext)
		if err != nil {
			return "", nil, "", fmt.Errorf("could not create startup script temp file: %w", err)
		}
		tmpScript := file.Name()
		if _, err := file.WriteString(command); err != nil {
			_ = file.Close()
			_ = os.Remove(tmpScript)
			return "", nil, "", fmt.Errorf("could not write startup script: %w", err)
		}
		_ = file.Close()
		if isPowerShell {
			return shellPath, []string{shellPath, "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", tmpScript}, tmpScript, nil
		}
		if isCmd {
			return shellPath, []string{shellPath, "/D", "/Q", "/C", tmpScript}, tmpScript, nil
		}
		return shellPath, []string{shellPath, tmpScript}, tmpScript, nil
	}

	if isPowerShell {
		return shellPath, []string{shellPath, "-NoLogo", "-NoProfile", "-Command", command}, "", nil
	}
	if isCmd {
		return shellPath, []string{shellPath, "/D", "/Q", "/S", "/C", command}, "", nil
	}
	return shellPath, []string{shellPath, "-c", command}, "", nil
}

func resizePlatformPTY(file io.ReadWriteCloser, cols int, rows int) error {
	console, ok := file.(*conpty.ConPty)
	if !ok {
		return fmt.Errorf("unsupported Windows PTY type %T", file)
	}
	return console.Resize(cols, rows)
}

func sizePlatformPTY(file io.ReadWriteCloser) (int, int, error) {
	console, ok := file.(*conpty.ConPty)
	if !ok {
		return 0, 0, fmt.Errorf("unsupported Windows PTY type %T", file)
	}
	return console.Size()
}

func resolvePTYShell(explicit string) string {
	if shell := strings.TrimSpace(explicit); shell != "" {
		return shell
	}
	if root := strings.TrimSpace(os.Getenv("SystemRoot")); root != "" {
		powershell := filepath.Join(root, "System32", "WindowsPowerShell", "v1.0", "powershell.exe")
		if _, err := os.Stat(powershell); err == nil {
			return powershell
		}
	}
	if shell := strings.TrimSpace(os.Getenv("COMSPEC")); shell != "" {
		return shell
	}
	return `C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe`
}
