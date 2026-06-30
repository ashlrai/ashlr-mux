package main

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"testing"
	"time"
)

// TestControlSocketRoundTrip exercises the platform transport seam end to end:
// listenControlSocket binds, dialControlSocket connects to the same addr, and a
// v2 JSON-RPC frame written by the client is echoed back by the server. It runs
// on whatever OS CI executes — an AF_UNIX socket on Unix, a named pipe on
// Windows — proving listen and dial agree on the address mapping.
func TestControlSocketRoundTrip(t *testing.T) {
	addr := filepath.Join(t.TempDir(), fmt.Sprintf("cmuxd-%d.sock", os.Getpid()))

	listener, err := listenControlSocket(addr)
	if err != nil {
		t.Fatalf("listenControlSocket(%q): %v", addr, err)
	}
	defer listener.Close()

	const frame = `{"id":1,"method":"ping"}` + "\n"

	serverErr := make(chan error, 1)
	go func() {
		conn, err := listener.Accept()
		if err != nil {
			serverErr <- fmt.Errorf("accept: %w", err)
			return
		}
		defer conn.Close()
		buf := make([]byte, len(frame))
		if _, err := io.ReadFull(conn, buf); err != nil {
			serverErr <- fmt.Errorf("read: %w", err)
			return
		}
		if _, err := conn.Write(buf); err != nil {
			serverErr <- fmt.Errorf("write: %w", err)
			return
		}
		serverErr <- nil
	}()

	conn, err := dialControlSocket(addr, 2*time.Second)
	if err != nil {
		t.Fatalf("dialControlSocket(%q): %v", addr, err)
	}
	defer conn.Close()

	if _, err := conn.Write([]byte(frame)); err != nil {
		t.Fatalf("client write: %v", err)
	}
	got := make([]byte, len(frame))
	if _, err := io.ReadFull(conn, got); err != nil {
		t.Fatalf("client read: %v", err)
	}
	if string(got) != frame {
		t.Fatalf("round-trip mismatch: got %q want %q", got, frame)
	}

	if err := <-serverErr; err != nil {
		t.Fatalf("server side: %v", err)
	}
}

// TestHalfCloseWrite verifies halfCloseWrite returns without error on a live
// control connection. On Unix this performs a real *net.UnixConn.CloseWrite; on
// Windows it is the documented best-effort no-op. Either way the proxy must be
// able to call it safely.
func TestHalfCloseWrite(t *testing.T) {
	addr := filepath.Join(t.TempDir(), fmt.Sprintf("cmuxd-hc-%d.sock", os.Getpid()))

	listener, err := listenControlSocket(addr)
	if err != nil {
		t.Fatalf("listenControlSocket: %v", err)
	}
	defer listener.Close()

	accepted := make(chan struct{}, 1)
	go func() {
		conn, err := listener.Accept()
		if err == nil {
			defer conn.Close()
		}
		accepted <- struct{}{}
	}()

	conn, err := dialControlSocket(addr, 2*time.Second)
	if err != nil {
		t.Fatalf("dialControlSocket: %v", err)
	}
	defer conn.Close()
	<-accepted

	if err := halfCloseWrite(conn); err != nil {
		t.Fatalf("halfCloseWrite: %v", err)
	}
}

// TestDialControlSocketMissingReturnsError ensures dialControlSocket surfaces an
// error (rather than hanging) for an address with no listener, honoring the
// supplied timeout as an upper bound.
func TestDialControlSocketMissingReturnsError(t *testing.T) {
	addr := filepath.Join(t.TempDir(), fmt.Sprintf("cmuxd-absent-%d.sock", os.Getpid()))

	start := time.Now()
	conn, err := dialControlSocket(addr, 200*time.Millisecond)
	elapsed := time.Since(start)
	if err == nil {
		_ = conn.Close()
		t.Fatalf("dialControlSocket to absent addr unexpectedly succeeded")
	}
	if elapsed > 5*time.Second {
		t.Fatalf("dialControlSocket took %v, expected it to give up promptly", elapsed)
	}
}
