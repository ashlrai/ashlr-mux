//go:build !windows

package main

import (
	"bufio"
	"encoding/json"
	"net"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
	"time"
)

// TestCreateTmuxShimDir asserts the Unix executable bit (Mode()&0111) on the
// generated tmux shim. Windows has no exec-bit, so this is gated to !windows.
func TestCreateTmuxShimDir(t *testing.T) {
	tmpDir := t.TempDir()
	origHome := os.Getenv("HOME")
	os.Setenv("HOME", tmpDir)
	defer os.Setenv("HOME", origHome)

	dir, err := createTmuxShimDir("test-shim-bin", claudeTeamsShimScript)
	if err != nil {
		t.Fatalf("createTmuxShimDir: %v", err)
	}
	tmuxPath := filepath.Join(dir, "tmux")
	info, err := os.Stat(tmuxPath)
	if err != nil {
		t.Fatalf("tmux shim not found: %v", err)
	}
	if info.Mode()&0111 == 0 {
		t.Error("tmux shim is not executable")
	}
	content, _ := os.ReadFile(tmuxPath)
	if !strings.Contains(string(content), "__tmux-compat") {
		t.Error("shim script should reference __tmux-compat")
	}
}

func TestTmuxDisplayReporterFormatFields(t *testing.T) {
	origHome := os.Getenv("HOME")
	origWorkspace := os.Getenv("CMUX_WORKSPACE_ID")
	origSurface := os.Getenv("CMUX_SURFACE_ID")
	origPane := os.Getenv("TMUX_PANE")
	os.Setenv("HOME", t.TempDir())
	os.Setenv("CMUX_WORKSPACE_ID", "workspace:1")
	os.Setenv("CMUX_SURFACE_ID", "surface:1")
	leaderPaneToken := "%" + tmuxStableNumericId("33333333-3333-4333-8333-333333333333")
	os.Setenv("TMUX_PANE", leaderPaneToken)
	defer func() {
		os.Setenv("HOME", origHome)
		if origWorkspace != "" {
			os.Setenv("CMUX_WORKSPACE_ID", origWorkspace)
		} else {
			os.Unsetenv("CMUX_WORKSPACE_ID")
		}
		if origSurface != "" {
			os.Setenv("CMUX_SURFACE_ID", origSurface)
		} else {
			os.Unsetenv("CMUX_SURFACE_ID")
		}
		if origPane != "" {
			os.Setenv("TMUX_PANE", origPane)
		} else {
			os.Unsetenv("TMUX_PANE")
		}
	}()

	sockPath := startMockTmuxCompatSocket(t)
	rc := &rpcContext{socketPath: sockPath}
	fields := []string{
		"session_id",
		"session_name",
		"window_index",
		"window_id",
		"pane_id",
		"pane_width",
		"pane_height",
		"window_width",
		"window_height",
		"pane_current_path",
		"pane_active",
		"window_active",
		"session_attached",
	}
	parts := make([]string, 0, len(fields))
	for _, field := range fields {
		parts = append(parts, field+"=#{"+field+"}")
	}

	output := captureStdout(t, func() {
		if err := dispatchTmuxCommand(rc, "display-message", []string{
			"-p",
			"-F", strings.Join(parts, "\t"),
			"-t", leaderPaneToken,
		}); err != nil {
			t.Fatalf("display-message: %v", err)
		}
	})

	values := map[string]string{}
	for _, part := range strings.Split(strings.TrimSpace(output), "\t") {
		key, value, ok := strings.Cut(part, "=")
		if !ok {
			t.Fatalf("malformed field %q in output %q", part, output)
		}
		values[key] = value
	}
	for _, field := range fields {
		if _, ok := values[field]; !ok {
			t.Fatalf("missing field %q in output %q", field, output)
		}
	}

	assertTmuxFieldMatch(t, values["session_id"], `^\$[0-9]+$`, "session_id")
	if values["session_name"] != "cmux" {
		t.Fatalf("session_name = %q, want cmux", values["session_name"])
	}
	assertTmuxFieldMatch(t, values["window_index"], `^[0-9]+$`, "window_index")
	assertTmuxFieldMatch(t, values["window_id"], `^@[0-9]+$`, "window_id")
	assertTmuxFieldMatch(t, values["pane_id"], `^%[0-9]+$`, "pane_id")
	assertTmuxFieldMatch(t, values["pane_width"], `^[0-9]+$`, "pane_width")
	assertTmuxFieldMatch(t, values["pane_height"], `^[0-9]+$`, "pane_height")
	assertTmuxFieldMatch(t, values["window_width"], `^[0-9]+$`, "window_width")
	assertTmuxFieldMatch(t, values["window_height"], `^[0-9]+$`, "window_height")
	if !filepath.IsAbs(values["pane_current_path"]) {
		t.Fatalf("pane_current_path = %q, want an absolute path", values["pane_current_path"])
	}
	if values["pane_active"] != "1" {
		t.Fatalf("pane_active = %q, want 1 for stringy focused metadata", values["pane_active"])
	}
	assertTmuxFieldMatch(t, values["pane_active"], `^[01]$`, "pane_active")
	assertTmuxFieldMatch(t, values["window_active"], `^[01]$`, "window_active")
	assertTmuxFieldMatch(t, values["session_attached"], `^[01]$`, "session_attached")
}

