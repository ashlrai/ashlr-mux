//go:build windows

package main

// Windows control-socket transport. macOS/Linux expose the per-slot daemon
// control channel as an AF_UNIX socket; Windows has no Unix-domain analogue
// suitable here, so this binds a named pipe instead, mirroring the Rust
// cmux-ipc named_pipe.rs scheme (\\.\pipe\cmux-<id>). The daemon keeps a
// distinct "cmuxd-" prefix so its control pipe never collides with the app's
// "cmux-" control pipe.
//
// The wire protocol above the transport (v2 NDJSON framing, the daemon auth
// token handshake) is unchanged: each accepted connection is a plain net.Conn.
//
// Security: the pipe is created with an explicit current-user-only DACL
// (D:P(A;;GA;;;<sid>)) derived from the process token, not go-winio's default
// descriptor. This matches named_pipe.rs's least-privilege intent — GENERIC_ALL
// to exactly the owning SID, SYSTEM/Administrators omitted, no inheritance.

import (
	"errors"
	"net"
	"path/filepath"
	"strings"
	"time"

	"github.com/Microsoft/go-winio"
	"golang.org/x/sys/windows"
)

// pipeNamespace is the \\.\pipe\ prefix for local named pipes.
const pipeNamespace = `\\.\pipe\`

// dialAttemptFloor is the minimum per-call budget handed to winio.DialPipe so a
// zero-timeout dial still performs one real CreateFile attempt: winio checks
// ctx cancellation before each attempt, so an already-expired deadline would
// otherwise make zero attempts.
const dialAttemptFloor = 10 * time.Millisecond

// controlPipeName derives the named-pipe path for a control-socket address. The
// daemon's address plumbing produces AF_UNIX-style paths ("…/cmuxd-<hex16>.sock")
// on every platform; here only the basename's hex digest matters (the /tmp
// directory must never leak into the pipe name), so we strip the directory and
// the .sock suffix and prefix the pipe namespace. An address that is already a
// \\.\pipe\ path is used verbatim so callers can dial a pipe directly.
func controlPipeName(addr string) string {
	if strings.HasPrefix(addr, pipeNamespace) {
		return addr
	}
	base := strings.TrimSuffix(filepath.Base(addr), ".sock")
	return pipeNamespace + base
}

// currentUserSID returns the string SID (e.g. S-1-5-21-…) owning the current
// process token, used to build the pipe DACL.
func currentUserSID() (string, error) {
	token, err := windows.OpenCurrentProcessToken()
	if err != nil {
		return "", err
	}
	defer token.Close()
	user, err := token.GetTokenUser()
	if err != nil {
		return "", err
	}
	return user.User.Sid.String(), nil
}

// currentUserOnlySDDL builds the SDDL for a DACL granting GENERIC_ALL to the
// current user alone, protected from inheritance — the same shape as
// named_pipe.rs's user_only_sddl.
func currentUserOnlySDDL() (string, error) {
	sid, err := currentUserSID()
	if err != nil {
		return "", err
	}
	return "D:P(A;;GA;;;" + sid + ")", nil
}

// listenControlSocket binds the per-slot control pipe with a current-user-only
// DACL. Unlike the Unix variant there is no filesystem entry to pre-remove or
// chmod; go-winio's ListenPipe creates the first instance with
// FILE_FLAG_FIRST_PIPE_INSTANCE (squat protection) and serves further instances
// from its listener routine.
func listenControlSocket(addr string) (net.Listener, error) {
	sddl, err := currentUserOnlySDDL()
	if err != nil {
		return nil, err
	}
	return winio.ListenPipe(controlPipeName(addr), &winio.PipeConfig{SecurityDescriptor: sddl})
}

// dialControlSocket connects to the control pipe at addr, retrying transient
// failures until timeout elapses, mirroring connect_pipe() in named_pipe.rs.
// go-winio handles ERROR_PIPE_BUSY internally (the server momentarily has no
// free instance); we additionally retry ERROR_FILE_NOT_FOUND (the server's
// first instance is not listening yet, e.g. the daemon is still launching) on a
// 25ms cadence. A zero timeout collapses to a single attempt.
func dialControlSocket(addr string, timeout time.Duration) (net.Conn, error) {
	pipe := controlPipeName(addr)
	deadline := time.Now().Add(timeout)
	for {
		budget := time.Until(deadline)
		if budget < dialAttemptFloor {
			budget = dialAttemptFloor
		}
		conn, err := winio.DialPipe(pipe, &budget)
		if err == nil {
			return conn, nil
		}
		if !errors.Is(err, windows.ERROR_FILE_NOT_FOUND) || !time.Now().Before(deadline) {
			return nil, err
		}
		time.Sleep(25 * time.Millisecond)
	}
}

// halfCloseWrite signals "no more input" to the daemon by fully closing the
// pipe. go-winio byte-mode pipes have no AF_UNIX-style write half-close
// (CloseWrite is only meaningful for message-mode pipes), so we cannot signal
// stdin EOF while keeping the read half open. The sole caller is the persistent
// stdio proxy (an SSH ProxyCommand bridge): there, stdin EOF means the remote
// channel is gone, so a full close is the correct teardown — it unblocks the
// daemon's blocked frame reader (which would otherwise never see EOF and leave
// the proxy's stdout copy hung) instead of leaking the connection. The proxy's
// own deferred conn.Close makes this double close a harmless no-op, and the
// resulting ErrClosed/EPIPE on the stdout copy is already treated as a clean
// drain by persistentProxyCopyError.
func halfCloseWrite(conn net.Conn) error {
	return conn.Close()
}
