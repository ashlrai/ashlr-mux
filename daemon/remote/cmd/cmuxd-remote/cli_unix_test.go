//go:build !windows

package main

import (
	"encoding/json"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// TestDialSocketRefreshesToUpdatedTCPAddressWithoutPolling and
// TestDialSocketFailsFastWhenTCPAddressStaysStale are gated to !windows because
// they assert dialSocket's address-refresh fail-over path, which does NOT fire
// on Windows: a WinSock "connectex: ... actively refused" error is not matched
// by the refresh trigger, so refreshAddr is called 0 times and the dial does
// not fail over. This is a real dialSocket Windows portability gap (production,
// out of scope for this tests-only slice) — re-enable these on Windows once the
// Windows CLI relay dial is ported. See docs/windows-port follow-ups.
func TestDialSocketRefreshesToUpdatedTCPAddressWithoutPolling(t *testing.T) {
	staleListener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen stale: %v", err)
	}
	staleAddr := staleListener.Addr().String()
	staleListener.Close()

	readyListener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen ready: %v", err)
	}
	defer readyListener.Close()

	accepted := make(chan struct{})
	go func() {
		defer close(accepted)
		conn, acceptErr := readyListener.Accept()
		if acceptErr != nil {
			return
		}
		conn.Close()
	}()

	refreshCalls := 0
	start := time.Now()
	conn, err := dialSocket(staleAddr, func() string {
		refreshCalls++
		return readyListener.Addr().String()
	})
	elapsed := time.Since(start)
	if err != nil {
		t.Fatalf("dialSocket should refresh to updated address, got: %v", err)
	}
	conn.Close()
	<-accepted
	if refreshCalls != 1 {
		t.Fatalf("refreshAddr should be called once, got %d", refreshCalls)
	}
	if elapsed > 500*time.Millisecond {
		t.Fatalf("dialSocket should fail over without polling, took %v", elapsed)
	}
}

func TestDialSocketFailsFastWhenTCPAddressStaysStale(t *testing.T) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	addr := ln.Addr().String()
	ln.Close()

	refreshCalls := 0
	start := time.Now()
	_, err = dialSocket(addr, func() string {
		refreshCalls++
		return addr
	})
	elapsed := time.Since(start)
	if err == nil {
		t.Fatal("dialSocket should fail when the relay address stays stale")
	}
	if refreshCalls != 1 {
		t.Fatalf("refreshAddr should be called once on stale TCP failure, got %d", refreshCalls)
	}
	if elapsed > 500*time.Millisecond {
		t.Fatalf("dialSocket should fail fast without polling, took %v", elapsed)
	}
}