func assertTmuxFieldMatch(t *testing.T, got string, pattern string, field string) {
	t.Helper()
	if !regexp.MustCompile(pattern).MatchString(got) {
		t.Fatalf("%s = %q, want match %s", field, got, pattern)
	}
}

func TestGetFocusedContextCanonicalizesPaneRef(t *testing.T) {
	sockPath := startMockTmuxCompatSocket(t)
	rc := &rpcContext{socketPath: sockPath}

	focused := getFocusedContext(rc)
	if focused == nil {
		t.Fatal("getFocusedContext returned nil")
	}
	if focused.paneHandle != "pane:1" {
		t.Fatalf("paneHandle = %q, want pane:1", focused.paneHandle)
	}
	if focused.paneId != "33333333-3333-4333-8333-333333333333" {
		t.Fatalf("paneId = %q, want canonical pane UUID", focused.paneId)
	}
}

func TestGetFocusedContextKeepsBaseContextWhenCanonicalizationTimesOut(t *testing.T) {
	sockPath := startSlowFocusedCanonicalizationSocket(t, 200*time.Millisecond)
	rc := &rpcContext{socketPath: sockPath}

	focused := getFocusedContextWithTimeout(rc, 50*time.Millisecond)
	if focused == nil {
		t.Fatal("getFocusedContextWithTimeout returned nil")
	}
	if focused.workspaceId != "11111111-1111-4111-8111-111111111111" {
		t.Fatalf("workspaceId = %q", focused.workspaceId)
	}
	if focused.paneHandle != "pane:1" {
		t.Fatalf("paneHandle = %q, want pane:1", focused.paneHandle)
	}
	if focused.paneId != "pane:1" {
		t.Fatalf("paneId = %q, want base pane id when canonicalization times out", focused.paneId)
	}
}

func TestTmuxSigiledSelectorsSkipRefsAndIndexes(t *testing.T) {
	sockPath := startMockTmuxCompatSocket(t)
	rc := &rpcContext{socketPath: sockPath}
	workspaceId := "11111111-1111-4111-8111-111111111111"
	paneId := "33333333-3333-4333-8333-333333333333"

	if got, err := tmuxResolveWorkspaceId(rc, "1"); err != nil || got != workspaceId {
		t.Fatalf("unsigiled workspace index resolved to %q, %v; want %s", got, err, workspaceId)
	}
	if got, err := tmuxCanonicalPaneId(rc, "1", workspaceId); err != nil || got != paneId {
		t.Fatalf("unsigiled pane index resolved to %q, %v; want %s", got, err, paneId)
	}
	if _, err := tmuxResolveWorkspaceId(rc, "$1"); err == nil {
		t.Fatal("sigiled workspace selector $1 resolved by index; want no match")
	}
	if _, err := tmuxCanonicalPaneId(rc, "%1", workspaceId); err == nil {
		t.Fatal("sigiled pane selector %1 resolved by index; want no match")
	}
	if got, err := tmuxResolveWorkspaceId(rc, "$"+tmuxStableNumericId(workspaceId)); err != nil || got != workspaceId {
		t.Fatalf("sigiled workspace numeric id resolved to %q, %v; want %s", got, err, workspaceId)
	}
	if got, err := tmuxCanonicalPaneId(rc, "%"+tmuxStableNumericId(paneId), workspaceId); err != nil || got != paneId {
		t.Fatalf("sigiled pane numeric id resolved to %q, %v; want %s", got, err, paneId)
	}
}

