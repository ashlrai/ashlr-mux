//go:build !windows

package main

import (
	"errors"
	"syscall"
)

// isRefusedErrno reports whether err is (or wraps) a connection-refused errno.
// On unix the connect error carries syscall.ECONNREFUSED, so this preserves the
// original byte-identical unix behavior of isConnectionRefused.
func isRefusedErrno(err error) bool {
	return errors.Is(err, syscall.ECONNREFUSED)
}
