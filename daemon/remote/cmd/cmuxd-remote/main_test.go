package main

import (
	"bufio"
	"bytes"
	"encoding/base64"
	"encoding/json"
	"io"
	"math"
	"net"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"
)

type notifyingBuffer struct {
	mu     sync.Mutex
	buffer bytes.Buffer
	notify chan struct{}
}

func newNotifyingBuffer() *notifyingBuffer {
	return &notifyingBuffer{notify: make(chan struct{}, 1)}
}

func (b *notifyingBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	n, err := b.buffer.Write(p)
	if n > 0 {
		select {
		case b.notify <- struct{}{}:
		default:
		}
	}
	return n, err
}

func (b *notifyingBuffer) String() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buffer.String()
}

type eofWithPayloadConn struct {
	payload  []byte
	readOnce bool
}

func (c *eofWithPayloadConn) Read(p []byte) (int, error) {
	if c.readOnce {
		return 0, io.EOF
	}
	c.readOnce = true
	n := copy(p, c.payload)
	return n, io.EOF
}

func (c *eofWithPayloadConn) Write(p []byte) (int, error) {
	return len(p), nil
}

func (c *eofWithPayloadConn) Close() error { return nil }
func (c *eofWithPayloadConn) LocalAddr() net.Addr {
	return &net.TCPAddr{IP: net.IPv4(127, 0, 0, 1), Port: 0}
}
func (c *eofWithPayloadConn) RemoteAddr() net.Addr {
	return &net.TCPAddr{IP: net.IPv4(127, 0, 0, 1), Port: 0}
}
func (c *eofWithPayloadConn) SetDeadline(time.Time) error      { return nil }
func (c *eofWithPayloadConn) SetReadDeadline(time.Time) error  { return nil }
func (c *eofWithPayloadConn) SetWriteDeadline(time.Time) error { return nil }

func TestRunVersion(t *testing.T) {
	var out bytes.Buffer
	code := run([]string{"version"}, strings.NewReader(""), &out, &bytes.Buffer{})
	if code != 0 {
		t.Fatalf("run version exit code = %d, want 0", code)
	}
	if strings.TrimSpace(out.String()) == "" {
		t.Fatalf("version output should not be empty")
	}
}

func TestRunStdioHelloAndPing(t *testing.T) {
	input := strings.NewReader(
		`{"id":1,"method":"hello","params":{}}` + "\n" +
			`{"id":2,"method":"ping","params":{}}` + "\n",
	)
	var out bytes.Buffer
	code := run([]string{"serve", "--stdio"}, input, &out, &bytes.Buffer{})
	if code != 0 {
		t.Fatalf("run serve exit code = %d, want 0", code)
	}

	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 2 {
		t.Fatalf("got %d response lines, want 2: %q", len(lines), out.String())
	}

	var first map[string]any
	if err := json.Unmarshal([]byte(lines[0]), &first); err != nil {
		t.Fatalf("failed to decode first response: %v", err)
	}
	if ok, _ := first["ok"].(bool); !ok {
		t.Fatalf("first response should be ok=true: %v", first)
	}
	firstResult, _ := first["result"].(map[string]any)
	if firstResult == nil {
		t.Fatalf("first response missing result object: %v", first)
	}
	capabilities, _ := firstResult["capabilities"].([]any)
	if len(capabilities) < 2 {
		t.Fatalf("hello should return capabilities: %v", firstResult)
	}
	var sawPushCapability bool
	for _, capability := range capabilities {
		if capability == "proxy.stream.push" {
			sawPushCapability = true
			break
		}
	}
	if !sawPushCapability {
		t.Fatalf("hello should advertise proxy.stream.push: %v", firstResult)
	}
	var sawPersistentPTYCapability bool
	for _, capability := range capabilities {
		if capability == "pty.session.persistent_daemon" {
			sawPersistentPTYCapability = true
			break
		}
	}
	if !sawPersistentPTYCapability {
		t.Fatalf("hello should advertise pty.session.persistent_daemon: %v", firstResult)
	}
	var sawPTYWriteNotificationCapability bool
	for _, capability := range capabilities {
		if capability == "pty.write.notification" {
			sawPTYWriteNotificationCapability = true
			break
		}
	}
	if !sawPTYWriteNotificationCapability {
		t.Fatalf("hello should advertise pty.write.notification: %v", firstResult)
	}
	var sawPTYResizeNotificationCapability bool
	for _, capability := range capabilities {
		if capability == "pty.resize.notification" {
			sawPTYResizeNotificationCapability = true
			break
		}
	}
	if !sawPTYResizeNotificationCapability {
		t.Fatalf("hello should advertise pty.resize.notification: %v", firstResult)
	}

	var second map[string]any
	if err := json.Unmarshal([]byte(lines[1]), &second); err != nil {
		t.Fatalf("failed to decode second response: %v", err)
	}
	if ok, _ := second["ok"].(bool); !ok {
		t.Fatalf("second response should be ok=true: %v", second)
	}
}

