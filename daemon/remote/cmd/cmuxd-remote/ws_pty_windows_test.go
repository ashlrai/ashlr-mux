//go:build windows

package main

import (
	"bytes"
	"context"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"nhooyr.io/websocket"
)

func testPowerShellPath(t *testing.T) string {
	t.Helper()
	root := os.Getenv("SystemRoot")
	if root == "" {
		t.Skip("SystemRoot is unavailable")
	}
	path := filepath.Join(root, "System32", "WindowsPowerShell", "v1.0", "powershell.exe")
	if _, err := os.Stat(path); err != nil {
		t.Skipf("Windows PowerShell is unavailable: %v", err)
	}
	return path
}

func TestWindowsPTYCommandUsesNativeShellSyntax(t *testing.T) {
	powershell := testPowerShellPath(t)
	program, args, tmpScript, err := windowsPTYCommand(powershell, "Write-Output 'ok'")
	if err != nil {
		t.Fatalf("windowsPTYCommand: %v", err)
	}
	if program != powershell || tmpScript != "" {
		t.Fatalf("command = (%q, %q), want PowerShell without temp script", program, tmpScript)
	}
	joined := strings.Join(args, " ")
	if !strings.Contains(joined, "-Command") || !strings.Contains(joined, "Write-Output 'ok'") {
		t.Fatalf("PowerShell arguments do not preserve the startup command: %q", args)
	}
}

func TestWindowsWebSocketPTYHubRunsInteractiveConPTY(t *testing.T) {
	powershell := testPowerShellPath(t)
	stderr := &bytes.Buffer{}
	hub := newWebSocketPTYHub(wsPTYServerConfig{Shell: powershell}, stderr)
	t.Cleanup(hub.closeAll)

	attachment, _, sessionDone, err := hub.attachRPC(
		context.Background(), "windows-conpty", "attachment-1", 90, 30, "", "token-1", false, false,
	)
	if err != nil {
		t.Fatalf("attachRPC: %v\nstderr:\n%s", err, stderr.String())
	}

	select {
	case frame := <-attachment.send:
		if frame.messageType != websocket.MessageText || !bytes.Contains(frame.payload, []byte(`"type":"ready"`)) {
			t.Fatalf("first frame = type %v payload %q, want ready event", frame.messageType, frame.payload)
		}
	case <-time.After(10 * time.Second):
		t.Fatal("timed out waiting for PTY ready event")
	}

	if !hub.resizeByID("windows-conpty", "attachment-1", "token-1", 101, 37) {
		t.Fatal("resizeByID rejected the live Windows PTY attachment")
	}
	hub.mu.Lock()
	session := hub.sessions[persistentPTYSessionKey("windows-conpty")]
	hub.mu.Unlock()
	if session == nil {
		t.Fatal("Windows PTY session disappeared after resize")
	}
	session.ptyWriteMu.Lock()
	cols, rows, sizeErr := sizePlatformPTY(session.ptyFile)
	session.ptyWriteMu.Unlock()
	if sizeErr != nil || cols != 101 || rows != 37 {
		t.Fatalf("ConPTY size = %dx%d, %v; want 101x37", cols, rows, sizeErr)
	}
	marker := "cmux-windows-conpty-ok"
	if status := hub.writeInputByID("windows-conpty", "attachment-1", "token-1", []byte("Write-Output '"+marker+"'\r\n")); status != wsPTYInputWriteOK {
		t.Fatalf("writeInputByID status = %v, want ok", status)
	}

	timer := time.NewTimer(15 * time.Second)
	defer timer.Stop()
	for {
		select {
		case frame := <-attachment.send:
			if frame.messageType == websocket.MessageBinary && bytes.Contains(frame.payload, []byte(marker)) {
				if status := hub.writeInputByID("windows-conpty", "attachment-1", "token-1", []byte("exit\r\n")); status != wsPTYInputWriteOK {
					t.Fatalf("exit write status = %v, want ok", status)
				}
				return
			}
		case <-sessionDone:
			t.Fatalf("Windows PTY exited before producing marker\nstderr:\n%s", stderr.String())
		case <-timer.C:
			t.Fatalf("timed out waiting for Windows PTY output\nstderr:\n%s", stderr.String())
		}
	}
}

func TestWindowsWebSocketPTYServerStartsAndShutsDown(t *testing.T) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("reserve listen address: %v", err)
	}
	addr := listener.Addr().String()
	_ = listener.Close()

	ctx, cancel := context.WithCancel(context.Background())
	result := make(chan error, 1)
	leasePath := filepath.Join(t.TempDir(), "lease.json")
	shellPath := testPowerShellPath(t)
	go func() {
		result <- runWebSocketPTYServer(ctx, wsPTYServerConfig{
			ListenAddr:       addr,
			PTYAuthLeaseFile: leasePath,
			Shell:            shellPath,
		}, &bytes.Buffer{})
	}()

	deadline := time.Now().Add(10 * time.Second)
	for {
		response, requestErr := http.Get("http://" + addr + "/healthz")
		if requestErr == nil {
			_ = response.Body.Close()
			if response.StatusCode != http.StatusOK {
				t.Fatalf("health status = %d, want 200", response.StatusCode)
			}
			break
		}
		if time.Now().After(deadline) {
			cancel()
			t.Fatalf("Windows WebSocket PTY server did not start: %v", requestErr)
		}
		time.Sleep(20 * time.Millisecond)
	}

	cancel()
	select {
	case err := <-result:
		if err != nil {
			t.Fatalf("server shutdown: %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("Windows WebSocket PTY server did not shut down")
	}
}
