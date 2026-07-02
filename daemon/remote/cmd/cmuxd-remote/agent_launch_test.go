//go:build !windows

// This launcher-shim test exercises the Unix tmux/opencode integration
// (bun/npm PATH probe + XDG ~/.config/opencode) and asserts Unix-oriented
// launcher behavior, so it is gated off Windows. omoEnsurePlugin on Windows
// short-circuits on the bun/npm PATH probe before it ever reaches the
// invalid-JSON path this test asserts.

package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestOmoEnsurePluginInvalidJSONErrorDoesNotExposeUserPath(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	userDir := filepath.Join(home, ".config", "opencode")
	if err := os.MkdirAll(userDir, 0755); err != nil {
		t.Fatalf("failed to create user config dir: %v", err)
	}
	userJSONPath := filepath.Join(userDir, "opencode.json")
	if err := os.WriteFile(userJSONPath, []byte("{"), 0644); err != nil {
		t.Fatalf("failed to write invalid config: %v", err)
	}

	err := omoEnsurePlugin(os.Getenv("PATH"))
	if err == nil {
		t.Fatal("omoEnsurePlugin returned nil for invalid opencode.json")
	}

	msg := err.Error()
	if strings.Contains(msg, home) || strings.Contains(msg, userJSONPath) {
		t.Fatalf("error %q exposes user config path %q", msg, userJSONPath)
	}
	if !strings.Contains(msg, "invalid opencode.json") {
		t.Fatalf("error = %q, want generic invalid opencode.json message", msg)
	}
}