func TestRunStdioPTYWriteNotificationDoesNotEmitResponse(t *testing.T) {
	input := strings.NewReader(
		`{"method":"pty.write","params":{"session_id":"missing","attachment_id":"missing","client_attachment_token":"token","data_base64":"YQ=="}}` + "\n" +
			`{"id":2,"method":"ping","params":{}}` + "\n",
	)
	var out bytes.Buffer
	code := run([]string{"serve", "--stdio"}, input, &out, &bytes.Buffer{})
	if code != 0 {
		t.Fatalf("run serve exit code = %d, want 0", code)
	}

	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 2 {
		t.Fatalf("got %d frame lines, want pty.error event plus ping response: %q", len(lines), out.String())
	}

	var event map[string]any
	if err := json.Unmarshal([]byte(lines[0]), &event); err != nil {
		t.Fatalf("failed to decode pty.write error event: %v", err)
	}
	if _, hasID := event["id"]; hasID {
		t.Fatalf("pty.write notification should not emit an RPC response id: %v", event)
	}
	if got := event["event"]; got != "pty.error" {
		t.Fatalf("first frame = %v, want pty.error event; payload=%v", got, event)
	}

	var response map[string]any
	if err := json.Unmarshal([]byte(lines[1]), &response); err != nil {
		t.Fatalf("failed to decode ping response: %v", err)
	}
	if got := response["id"]; got != float64(2) {
		t.Fatalf("response id = %v, want ping id 2; payload=%v", got, response)
	}
	if ok, _ := response["ok"].(bool); !ok {
		t.Fatalf("ping response should be ok=true after pty.write notification: %v", response)
	}
}

func TestRunStdioPTYResizeNotificationDoesNotEmitResponse(t *testing.T) {
	input := strings.NewReader(
		`{"method":"pty.resize","params":{"session_id":"missing","attachment_id":"missing","client_attachment_token":"token","cols":100,"rows":30}}` + "\n" +
			`{"id":2,"method":"ping","params":{}}` + "\n",
	)
	var out bytes.Buffer
	code := run([]string{"serve", "--stdio"}, input, &out, &bytes.Buffer{})
	if code != 0 {
		t.Fatalf("run serve exit code = %d, want 0", code)
	}

	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 2 {
		t.Fatalf("got %d frame lines, want pty.error event plus ping response: %q", len(lines), out.String())
	}

	var event map[string]any
	if err := json.Unmarshal([]byte(lines[0]), &event); err != nil {
		t.Fatalf("failed to decode pty.resize error event: %v", err)
	}
	if _, hasID := event["id"]; hasID {
		t.Fatalf("pty.resize notification should not emit an RPC response id: %v", event)
	}
	if got := event["event"]; got != "pty.error" {
		t.Fatalf("first frame = %v, want pty.error event; payload=%v", got, event)
	}

	var response map[string]any
	if err := json.Unmarshal([]byte(lines[1]), &response); err != nil {
		t.Fatalf("failed to decode ping response: %v", err)
	}
	if got := response["id"]; got != float64(2) {
		t.Fatalf("response id = %v, want ping id 2; payload=%v", got, response)
	}
	if ok, _ := response["ok"].(bool); !ok {
		t.Fatalf("ping response should be ok=true after pty.resize notification: %v", response)
	}
}

func TestRunStdioNoIDNonPTYRequestStillEmitsResponse(t *testing.T) {
	input := strings.NewReader(`{"method":"ping","params":{}}` + "\n")
	var out bytes.Buffer
	code := run([]string{"serve", "--stdio"}, input, &out, &bytes.Buffer{})
	if code != 0 {
		t.Fatalf("run serve exit code = %d, want 0", code)
	}

	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 1 {
		t.Fatalf("got %d response lines, want ping response: %q", len(lines), out.String())
	}
	var response map[string]any
	if err := json.Unmarshal([]byte(lines[0]), &response); err != nil {
		t.Fatalf("failed to decode ping response: %v", err)
	}
	if ok, _ := response["ok"].(bool); !ok {
		t.Fatalf("no-id ping should still emit an ok response: %v", response)
	}
}

func TestRunStdioNullIDPTYWriteStillEmitsResponse(t *testing.T) {
	input := strings.NewReader(
		`{"id":null,"method":"pty.write","params":{"session_id":"missing","attachment_id":"missing","client_attachment_token":"token","data_base64":"YQ=="}}` + "\n" +
			`{"id":2,"method":"ping","params":{}}` + "\n",
	)
	var out bytes.Buffer
	code := run([]string{"serve", "--stdio"}, input, &out, &bytes.Buffer{})
	if code != 0 {
		t.Fatalf("run serve exit code = %d, want 0", code)
	}

	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 2 {
		t.Fatalf("got %d frame lines, want pty.write response plus ping response: %q", len(lines), out.String())
	}

	var response map[string]any
	if err := json.Unmarshal([]byte(lines[0]), &response); err != nil {
		t.Fatalf("failed to decode pty.write response: %v", err)
	}
	if eventName, _ := response["event"].(string); eventName != "" {
		t.Fatalf("id:null pty.write should emit an RPC response, got event: %v", response)
	}
	if ok, _ := response["ok"].(bool); ok {
		t.Fatalf("missing pty.write target should fail: %v", response)
	}

	var ping map[string]any
	if err := json.Unmarshal([]byte(lines[1]), &ping); err != nil {
		t.Fatalf("failed to decode ping response: %v", err)
	}
	if got := ping["id"]; got != float64(2) {
		t.Fatalf("response id = %v, want ping id 2; payload=%v", got, ping)
	}
	if ok, _ := ping["ok"].(bool); !ok {
		t.Fatalf("ping response should be ok=true after id:null pty.write: %v", ping)
	}
}