func TestTmuxCanonicalSelectorsPreferRefsBeforeIndexFallback(t *testing.T) {
	sockPath := startMockTmuxSelectorPrioritySocket(t)
	rc := &rpcContext{socketPath: sockPath}
	workspaceId := "11111111-1111-4111-8111-111111111111"
	refPaneId := "33333333-3333-4333-8333-333333333333"
	refSurfaceId := "55555555-5555-4555-8555-555555555555"

	if got, err := tmuxCanonicalPaneId(rc, "1", workspaceId); err != nil || got != refPaneId {
		t.Fatalf("pane selector resolved to %q, %v; want ref match %s before index fallback", got, err, refPaneId)
	}
	if got, err := tmuxCanonicalSurfaceId(rc, "1", workspaceId); err != nil || got != refSurfaceId {
		t.Fatalf("surface selector resolved to %q, %v; want ref match %s before index fallback", got, err, refSurfaceId)
	}
}

func startMockTmuxSelectorPrioritySocket(t *testing.T) string {
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
			go func(conn net.Conn) {
				defer conn.Close()
				reader := bufio.NewReader(conn)
				line, err := reader.ReadBytes('\n')
				if err != nil {
					return
				}

				var req map[string]any
				if err := json.Unmarshal(line, &req); err != nil {
					_, _ = conn.Write([]byte(`{"ok":false,"error":{"code":"parse","message":"bad json"}}` + "\n"))
					return
				}

				method, _ := req["method"].(string)
				resp := map[string]any{
					"id": req["id"],
					"ok": true,
				}
				switch method {
				case "pane.list":
					resp["result"] = map[string]any{
						"panes": []map[string]any{
							{"id": "22222222-2222-4222-8222-222222222222", "ref": "pane:index", "index": 1},
							{"id": "33333333-3333-4333-8333-333333333333", "ref": "1", "index": 2},
						},
					}
				case "surface.list":
					resp["result"] = map[string]any{
						"surfaces": []map[string]any{
							{"id": "44444444-4444-4444-8444-444444444444", "ref": "surface:index", "index": 1},
							{"id": "55555555-5555-4555-8555-555555555555", "ref": "1", "index": 2},
						},
					}
				default:
					resp["result"] = map[string]any{}
				}

				data, _ := json.Marshal(resp)
				_, _ = conn.Write(append(data, '\n'))
			}(conn)
		}
	}()

	return sockPath
}

func startSlowFocusedCanonicalizationSocket(t *testing.T, delay time.Duration) string {
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
			go func(conn net.Conn) {
				defer conn.Close()
				reader := bufio.NewReader(conn)
				line, err := reader.ReadBytes('\n')
				if err != nil {
					return
				}

				var req map[string]any
				if err := json.Unmarshal(line, &req); err != nil {
					_, _ = conn.Write([]byte(`{"ok":false,"error":{"code":"parse","message":"bad json"}}` + "\n"))
					return
				}

				method, _ := req["method"].(string)
				resp := map[string]any{
					"id": req["id"],
					"ok": true,
				}
				switch method {
				case "system.identify":
					resp["result"] = map[string]any{
						"focused": map[string]any{
							"workspace_id": "11111111-1111-4111-8111-111111111111",
							"pane_id":      "pane:1",
							"pane_ref":     "pane:1",
							"surface_ref":  "surface:1",
						},
					}
				case "pane.list":
					time.Sleep(delay)
					resp["result"] = map[string]any{
						"panes": []map[string]any{{
							"id":    "33333333-3333-4333-8333-333333333333",
							"ref":   "pane:1",
							"index": 1,
						}},
					}
				default:
					resp["result"] = map[string]any{}
				}

				data, _ := json.Marshal(resp)
				_, _ = conn.Write(append(data, '\n'))
			}(conn)
		}
	}()

	return sockPath
}
