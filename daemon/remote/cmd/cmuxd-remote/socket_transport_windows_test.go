//go:build windows

package main

import (
	"strings"
	"testing"
)

// TestControlPipeNameDerivation pins the AF_UNIX-path -> named-pipe mapping:
// only the basename's hex digest leaks into the pipe name (never the /tmp
// directory), the .sock suffix is dropped, and an already-pipe address passes
// through verbatim. This keeps per-slot isolation and parity with the Rust
// \\.\pipe\cmux-<id> scheme (the daemon uses the cmuxd- prefix).
func TestControlPipeNameDerivation(t *testing.T) {
	cases := []struct {
		name string
		addr string
		want string
	}{
		{
			name: "tmp socket path strips dir and suffix",
			addr: `/tmp/cmuxd-remote-501/cmuxd-0123456789abcdef.sock`,
			want: `\\.\pipe\cmuxd-0123456789abcdef`,
		},
		{
			name: "windows-style dir is stripped too",
			addr: `C:\Users\me\.cmux\daemon\dev\slot\cmuxd-deadbeefcafef00d.sock`,
			want: `\\.\pipe\cmuxd-deadbeefcafef00d`,
		},
		{
			name: "bare basename without suffix",
			addr: "cmuxd-0123456789abcdef",
			want: `\\.\pipe\cmuxd-0123456789abcdef`,
		},
		{
			name: "already a pipe path passes through",
			addr: `\\.\pipe\cmuxd-0123456789abcdef`,
			want: `\\.\pipe\cmuxd-0123456789abcdef`,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := controlPipeName(tc.addr); got != tc.want {
				t.Fatalf("controlPipeName(%q) = %q, want %q", tc.addr, got, tc.want)
			}
		})
	}
}

// TestCurrentUserSID asserts the process-token SID is well formed, matching the
// Rust current_user_sid check.
func TestCurrentUserSID(t *testing.T) {
	sid, err := currentUserSID()
	if err != nil {
		t.Fatalf("currentUserSID: %v", err)
	}
	if !strings.HasPrefix(sid, "S-1-") {
		t.Fatalf("unexpected SID form: %q", sid)
	}
}

// TestCurrentUserOnlySDDL asserts the DACL grants GENERIC_ALL to exactly the
// current user, protected from inheritance (D:P(A;;GA;;;<sid>)) — least
// privilege, mirroring named_pipe.rs's user_only_sddl.
func TestCurrentUserOnlySDDL(t *testing.T) {
	sddl, err := currentUserOnlySDDL()
	if err != nil {
		t.Fatalf("currentUserOnlySDDL: %v", err)
	}
	sid, err := currentUserSID()
	if err != nil {
		t.Fatalf("currentUserSID: %v", err)
	}
	want := "D:P(A;;GA;;;" + sid + ")"
	if sddl != want {
		t.Fatalf("currentUserOnlySDDL() = %q, want %q", sddl, want)
	}
}