func TestRunStdioNullIDPTYResizeStillEmitsResponse(t *testing.T) {
	input := strings.NewReader(
		`{"id":null,"method":"pty.resize","params":{"session_id":"missing","attachment_id":"missing","client_attachment_token":"token","cols":100,"rows":30}}` + "\n" +
			`{"id":2,"method":"ping","params":{}}` + "\n",
	)
	var out bytes.Buffer
	code := run([]string{"serve", "--stdio"}, input, &out, &bytes.Buffer{})
	if code != 0 {
		t.Fatalf("run serve exit code = %d, want 0", code)
	}

	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 2 {
		t.Fatalf("got %d frame lines, want pty.resize response plus ping response: %q", len(lines), out.String())
	}

	var response map[string]any
	if err := json.Unmarshal([]byte(lines[0]), &response); err != nil {
		t.Fatalf("failed to decode pty.resize response: %v", err)
	}
	if eventName, _ := response["event"].(string); eventName != "" {
		t.Fatalf("id:null pty.resize should emit an RPC response, got event: %v", response)
	}
	if ok, _ := response["ok"].(bool); ok {
		t.Fatalf("missing pty.resize target should fail: %v", response)
	}

	var ping map[string]any
	if err := json.Unmarshal([]byte(lines[1]), &ping); err != nil {
		t.Fatalf("failed to decode ping response: %v", err)
	}
	if got := ping["id"]; got != float64(2) {
		t.Fatalf("response id = %v, want ping id 2; payload=%v", got, ping)
	}
	if ok, _ := ping["ok"].(bool); !ok {
		t.Fatalf("ping response should be ok=true after id:null pty.resize: %v", ping)
	}
}

func TestPersistentDaemonRejectsInvalidSlot(t *testing.T) {
	for _, slot := range []string{"", ".", "..", "../nope", "bad/slot", strings.Repeat("a", 129)} {
		if _, err := persistentDaemonPathsForSlot(slot); err == nil {
			t.Fatalf("persistentDaemonPathsForSlot(%q) succeeded, want error", slot)
		}
	}
}

func TestPersistentDaemonPathsUseShortSocketPath(t *testing.T) {
	rootBase := filepath.Join(
		t.TempDir(),
		strings.Repeat("long-path-segment-", 4),
		"daemon-root",
	)
	t.Setenv("CMUX_REMOTE_DAEMON_ROOT", rootBase)
	t.Setenv("CMUX_REMOTE_DAEMON_SOCKET_DIR", "")

	paths, err := persistentDaemonPathsForSlot(strings.Repeat("a", 128))
	if err != nil {
		t.Fatalf("persistentDaemonPathsForSlot returned error: %v", err)
	}
	if strings.HasPrefix(paths.socket, paths.root) {
		t.Fatalf("socket path should not live under long daemon root: socket=%q root=%q", paths.socket, paths.root)
	}
	if len(paths.socket) >= 100 {
		t.Fatalf("socket path length = %d, want < 100: %q", len(paths.socket), paths.socket)
	}
}

func TestPersistentDaemonPathsIncludeDaemonVersion(t *testing.T) {
	rootBase := filepath.Join(t.TempDir(), "daemon-root")
	t.Setenv("CMUX_REMOTE_DAEMON_ROOT", rootBase)
	t.Setenv("CMUX_REMOTE_DAEMON_SOCKET_DIR", "")
	oldVersion := version
	defer func() { version = oldVersion }()

	version = "v1.2.3"
	first, err := persistentDaemonPathsForSlot("versioned-slot")
	if err != nil {
		t.Fatalf("persistentDaemonPathsForSlot returned error: %v", err)
	}
	if !strings.Contains(first.root, string(filepath.Separator)+"v1.2.3"+string(filepath.Separator)) {
		t.Fatalf("root %q should include daemon version", first.root)
	}

	version = "v1.2.4"
	second, err := persistentDaemonPathsForSlot("versioned-slot")
	if err != nil {
		t.Fatalf("persistentDaemonPathsForSlot returned error: %v", err)
	}
	if first.root == second.root {
		t.Fatalf("root should change across versions: %q", first.root)
	}
	if first.socket == second.socket {
		t.Fatalf("socket should change across versions: %q", first.socket)
	}
	if first.lockFile == second.lockFile {
		t.Fatalf("lock file should change across versions: %q", first.lockFile)
	}
}

