//go:build windows

package main

import (
	"context"
	"encoding/base64"
	"errors"
	"io"
	"time"
)

type wsPTYServerConfig struct {
	ListenAddr       string
	PTYAuthLeaseFile string
	RPCAuthLeaseFile string
	Shell            string
	PTYHub           *wsPTYHub
	ScrollbackLimit  int
	SessionIdleTTL   time.Duration
}

type wsPTYOutgoingFrame struct {
	payload []byte
}

type wsPTYInputWriteStatus uint8

const (
	wsPTYInputWriteOK wsPTYInputWriteStatus = iota
	wsPTYInputWriteNotFound
	wsPTYInputWriteQueueFull
)

type wsPTYSessionKind uint8

type wsPTYSessionKey struct {
	kind        wsPTYSessionKind
	sessionID   string
	anonymousID uint64
}

type wsPTYAttachment struct {
	sessionKey  wsPTYSessionKey
	id          string
	clientToken string
	send        chan wsPTYOutgoingFrame
}

type wsPTYHub struct{}

type wsPTYEventFrame struct {
	Type         string `json:"type"`
	SessionID    string `json:"session_id,omitempty"`
	AttachmentID string `json:"attachment_id,omitempty"`
	Message      string `json:"message,omitempty"`
}

func newWebSocketPTYHub(_ wsPTYServerConfig, _ io.Writer) *wsPTYHub {
	return &wsPTYHub{}
}

func runWebSocketPTYServer(_ context.Context, _ wsPTYServerConfig, _ io.Writer) error {
	return errors.New("websocket PTY transport is not implemented on Windows in M0")
}

func (h *wsPTYHub) attachRPC(
	_ context.Context,
	sessionID string,
	attachmentID string,
	_ int,
	_ int,
	_ string,
	attachmentToken string,
	_ bool,
) (*wsPTYAttachment, context.Context, <-chan struct{}, error) {
	return &wsPTYAttachment{
		sessionKey:  wsPTYSessionKey{sessionID: sessionID},
		id:          attachmentID,
		clientToken: attachmentToken,
		send:        make(chan wsPTYOutgoingFrame),
	}, context.Background(), make(chan struct{}), errors.New("PTY attach is not implemented on Windows in M0")
}

func (h *wsPTYHub) closeAll() {}

func (h *wsPTYHub) activeSessionCount() int { return 0 }

func (h *wsPTYHub) dropAttachment(_ *wsPTYAttachment) {}

func (h *wsPTYHub) writeInputByID(_ string, _ string, _ string, _ []byte) wsPTYInputWriteStatus {
	return wsPTYInputWriteNotFound
}

func (h *wsPTYHub) resizeByID(_ string, _ string, _ string, _ int, _ int) bool {
	return false
}

func (h *wsPTYHub) detachByID(_ string, _ string, _ string) bool {
	return false
}

func (h *wsPTYHub) closeSessionByID(_ string) bool {
	return false
}

func (h *wsPTYHub) sessionSnapshots() []map[string]any {
	return []map[string]any{}
}

func rpcPTYEventForFrame(attachment *wsPTYAttachment, frame wsPTYOutgoingFrame) rpcEvent {
	return rpcEvent{
		Event:           "pty.data",
		SessionID:       attachment.sessionKey.sessionID,
		AttachmentID:    attachment.id,
		AttachmentToken: attachment.clientToken,
		DataBase64:      base64.StdEncoding.EncodeToString(frame.payload),
	}
}

func rpcPTYExitEvent(attachment *wsPTYAttachment) rpcEvent {
	return rpcEvent{
		Event:           "pty.exit",
		SessionID:       attachment.sessionKey.sessionID,
		AttachmentID:    attachment.id,
		AttachmentToken: attachment.clientToken,
	}
}

func (a *wsPTYAttachment) closeNow() {}
