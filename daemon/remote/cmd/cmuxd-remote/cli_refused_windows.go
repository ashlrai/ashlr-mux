//go:build windows

package main

import (
	"errors"

	"golang.org/x/sys/windows"
)

// isRefusedErrno reports whether err is (or wraps) a connection-refused errno.
// On Windows a refused connect surfaces as the winsock errno WSAECONNREFUSED
// (10061), NOT syscall.ECONNREFUSED (which is a distinct synthetic value here),
// so the match must target windows.WSAECONNREFUSED to fire on a refused connect.
func isRefusedErrno(err error) bool {
	return errors.Is(err, windows.WSAECONNREFUSED)
}