func TestPersistentDaemonTokenConcurrentCreate(t *testing.T) {
	root := t.TempDir()
	paths := persistentDaemonPaths{
		root:      root,
		tokenFile: filepath.Join(root, "auth.token"),
	}
	if err := os.MkdirAll(filepath.Dir(paths.tokenFile), 0o700); err != nil {
		t.Fatalf("mkdir token dir: %v", err)
	}

	const workers = 12
	var wg sync.WaitGroup
	results := make(chan string, workers)
	errorsCh := make(chan error, workers)
	for i := 0; i < workers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			token, err := persistentDaemonToken(paths)
			if err != nil {
				errorsCh <- err
				return
			}
			results <- token
		}()
	}
	wg.Wait()
	close(results)
	close(errorsCh)
	for err := range errorsCh {
		t.Fatalf("persistentDaemonToken returned error: %v", err)
	}
	var first string
	for token := range results {
		if len(token) != 64 {
			t.Fatalf("token length = %d, want 64", len(token))
		}
		if first == "" {
			first = token
			continue
		}
		if token != first {
			t.Fatalf("concurrent token mismatch: got %q want %q", token, first)
		}
	}
	onDisk, err := os.ReadFile(paths.tokenFile)
	if err != nil {
		t.Fatalf("read token file: %v", err)
	}
	if strings.TrimSpace(string(onDisk)) != first {
		t.Fatalf("token file = %q, want %q", strings.TrimSpace(string(onDisk)), first)
	}
}

func TestAuthenticatePersistentDaemonClientReadDeadline(t *testing.T) {
	client, server := net.Pipe()
	defer client.Close()
	defer server.Close()

	requestRead := make(chan error, 1)
	go func() {
		_, err := bufio.NewReader(server).ReadString('\n')
		requestRead <- err
	}()

	start := time.Now()
	err := authenticatePersistentDaemonClientWithTimeout(client, "token", 50*time.Millisecond)
	if err == nil {
		t.Fatalf("authenticatePersistentDaemonClientWithTimeout succeeded, want timeout error")
	}
	if elapsed := time.Since(start); elapsed > time.Second {
		t.Fatalf("authenticatePersistentDaemonClientWithTimeout took %s, want bounded deadline", elapsed)
	}
	select {
	case readErr := <-requestRead:
		if readErr != nil {
			t.Fatalf("server failed to read auth request: %v", readErr)
		}
	case <-time.After(time.Second):
		t.Fatalf("server did not receive auth request")
	}
}

func TestAuthenticatePersistentDaemonServerReadDeadline(t *testing.T) {
	client, server := net.Pipe()
	defer client.Close()

	hub := newWebSocketPTYHub(wsPTYServerConfig{}, io.Discard)
	defer hub.closeAll()

	done := make(chan struct{}, 1)
	go func() {
		handlePersistentDaemonConnWithAuthTimeout(server, persistentDaemonFixedTokenVerifier("token"), hub, 50*time.Millisecond)
		done <- struct{}{}
	}()

	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatalf("server auth handler did not return after deadline")
	}
}

func TestPersistentStdioProxyReturnsWhenDaemonClosesFirst(t *testing.T) {
	client, server := net.Pipe()
	stdinReader, stdinWriter := io.Pipe()
	defer stdinWriter.Close()

	done := make(chan error, 1)
	go func() {
		done <- proxyPersistentDaemonConn(stdinReader, io.Discard, client)
	}()

	_ = server.Close()
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("proxyPersistentDaemonConn returned error: %v", err)
		}
	case <-time.After(time.Second):
		t.Fatalf("proxyPersistentDaemonConn did not return after daemon side closed")
	}
	_ = stdinWriter.Close()
}

func TestRunStdioSlotRequiresPersistent(t *testing.T) {
	var stderr bytes.Buffer
	code := run([]string{"serve", "--stdio", "--slot", "slot-without-persistent"}, strings.NewReader(""), &bytes.Buffer{}, &stderr)
	if code != 2 {
		t.Fatalf("run serve exit code = %d, want 2", code)
	}
	if !strings.Contains(stderr.String(), "serve --slot requires --persistent") {
		t.Fatalf("stderr = %q, want --slot validation error", stderr.String())
	}
}

