//go:build !windows

package main

import (
	"net"
	"os"
	"time"
)

// listenControlSocket binds the per-slot control socket. On Unix this is an
// AF_UNIX listener at addr; the pre-remove of a stale socket file and the
// 0o600 chmod preserve the exact housekeeping the daemon performed inline
// before the transport seam was introduced. The caller is responsible for
// closing the listener and removing the socket file on shutdown (the post-
// remove defer stays at the call site so it can be a no-op on Windows).
func listenControlSocket(addr string) (net.Listener, error) {
	_ = os.Remove(addr)
	listener, err := net.Listen("unix", addr)
	if err != nil {
		return nil, err
	}
	_ = os.Chmod(addr, 0o600)
	return listener, nil
}

// dialControlSocket connects to the control socket at addr. A zero timeout maps
// to a single blocking net.Dial (matching the in-app CLI's net.Dial("unix")),
// otherwise net.DialTimeout bounds the attempt.
func dialControlSocket(addr string, timeout time.Duration) (net.Conn, error) {
	if timeout <= 0 {
		return net.Dial("unix", addr)
	}
	return net.DialTimeout("unix", addr, timeout)
}

// halfCloseWrite signals stdin EOF to the daemon while leaving the read half
// open. On Unix the control socket is an *net.UnixConn, which supports a real
// write half-close via CloseWrite.
func halfCloseWrite(conn net.Conn) error {
	if unixConn, ok := conn.(*net.UnixConn); ok {
		return unixConn.CloseWrite()
	}
	return nil
}