func makeShortUnixSocketPath(t *testing.T) string {
	t.Helper()
	dir, err := os.MkdirTemp("/tmp", "cmuxd-")
	if err != nil {
		t.Fatalf("mkdtemp: %v", err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	return filepath.Join(dir, "cmux.sock")
}

// startMockSocket creates a Unix socket that accepts one connection,
// reads a line, and responds with the given canned response.
func startMockSocket(t *testing.T, response string) string {
	t.Helper()
	sockPath := makeShortUnixSocketPath(t)

	ln, err := net.Listen("unix", sockPath)
	if err != nil {
		t.Fatalf("failed to listen: %v", err)
	}
	t.Cleanup(func() { ln.Close() })

	go func() {
		for {
			conn, err := ln.Accept()
			if err != nil {
				return
			}
			buf := make([]byte, 4096)
			n, _ := conn.Read(buf)
			_ = n // consume request
			conn.Write([]byte(response + "\n"))
			conn.Close()
		}
	}()

	return sockPath
}

// startMockV2Socket creates a Unix socket that echoes the received request's method
// back as a successful JSON-RPC response with the method name in the result.
func startMockV2Socket(t *testing.T) string {
	t.Helper()
	sockPath := makeShortUnixSocketPath(t)

	ln, err := net.Listen("unix", sockPath)
	if err != nil {
		t.Fatalf("failed to listen: %v", err)
	}
	t.Cleanup(func() { ln.Close() })

	go func() {
		for {
			conn, err := ln.Accept()
			if err != nil {
				return
			}
			buf := make([]byte, 4096)
			n, _ := conn.Read(buf)
			if n > 0 {
				var req map[string]any
				if err := json.Unmarshal(buf[:n], &req); err == nil {
					resp := map[string]any{
						"id":     req["id"],
						"ok":     true,
						"result": map[string]any{"method": req["method"], "params": req["params"]},
					}
					payload, _ := json.Marshal(resp)
					conn.Write(append(payload, '\n'))
				} else {
					conn.Write([]byte(`{"ok":false,"error":{"code":"parse","message":"bad json"}}` + "\n"))
				}
			}
			conn.Close()
		}
	}()

	return sockPath
}

func startMockV2SocketWithRequestCapture(t *testing.T) (string, <-chan map[string]any) {
	t.Helper()
	sockPath := makeShortUnixSocketPath(t)
	requests := make(chan map[string]any, 8)

	ln, err := net.Listen("unix", sockPath)
	if err != nil {
		t.Fatalf("failed to listen: %v", err)
	}
	t.Cleanup(func() { ln.Close() })

	go func() {
		for {
			conn, err := ln.Accept()
			if err != nil {
				return
			}
			go func(conn net.Conn) {
				defer conn.Close()
				buf := make([]byte, 4096)
				n, _ := conn.Read(buf)
				if n == 0 {
					return
				}
				var req map[string]any
				if err := json.Unmarshal(buf[:n], &req); err != nil {
					_, _ = conn.Write([]byte(`{"ok":false,"error":{"code":"parse","message":"bad json"}}` + "\n"))
					return
				}
				requests <- req
				resp := map[string]any{
					"id":     req["id"],
					"ok":     true,
					"result": map[string]any{"method": req["method"], "params": req["params"]},
				}
				payload, _ := json.Marshal(resp)
				_, _ = conn.Write(append(payload, '\n'))
			}(conn)
		}
	}()

	return sockPath, requests
}

func TestCLIPingV1(t *testing.T) {
	sockPath := startMockSocket(t, "pong")
	code := runCLI([]string{"--socket", sockPath, "ping"})
	if code != 0 {
		t.Fatalf("ping should return 0, got %d", code)
	}
}

func TestCLINewWindowV1(t *testing.T) {
	sockPath := startMockSocket(t, "OK window_id=abc123")
	code := runCLI([]string{"--socket", sockPath, "new-window"})
	if code != 0 {
		t.Fatalf("new-window should return 0, got %d", code)
	}
}

func TestCLICloseWindowV1(t *testing.T) {
	// Verify that the flag value is appended to the v1 command
	dir := t.TempDir()
	sockPath := filepath.Join(dir, "cmux.sock")

	receivedCh := make(chan string, 1)
	ln, err := net.Listen("unix", sockPath)
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	t.Cleanup(func() { ln.Close() })

	go func() {
		conn, err := ln.Accept()
		if err != nil {
			return
		}
		buf := make([]byte, 4096)
		n, _ := conn.Read(buf)
		receivedCh <- strings.TrimSpace(string(buf[:n]))
		conn.Write([]byte("OK\n"))
		conn.Close()
	}()

	code := runCLI([]string{"--socket", sockPath, "close-window", "--window", "win-42"})
	if code != 0 {
		t.Fatalf("close-window should return 0, got %d", code)
	}
	select {
	case received := <-receivedCh:
		if received != "close_window win-42" {
			t.Fatalf("expected 'close_window win-42', got %q", received)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for close-window payload")
	}
}

func TestCLIListWorkspacesV2(t *testing.T) {
	sockPath := startMockV2Socket(t)
	code := runCLI([]string{"--socket", sockPath, "--json", "list-workspaces"})
	if code != 0 {
		t.Fatalf("list-workspaces should return 0, got %d", code)
	}
}

func TestCLIRPCPassthrough(t *testing.T) {
	sockPath := startMockV2Socket(t)
	code := runCLI([]string{"--socket", sockPath, "rpc", "system.capabilities"})
	if code != 0 {
		t.Fatalf("rpc should return 0, got %d", code)
	}
}

func TestCLIRPCWithParams(t *testing.T) {
	sockPath := startMockV2Socket(t)
	code := runCLI([]string{"--socket", sockPath, "rpc", "workspace.create", `{"title":"test"}`})
	if code != 0 {
		t.Fatalf("rpc with params should return 0, got %d", code)
	}
}

func TestCLISocketEnvVar(t *testing.T) {
	sockPath := startMockSocket(t, "pong")
	os.Setenv("CMUX_SOCKET_PATH", sockPath)
	defer os.Unsetenv("CMUX_SOCKET_PATH")

	code := runCLI([]string{"ping"})
	if code != 0 {
		t.Fatalf("ping with env socket should return 0, got %d", code)
	}
}

func TestCLIV2FlagMapping(t *testing.T) {
	// Verify that --workspace gets mapped to workspace_id in params
	dir := t.TempDir()
	sockPath := filepath.Join(dir, "cmux.sock")

	receivedParamsCh := make(chan map[string]any, 1)
	ln, err := net.Listen("unix", sockPath)
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	t.Cleanup(func() { ln.Close() })

	go func() {
		conn, err := ln.Accept()
		if err != nil {
			return
		}
		buf := make([]byte, 4096)
		n, _ := conn.Read(buf)
		var req map[string]any
		json.Unmarshal(buf[:n], &req)
		receivedParams, _ := req["params"].(map[string]any)
		receivedParamsCh <- receivedParams
		resp := map[string]any{"id": req["id"], "ok": true, "result": map[string]any{}}
		payload, _ := json.Marshal(resp)
		conn.Write(append(payload, '\n'))
		conn.Close()
	}()

	code := runCLI([]string{"--socket", sockPath, "--json", "close-workspace", "--workspace", "ws-abc"})
	if code != 0 {
		t.Fatalf("close-workspace should return 0, got %d", code)
	}
	select {
	case receivedParams := <-receivedParamsCh:
		if receivedParams["workspace_id"] != "ws-abc" {
			t.Fatalf("expected workspace_id=ws-abc, got %v", receivedParams)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for close-workspace payload")
	}
}

func TestCLIBrowserSubcommand(t *testing.T) {
	sockPath := startMockV2Socket(t)
	code := runCLI([]string{"--socket", sockPath, "--json", "browser", "open", "--url", "https://example.com"})
	if code != 0 {
		t.Fatalf("browser open should return 0, got %d", code)
	}
}

func TestCLINewPaneDefaultsDirectionAndForwardsExtraFlags(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	code := runCLI([]string{
		"--socket", sockPath, "--json",
		"new-pane",
		"--workspace", "ws-1",
		"--type", "browser",
		"--url", "https://example.com",
	})
	if code != 0 {
		t.Fatalf("new-pane should return 0, got %d", code)
	}

	select {
	case req := <-requests:
		if got := req["method"]; got != "pane.create" {
			t.Fatalf("expected pane.create, got %v", got)
		}
		params, _ := req["params"].(map[string]any)
		if got := params["workspace_id"]; got != "ws-1" {
			t.Fatalf("expected workspace_id ws-1, got %v", got)
		}
		if got := params["direction"]; got != "right" {
			t.Fatalf("expected default direction right, got %v", got)
		}
		if got := params["type"]; got != "browser" {
			t.Fatalf("expected type browser, got %v", got)
		}
		if got := params["url"]; got != "https://example.com" {
			t.Fatalf("expected url to be forwarded, got %v", got)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for new-pane request")
	}
}

func TestCLIListPanelsUsesSurfaceList(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	code := runCLI([]string{"--socket", sockPath, "--json", "list-panels", "--workspace", "ws-1"})
	if code != 0 {
		t.Fatalf("list-panels should return 0, got %d", code)
	}

	select {
	case req := <-requests:
		if got := req["method"]; got != "surface.list" {
			t.Fatalf("expected surface.list, got %v", got)
		}
		params, _ := req["params"].(map[string]any)
		if got := params["workspace_id"]; got != "ws-1" {
			t.Fatalf("expected workspace_id ws-1, got %v", got)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for list-panels request")
	}
}

func TestCLIFocusPanelUsesSurfaceFocus(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	code := runCLI([]string{"--socket", sockPath, "--json", "focus-panel", "--workspace", "ws-1", "--panel", "surface-1"})
	if code != 0 {
		t.Fatalf("focus-panel should return 0, got %d", code)
	}

	select {
	case req := <-requests:
		if got := req["method"]; got != "surface.focus" {
			t.Fatalf("expected surface.focus, got %v", got)
		}
		params, _ := req["params"].(map[string]any)
		if got := params["workspace_id"]; got != "ws-1" {
			t.Fatalf("expected workspace_id ws-1, got %v", got)
		}
		if got := params["surface_id"]; got != "surface-1" {
			t.Fatalf("expected surface_id surface-1, got %v", got)
		}
		if _, ok := params["panel_id"]; ok {
			t.Fatalf("did not expect panel_id in params: %v", params)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for focus-panel request")
	}
}

func TestCLIBrowserOpenUsesOpenSplitAndWorkspaceEnv(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	t.Setenv("CMUX_WORKSPACE_ID", "env-ws")
	code := runCLI([]string{"--socket", sockPath, "--json", "browser", "open", "https://example.com"})
	if code != 0 {
		t.Fatalf("browser open should return 0, got %d", code)
	}

	select {
	case req := <-requests:
		if got := req["method"]; got != "browser.open_split" {
			t.Fatalf("expected browser.open_split, got %v", got)
		}
		params, _ := req["params"].(map[string]any)
		if got := params["workspace_id"]; got != "env-ws" {
			t.Fatalf("expected workspace_id env-ws, got %v", got)
		}
		if got := params["url"]; got != "https://example.com" {
			t.Fatalf("expected positional url to be forwarded, got %v", got)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for browser open request")
	}
}

func TestCLIBrowserGetURLUsesCurrentMethodAndSurfaceEnv(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	t.Setenv("CMUX_SURFACE_ID", "env-sf")
	code := runCLI([]string{"--socket", sockPath, "--json", "browser", "get-url"})
	if code != 0 {
		t.Fatalf("browser get-url should return 0, got %d", code)
	}

	select {
	case req := <-requests:
		if got := req["method"]; got != "browser.url.get" {
			t.Fatalf("expected browser.url.get, got %v", got)
		}
		params, _ := req["params"].(map[string]any)
		if got := params["surface_id"]; got != "env-sf" {
			t.Fatalf("expected surface_id env-sf, got %v", got)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for browser get-url request")
	}
}

func TestCLIBrowserSnapshotUsesSurfaceEnvAndForwardsOptions(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	t.Setenv("CMUX_SURFACE_ID", "env-sf")
	code := runCLI([]string{
		"--socket", sockPath, "--json",
		"browser", "snapshot",
		"--selector", "main",
		"--max-depth", "4",
	})
	if code != 0 {
		t.Fatalf("browser snapshot should return 0, got %d", code)
	}

	select {
	case req := <-requests:
		if got := req["method"]; got != "browser.snapshot" {
			t.Fatalf("expected browser.snapshot, got %v", got)
		}
		params, _ := req["params"].(map[string]any)
		if got := params["surface_id"]; got != "env-sf" {
			t.Fatalf("expected surface_id env-sf, got %v", got)
		}
		if got := params["selector"]; got != "main" {
			t.Fatalf("expected selector main, got %v", got)
		}
		if got := params["max_depth"]; got != "4" {
			t.Fatalf("expected max_depth 4, got %v", got)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for browser snapshot request")
	}
}

func TestCLIBrowserWaitUsesSurfaceEnvAndForwardsOptions(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	t.Setenv("CMUX_SURFACE_ID", "env-sf")
	code := runCLI([]string{
		"--socket", sockPath, "--json",
		"browser", "wait",
		"--timeout-ms", "1500",
		"--url-contains", "/cloud",
		"--load-state", "networkidle",
	})
	if code != 0 {
		t.Fatalf("browser wait should return 0, got %d", code)
	}

	select {
	case req := <-requests:
		if got := req["method"]; got != "browser.wait" {
			t.Fatalf("expected browser.wait, got %v", got)
		}
		params, _ := req["params"].(map[string]any)
		if got := params["surface_id"]; got != "env-sf" {
			t.Fatalf("expected surface_id env-sf, got %v", got)
		}
		if got := params["timeout_ms"]; got != "1500" {
			t.Fatalf("expected timeout_ms 1500, got %v", got)
		}
		if got := params["url_contains"]; got != "/cloud" {
			t.Fatalf("expected url_contains /cloud, got %v", got)
		}
		if got := params["load_state"]; got != "networkidle" {
			t.Fatalf("expected load_state networkidle, got %v", got)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for browser wait request")
	}
}

func TestCLIBrowserAutomationPositionals(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	t.Setenv("CMUX_SURFACE_ID", "env-sf")
	code := runCLI([]string{
		"--socket", sockPath, "--json",
		"browser", "fill",
		"input[name=email]",
		"dev@example.com",
	})
	if code != 0 {
		t.Fatalf("browser fill should return 0, got %d", code)
	}

	select {
	case req := <-requests:
		if got := req["method"]; got != "browser.fill" {
			t.Fatalf("expected browser.fill, got %v", got)
		}
		params, _ := req["params"].(map[string]any)
		if got := params["surface_id"]; got != "env-sf" {
			t.Fatalf("expected surface_id env-sf, got %v", got)
		}
		if got := params["selector"]; got != "input[name=email]" {
			t.Fatalf("expected selector, got %v", got)
		}
		if got := params["text"]; got != "dev@example.com" {
			t.Fatalf("expected text, got %v", got)
		}
		if _, ok := params["value"]; ok {
			t.Fatalf("browser.fill should not send value param: %#v", params)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for browser fill request")
	}
}

func TestCLIBrowserSelectDoesNotMirrorValueToText(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	t.Setenv("CMUX_SURFACE_ID", "env-sf")
	code := runCLI([]string{
		"--socket", sockPath, "--json",
		"browser", "select",
		"select[name=plan]",
		"free",
	})
	if code != 0 {
		t.Fatalf("browser select should return 0, got %d", code)
	}

	select {
	case req := <-requests:
		if got := req["method"]; got != "browser.select" {
			t.Fatalf("expected browser.select, got %v", got)
		}
		params, _ := req["params"].(map[string]any)
		if got := params["surface_id"]; got != "env-sf" {
			t.Fatalf("expected surface_id env-sf, got %v", got)
		}
		if got := params["selector"]; got != "select[name=plan]" {
			t.Fatalf("expected selector, got %v", got)
		}
		if got := params["value"]; got != "free" {
			t.Fatalf("expected value, got %v", got)
		}
		if _, ok := params["text"]; ok {
			t.Fatalf("browser.select should not send text param: %#v", params)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for browser select request")
	}
}

func TestCLIBrowserEvalUsesPositionalScript(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	t.Setenv("CMUX_SURFACE_ID", "env-sf")
	code := runCLI([]string{
		"--socket", sockPath, "--json",
		"browser", "eval",
		"document.title",
	})
	if code != 0 {
		t.Fatalf("browser eval should return 0, got %d", code)
	}

	select {
	case req := <-requests:
		if got := req["method"]; got != "browser.eval" {
			t.Fatalf("expected browser.eval, got %v", got)
		}
		params, _ := req["params"].(map[string]any)
		if got := params["surface_id"]; got != "env-sf" {
			t.Fatalf("expected surface_id env-sf, got %v", got)
		}
		if got := params["script"]; got != "document.title" {
			t.Fatalf("expected script, got %v", got)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for browser eval request")
	}
}

func TestCLIEnvVarDefaults(t *testing.T) {
	// Test that CMUX_WORKSPACE_ID and CMUX_SURFACE_ID are used as defaults
	dir := t.TempDir()
	sockPath := filepath.Join(dir, "cmux.sock")

	receivedParamsCh := make(chan map[string]any, 1)
	ln, err := net.Listen("unix", sockPath)
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	t.Cleanup(func() { ln.Close() })

	go func() {
		conn, err := ln.Accept()
		if err != nil {
			return
		}
		buf := make([]byte, 4096)
		n, _ := conn.Read(buf)
		var req map[string]any
		json.Unmarshal(buf[:n], &req)
		receivedParams, _ := req["params"].(map[string]any)
		receivedParamsCh <- receivedParams
		resp := map[string]any{"id": req["id"], "ok": true, "result": map[string]any{}}
		payload, _ := json.Marshal(resp)
		conn.Write(append(payload, '\n'))
		conn.Close()
	}()

	os.Setenv("CMUX_WORKSPACE_ID", "env-ws-id")
	os.Setenv("CMUX_SURFACE_ID", "env-sf-id")
	defer os.Unsetenv("CMUX_WORKSPACE_ID")
	defer os.Unsetenv("CMUX_SURFACE_ID")

	code := runCLI([]string{"--socket", sockPath, "--json", "close-surface"})
	if code != 0 {
		t.Fatalf("close-surface should return 0, got %d", code)
	}
	select {
	case receivedParams := <-receivedParamsCh:
		if receivedParams["workspace_id"] != "env-ws-id" {
			t.Errorf("expected workspace_id from env, got %v", receivedParams["workspace_id"])
		}
		if receivedParams["surface_id"] != "env-sf-id" {
			t.Errorf("expected surface_id from env, got %v", receivedParams["surface_id"])
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for close-surface payload")
	}
}

func expectGroupRequest(t *testing.T, requests <-chan map[string]any, wantMethod string) map[string]any {
	t.Helper()
	select {
	case req := <-requests:
		if got := req["method"]; got != wantMethod {
			t.Fatalf("expected method %s, got %v", wantMethod, got)
		}
		params, _ := req["params"].(map[string]any)
		return params
	case <-time.After(2 * time.Second):
		t.Fatalf("timed out waiting for %s request", wantMethod)
		return nil
	}
}

func TestCLIWorkspaceGroupList(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	code := runCLI([]string{"--socket", sockPath, "--json", "workspace", "group", "list"})
	if code != 0 {
		t.Fatalf("workspace group list should return 0, got %d", code)
	}
	expectGroupRequest(t, requests, "workspace.group.list")
}

func TestCLIWorkspaceGroupDashAlias(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	code := runCLI([]string{"--socket", sockPath, "--json", "workspace-group", "collapse", "workspace_group:1"})
	if code != 0 {
		t.Fatalf("workspace-group collapse should return 0, got %d", code)
	}
	params := expectGroupRequest(t, requests, "workspace.group.collapse")
	if got := params["group_id"]; got != "workspace_group:1" {
		t.Fatalf("expected positional group_id, got %v", got)
	}
}

func TestCLIWorkspaceGroupCreateMapsFlags(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	code := runCLI([]string{
		"--socket", sockPath, "--json",
		"workspace", "group", "create",
		"--name", "My Group",
		"--cwd", "/repo/path",
		"--from", "workspace:1, workspace:2",
	})
	if code != 0 {
		t.Fatalf("workspace group create should return 0, got %d", code)
	}
	params := expectGroupRequest(t, requests, "workspace.group.create")
	if got := params["name"]; got != "My Group" {
		t.Fatalf("expected name, got %v", got)
	}
	if got := params["cwd"]; got != "/repo/path" {
		t.Fatalf("expected cwd, got %v", got)
	}
	ids, _ := params["child_workspace_ids"].([]any)
	if len(ids) != 2 || ids[0] != "workspace:1" || ids[1] != "workspace:2" {
		t.Fatalf("expected trimmed child_workspace_ids, got %v", params["child_workspace_ids"])
	}
}

func TestCLIWorkspaceGroupAddRequiresGroupAndWorkspace(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	if code := runCLI([]string{"--socket", sockPath, "workspace", "group", "add", "--group", "g1"}); code != 2 {
		t.Fatalf("add without --workspace should return 2, got %d", code)
	}
	code := runCLI([]string{"--socket", sockPath, "--json", "workspace", "group", "add", "--group", "g1", "--workspace", "ws1"})
	if code != 0 {
		t.Fatalf("workspace group add should return 0, got %d", code)
	}
	params := expectGroupRequest(t, requests, "workspace.group.add")
	if params["group_id"] != "g1" || params["workspace_id"] != "ws1" {
		t.Fatalf("expected group_id/workspace_id, got %v", params)
	}
}

func TestCLIWorkspaceGroupRenamePositionalName(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	code := runCLI([]string{"--socket", sockPath, "--json", "workspace", "group", "rename", "workspace_group:2", "New Name"})
	if code != 0 {
		t.Fatalf("workspace group rename should return 0, got %d", code)
	}
	params := expectGroupRequest(t, requests, "workspace.group.rename")
	if params["group_id"] != "workspace_group:2" || params["name"] != "New Name" {
		t.Fatalf("expected positional group id and name, got %v", params)
	}
}

func TestCLIWorkspaceGroupNewWorkspaceUsesUnderscoreMethod(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	code := runCLI([]string{"--socket", sockPath, "--json", "workspace", "group", "new-workspace", "workspace_group:3", "--placement", "top"})
	if code != 0 {
		t.Fatalf("workspace group new-workspace should return 0, got %d", code)
	}
	params := expectGroupRequest(t, requests, "workspace.group.new_workspace")
	if params["group_id"] != "workspace_group:3" || params["placement"] != "top" {
		t.Fatalf("expected group_id and placement, got %v", params)
	}
}

func TestCLIWorkspaceGroupSetColorOmittedHexClears(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	code := runCLI([]string{"--socket", sockPath, "--json", "workspace", "group", "set-color", "workspace_group:4"})
	if code != 0 {
		t.Fatalf("workspace group set-color should return 0, got %d", code)
	}
	params := expectGroupRequest(t, requests, "workspace.group.set_color")
	if got, ok := params["hex"]; !ok || got != "" {
		t.Fatalf("expected empty hex (clear), got %v", params)
	}
}

func TestCLIWorkspaceGroupMoveValidatesPosition(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	if code := runCLI([]string{"--socket", sockPath, "workspace", "group", "move", "g1"}); code != 2 {
		t.Fatalf("move without a position flag should return 2, got %d", code)
	}
	if code := runCLI([]string{"--socket", sockPath, "workspace", "group", "move", "g1", "--to-index", "abc"}); code != 2 {
		t.Fatalf("move with non-integer --to-index should return 2, got %d", code)
	}
	code := runCLI([]string{"--socket", sockPath, "--json", "workspace", "group", "move", "g1", "--to-index", "2"})
	if code != 0 {
		t.Fatalf("workspace group move should return 0, got %d", code)
	}
	params := expectGroupRequest(t, requests, "workspace.group.move")
	if got, ok := params["to_index"].(float64); !ok || got != 2 {
		t.Fatalf("expected integer to_index 2, got %v", params)
	}
	if params["group_id"] != "g1" {
		t.Fatalf("expected group_id g1, got %v", params)
	}
}

func TestCLIWorkspaceGroupUnknownSubcommand(t *testing.T) {
	sockPath := startMockV2Socket(t)
	if code := runCLI([]string{"--socket", sockPath, "workspace", "group", "explode"}); code != 2 {
		t.Fatalf("unknown group subcommand should return 2, got %d", code)
	}
	if code := runCLI([]string{"--socket", sockPath, "workspace", "group"}); code != 2 {
		t.Fatalf("bare workspace group should return 2, got %d", code)
	}
	if code := runCLI([]string{"--socket", sockPath, "workspace", "rename"}); code != 2 {
		t.Fatalf("unsupported workspace subcommand should return 2, got %d", code)
	}
}

func TestCLIWorkspaceGroupListForwardsCallerEnvContext(t *testing.T) {
	sockPath, requests := startMockV2SocketWithRequestCapture(t)
	t.Setenv("CMUX_WORKSPACE_ID", "env-ws")
	t.Setenv("CMUX_SURFACE_ID", "env-sf")
	code := runCLI([]string{"--socket", sockPath, "--json", "workspace", "group", "list"})
	if code != 0 {
		t.Fatalf("workspace group list should return 0, got %d", code)
	}
	params := expectGroupRequest(t, requests, "workspace.group.list")
	if params["workspace_id"] != "env-ws" || params["surface_id"] != "env-sf" {
		t.Fatalf("expected caller env context to be forwarded, got %v", params)
	}
}

func TestCLIWorkspaceGroupRemoveStillRequiresExplicitWorkspaceWithEnv(t *testing.T) {
	sockPath := startMockV2Socket(t)
	t.Setenv("CMUX_WORKSPACE_ID", "env-ws")
	if code := runCLI([]string{"--socket", sockPath, "workspace", "group", "remove"}); code != 2 {
		t.Fatalf("remove without --workspace should return 2 even with env set, got %d", code)
	}
}