func TestRunStdioInvalidJSONAndUnknownMethod(t *testing.T) {
	input := strings.NewReader(
		`{"id":1,"method":"hello","params":{}` + "\n" +
			`{"id":2,"method":"unknown","params":{}}` + "\n",
	)
	var out bytes.Buffer
	code := run([]string{"serve", "--stdio"}, input, &out, &bytes.Buffer{})
	if code != 0 {
		t.Fatalf("run serve exit code = %d, want 0", code)
	}

	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 2 {
		t.Fatalf("got %d response lines, want 2: %q", len(lines), out.String())
	}

	var first map[string]any
	if err := json.Unmarshal([]byte(lines[0]), &first); err != nil {
		t.Fatalf("failed to decode first response: %v", err)
	}
	if ok, _ := first["ok"].(bool); ok {
		t.Fatalf("first response should be ok=false for invalid JSON: %v", first)
	}
	firstError, _ := first["error"].(map[string]any)
	if got := firstError["code"]; got != "invalid_request" {
		t.Fatalf("invalid JSON should return invalid_request; got=%v payload=%v", got, first)
	}

	var second map[string]any
	if err := json.Unmarshal([]byte(lines[1]), &second); err != nil {
		t.Fatalf("failed to decode second response: %v", err)
	}
	if ok, _ := second["ok"].(bool); ok {
		t.Fatalf("second response should be ok=false for unknown method: %v", second)
	}
	secondError, _ := second["error"].(map[string]any)
	if got := secondError["code"]; got != "method_not_found" {
		t.Fatalf("unknown method should return method_not_found; got=%v payload=%v", got, second)
	}
}

func TestRunStdioSessionResizeFlow(t *testing.T) {
	input := strings.NewReader(
		`{"id":1,"method":"session.open","params":{"session_id":"sess-stdio"}}` + "\n" +
			`{"id":2,"method":"session.attach","params":{"session_id":"sess-stdio","attachment_id":"a1","cols":120,"rows":40}}` + "\n" +
			`{"id":3,"method":"session.attach","params":{"session_id":"sess-stdio","attachment_id":"a2","cols":90,"rows":30}}` + "\n" +
			`{"id":4,"method":"session.status","params":{"session_id":"sess-stdio"}}` + "\n",
	)
	var out bytes.Buffer
	code := run([]string{"serve", "--stdio"}, input, &out, &bytes.Buffer{})
	if code != 0 {
		t.Fatalf("run serve exit code = %d, want 0", code)
	}

	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 4 {
		t.Fatalf("got %d response lines, want 4: %q", len(lines), out.String())
	}

	var status map[string]any
	if err := json.Unmarshal([]byte(lines[3]), &status); err != nil {
		t.Fatalf("failed to decode status response: %v", err)
	}
	if ok, _ := status["ok"].(bool); !ok {
		t.Fatalf("session.status should be ok=true: %v", status)
	}
	result, _ := status["result"].(map[string]any)
	if result == nil {
		t.Fatalf("session.status missing result object: %v", status)
	}
	effectiveCols, _ := result["effective_cols"].(float64)
	effectiveRows, _ := result["effective_rows"].(float64)
	if int(effectiveCols) != 90 || int(effectiveRows) != 30 {
		t.Fatalf("session smallest-wins effective size mismatch: got=%vx%v payload=%v", effectiveCols, effectiveRows, result)
	}
}

func TestProxyStreamRoundTrip(t *testing.T) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen failed: %v", err)
	}
	defer listener.Close()

	done := make(chan struct{})
	go func() {
		defer close(done)
		conn, acceptErr := listener.Accept()
		if acceptErr != nil {
			return
		}
		defer conn.Close()

		buffer := make([]byte, 4)
		if _, readErr := io.ReadFull(conn, buffer); readErr != nil {
			return
		}
		if string(buffer) != "ping" {
			return
		}
		_, _ = conn.Write([]byte("pong"))
	}()

	eventOutput := newNotifyingBuffer()
	server := &rpcServer{
		nextStreamID:  1,
		nextSessionID: 1,
		streams:       map[string]*streamState{},
		sessions:      map[string]*sessionState{},
		frameWriter: &stdioFrameWriter{
			writer: bufio.NewWriter(eventOutput),
		},
	}
	defer server.closeAll()

	port := listener.Addr().(*net.TCPAddr).Port
	openResp := server.handleRequest(rpcRequest{
		ID:     1,
		Method: "proxy.open",
		Params: map[string]any{
			"host":       "127.0.0.1",
			"port":       port,
			"timeout_ms": 1000,
		},
	})
	if !openResp.OK {
		t.Fatalf("proxy.open failed: %+v", openResp)
	}
	openResult, _ := openResp.Result.(map[string]any)
	streamID, _ := openResult["stream_id"].(string)
	if streamID == "" {
		t.Fatalf("proxy.open missing stream_id: %+v", openResp)
	}

	writeResp := server.handleRequest(rpcRequest{
		ID:     2,
		Method: "proxy.write",
		Params: map[string]any{
			"stream_id":   streamID,
			"data_base64": base64.StdEncoding.EncodeToString([]byte("ping")),
		},
	})
	if !writeResp.OK {
		t.Fatalf("proxy.write failed: %+v", writeResp)
	}

	readResp := server.handleRequest(rpcRequest{
		ID:     3,
		Method: "proxy.stream.subscribe",
		Params: map[string]any{
			"stream_id": streamID,
		},
	})
	if !readResp.OK {
		t.Fatalf("proxy.stream.subscribe failed: %+v", readResp)
	}
	select {
	case <-eventOutput.notify:
	case <-time.After(2 * time.Second):
		t.Fatalf("timed out waiting for proxy.stream.data event")
	}

	lines := strings.Split(strings.TrimSpace(eventOutput.String()), "\n")
	if len(lines) == 0 || strings.TrimSpace(lines[0]) == "" {
		t.Fatalf("proxy.stream.data event output was empty")
	}

	var event map[string]any
	if err := json.Unmarshal([]byte(lines[0]), &event); err != nil {
		t.Fatalf("failed to decode stream event: %v", err)
	}
	if got := event["event"]; got != "proxy.stream.data" {
		t.Fatalf("unexpected stream event=%v payload=%v", got, event)
	}
	dataBase64, _ := event["data_base64"].(string)
	data, decodeErr := base64.StdEncoding.DecodeString(dataBase64)
	if decodeErr != nil {
		t.Fatalf("proxy.stream.data returned invalid base64: %v", decodeErr)
	}
	if string(data) != "pong" {
		t.Fatalf("proxy.stream.data payload=%q, want %q", string(data), "pong")
	}

	closeResp := server.handleRequest(rpcRequest{
		ID:     4,
		Method: "proxy.close",
		Params: map[string]any{
			"stream_id": streamID,
		},
	})
	if !closeResp.OK {
		t.Fatalf("proxy.close failed: %+v", closeResp)
	}

	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatalf("proxy test server goroutine did not finish")
	}
}

func TestProxyStreamEOFPayloadIsNotDuplicatedAcrossDataAndEOFEvents(t *testing.T) {
	eventOutput := newNotifyingBuffer()
	server := &rpcServer{
		nextStreamID:  1,
		nextSessionID: 1,
		streams: map[string]*streamState{
			"stream-1": {
				conn: &eofWithPayloadConn{payload: []byte("tail")},
			},
		},
		sessions: map[string]*sessionState{},
		frameWriter: &stdioFrameWriter{
			writer: bufio.NewWriter(eventOutput),
		},
	}
	defer server.closeAll()

	resp := server.handleRequest(rpcRequest{
		ID:     1,
		Method: "proxy.stream.subscribe",
		Params: map[string]any{"stream_id": "stream-1"},
	})
	if !resp.OK {
		t.Fatalf("proxy.stream.subscribe failed: %+v", resp)
	}

	deadline := time.Now().Add(2 * time.Second)
	for strings.Count(strings.TrimSpace(eventOutput.String()), "\n")+boolToInt(strings.TrimSpace(eventOutput.String()) != "") < 2 {
		remaining := time.Until(deadline)
		if remaining <= 0 {
			t.Fatalf("timed out waiting for proxy stream events: %q", eventOutput.String())
		}
		select {
		case <-eventOutput.notify:
		case <-time.After(remaining):
			t.Fatalf("timed out waiting for proxy stream events: %q", eventOutput.String())
		}
	}

	lines := strings.Split(strings.TrimSpace(eventOutput.String()), "\n")
	if len(lines) != 2 {
		t.Fatalf("expected exactly 2 stream events, got %d: %q", len(lines), eventOutput.String())
	}

	var first map[string]any
	if err := json.Unmarshal([]byte(lines[0]), &first); err != nil {
		t.Fatalf("decode first event: %v", err)
	}
	var second map[string]any
	if err := json.Unmarshal([]byte(lines[1]), &second); err != nil {
		t.Fatalf("decode second event: %v", err)
	}

	if got := first["event"]; got != "proxy.stream.data" {
		t.Fatalf("first event = %v, want proxy.stream.data", got)
	}
	if got := second["event"]; got != "proxy.stream.eof" {
		t.Fatalf("second event = %v, want proxy.stream.eof", got)
	}

	firstPayload, err := base64.StdEncoding.DecodeString(first["data_base64"].(string))
	if err != nil {
		t.Fatalf("decode first payload: %v", err)
	}
	secondPayload, err := decodeOptionalBase64(second["data_base64"])
	if err != nil {
		t.Fatalf("decode second payload: %v", err)
	}

	if string(firstPayload) != "tail" {
		t.Fatalf("proxy.stream.data payload = %q, want %q", string(firstPayload), "tail")
	}
	if len(secondPayload) != 0 {
		t.Fatalf("proxy.stream.eof payload = %q, want empty payload after data event", string(secondPayload))
	}
}

func waitForRPCEvent(t *testing.T, buffer *notifyingBuffer, startLine int, matches func(map[string]any) bool) map[string]any {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for {
		lines := rpcEventLines(buffer)
		for _, line := range lines[min(startLine, len(lines)):] {
			var event map[string]any
			if err := json.Unmarshal([]byte(line), &event); err != nil {
				continue
			}
			if matches(event) {
				return event
			}
		}
		remaining := time.Until(deadline)
		if remaining <= 0 {
			t.Fatalf("timed out waiting for matching RPC event after line %d: %q", startLine, buffer.String())
		}
		select {
		case <-buffer.notify:
		case <-time.After(remaining):
			t.Fatalf("timed out waiting for matching RPC event after line %d: %q", startLine, buffer.String())
		}
	}
}

func rpcEventLineCount(buffer *notifyingBuffer) int {
	return len(rpcEventLines(buffer))
}

func rpcEventLines(buffer *notifyingBuffer) []string {
	trimmed := strings.TrimSpace(buffer.String())
	if trimmed == "" {
		return nil
	}
	return strings.Split(trimmed, "\n")
}

func boolToInt(value bool) int {
	if value {
		return 1
	}
	return 0
}

func decodeOptionalBase64(value any) ([]byte, error) {
	encoded, ok := value.(string)
	if !ok || encoded == "" {
		return nil, nil
	}
	return base64.StdEncoding.DecodeString(encoded)
}

func TestGetIntParamRejectsFractionalFloat64(t *testing.T) {
	params := map[string]any{
		"port":       80.9,
		"timeout_ms": 100.0,
	}

	if _, ok := getIntParam(params, "port"); ok {
		t.Fatalf("fractional float64 should be rejected")
	}

	timeout, ok := getIntParam(params, "timeout_ms")
	if !ok {
		t.Fatalf("integral float64 should be accepted")
	}
	if timeout != 100 {
		t.Fatalf("timeout_ms = %d, want 100", timeout)
	}
}

func TestRunStdioOversizedFrameContinuesServing(t *testing.T) {
	oversized := `{"id":1,"method":"ping","params":{"blob":"` + strings.Repeat("a", maxRPCFrameBytes) + `"}}`
	input := strings.NewReader(oversized + "\n" + `{"id":2,"method":"ping","params":{}}` + "\n")
	var out bytes.Buffer
	code := run([]string{"serve", "--stdio"}, input, &out, &bytes.Buffer{})
	if code != 0 {
		t.Fatalf("run serve exit code = %d, want 0", code)
	}

	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 2 {
		t.Fatalf("got %d response lines, want 2: %q", len(lines), out.String())
	}

	var first map[string]any
	if err := json.Unmarshal([]byte(lines[0]), &first); err != nil {
		t.Fatalf("failed to decode first response: %v", err)
	}
	if ok, _ := first["ok"].(bool); ok {
		t.Fatalf("first response should be oversized-frame error: %v", first)
	}
	firstError, _ := first["error"].(map[string]any)
	if got := firstError["code"]; got != "invalid_request" {
		t.Fatalf("oversized frame should return invalid_request; got=%v payload=%v", got, first)
	}

	var second map[string]any
	if err := json.Unmarshal([]byte(lines[1]), &second); err != nil {
		t.Fatalf("failed to decode second response: %v", err)
	}
	if ok, _ := second["ok"].(bool); !ok {
		t.Fatalf("second response should still be handled after oversized frame: %v", second)
	}
}

func TestProxyOpenInvalidParams(t *testing.T) {
	server := &rpcServer{
		nextStreamID:  1,
		nextSessionID: 1,
		streams:       map[string]*streamState{},
		sessions:      map[string]*sessionState{},
	}
	defer server.closeAll()

	resp := server.handleRequest(rpcRequest{
		ID:     1,
		Method: "proxy.open",
		Params: map[string]any{
			"host": "127.0.0.1",
			"port": strconv.Itoa(8080),
		},
	})
	if resp.OK {
		t.Fatalf("proxy.open with invalid port type should fail: %+v", resp)
	}
	errObj, _ := resp.Error, resp.Error
	if errObj == nil || errObj.Code != "invalid_params" {
		t.Fatalf("proxy.open invalid params should return invalid_params: %+v", resp)
	}
}

func TestSessionResizeCoordinator(t *testing.T) {
	server := &rpcServer{
		nextStreamID:  1,
		nextSessionID: 1,
		streams:       map[string]*streamState{},
		sessions:      map[string]*sessionState{},
	}
	defer server.closeAll()

	openResp := server.handleRequest(rpcRequest{
		ID:     1,
		Method: "session.open",
		Params: map[string]any{
			"session_id": "sess-rz",
		},
	})
	if !openResp.OK {
		t.Fatalf("session.open failed: %+v", openResp)
	}

	attachSmall := server.handleRequest(rpcRequest{
		ID:     2,
		Method: "session.attach",
		Params: map[string]any{
			"session_id":    "sess-rz",
			"attachment_id": "a-small",
			"cols":          90,
			"rows":          30,
		},
	})
	assertEffectiveSize(t, attachSmall, 90, 30)

	attachLarge := server.handleRequest(rpcRequest{
		ID:     3,
		Method: "session.attach",
		Params: map[string]any{
			"session_id":    "sess-rz",
			"attachment_id": "a-large",
			"cols":          120,
			"rows":          40,
		},
	})
	assertEffectiveSize(t, attachLarge, 90, 30) // RZ-001: smallest wins

	resizeLarge := server.handleRequest(rpcRequest{
		ID:     4,
		Method: "session.resize",
		Params: map[string]any{
			"session_id":    "sess-rz",
			"attachment_id": "a-large",
			"cols":          200,
			"rows":          60,
		},
	})
	assertEffectiveSize(t, resizeLarge, 90, 30) // RZ-002: still bounded by smallest

	detachSmall := server.handleRequest(rpcRequest{
		ID:     5,
		Method: "session.detach",
		Params: map[string]any{
			"session_id":    "sess-rz",
			"attachment_id": "a-small",
		},
	})
	assertEffectiveSize(t, detachSmall, 200, 60) // RZ-003: expands to next smallest

	detachLarge := server.handleRequest(rpcRequest{
		ID:     6,
		Method: "session.detach",
		Params: map[string]any{
			"session_id":    "sess-rz",
			"attachment_id": "a-large",
		},
	})
	assertEffectiveSize(t, detachLarge, 200, 60) // no attachments: keep last-known size
	assertAttachmentCount(t, detachLarge, 0)

	reattach := server.handleRequest(rpcRequest{
		ID:     7,
		Method: "session.attach",
		Params: map[string]any{
			"session_id":    "sess-rz",
			"attachment_id": "a-reconnect",
			"cols":          110,
			"rows":          50,
		},
	})
	assertEffectiveSize(t, reattach, 110, 50) // RZ-004: recompute from active attachments on reattach
}

func TestSessionInvalidParamsAndNotFound(t *testing.T) {
	server := &rpcServer{
		nextStreamID:  1,
		nextSessionID: 1,
		streams:       map[string]*streamState{},
		sessions:      map[string]*sessionState{},
	}
	defer server.closeAll()

	missingSession := server.handleRequest(rpcRequest{
		ID:     1,
		Method: "session.attach",
		Params: map[string]any{
			"session_id":    "missing",
			"attachment_id": "a1",
			"cols":          80,
			"rows":          24,
		},
	})
	if missingSession.OK || missingSession.Error == nil || missingSession.Error.Code != "not_found" {
		t.Fatalf("session.attach on missing session should return not_found: %+v", missingSession)
	}

	badSize := server.handleRequest(rpcRequest{
		ID:     2,
		Method: "session.attach",
		Params: map[string]any{
			"session_id":    "missing",
			"attachment_id": "a1",
			"cols":          0,
			"rows":          24,
		},
	})
	if badSize.OK || badSize.Error == nil || badSize.Error.Code != "invalid_params" {
		t.Fatalf("session.attach with cols=0 should return invalid_params: %+v", badSize)
	}
}

func assertEffectiveSize(t *testing.T, resp rpcResponse, wantCols, wantRows int) {
	t.Helper()
	if !resp.OK {
		t.Fatalf("expected ok response, got error: %+v", resp)
	}
	result, ok := resp.Result.(map[string]any)
	if !ok {
		t.Fatalf("response missing result map: %+v", resp)
	}
	gotCols := asInt(t, result["effective_cols"], "effective_cols")
	gotRows := asInt(t, result["effective_rows"], "effective_rows")
	if gotCols != wantCols || gotRows != wantRows {
		t.Fatalf("effective size = %dx%d, want %dx%d payload=%+v", gotCols, gotRows, wantCols, wantRows, result)
	}
}

func assertAttachmentCount(t *testing.T, resp rpcResponse, want int) {
	t.Helper()
	if !resp.OK {
		t.Fatalf("expected ok response, got error: %+v", resp)
	}
	result, ok := resp.Result.(map[string]any)
	if !ok {
		t.Fatalf("response missing result map: %+v", resp)
	}
	attachments, ok := result["attachments"].([]map[string]any)
	if ok {
		if len(attachments) != want {
			t.Fatalf("attachments len = %d, want %d payload=%+v", len(attachments), want, result)
		}
		return
	}
	attachmentsAny, ok := result["attachments"].([]any)
	if !ok {
		t.Fatalf("attachments field has unexpected type (%T) payload=%+v", result["attachments"], result)
	}
	if len(attachmentsAny) != want {
		t.Fatalf("attachments len = %d, want %d payload=%+v", len(attachmentsAny), want, result)
	}
}

func asInt(t *testing.T, value any, field string) int {
	t.Helper()
	switch typed := value.(type) {
	case int:
		return typed
	case int8:
		return int(typed)
	case int16:
		return int(typed)
	case int32:
		return int(typed)
	case int64:
		return int(typed)
	case uint:
		return int(typed)
	case uint8:
		return int(typed)
	case uint16:
		return int(typed)
	case uint32:
		return int(typed)
	case uint64:
		return int(typed)
	case float64:
		if typed != math.Trunc(typed) {
			t.Fatalf("%s should be integer-valued, got %v", field, typed)
		}
		return int(typed)
	default:
		t.Fatalf("%s has unexpected type %T (%v)", field, value, value)
		return 0
	}
}
