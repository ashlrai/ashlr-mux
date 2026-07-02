//go:build !windows

package main

import (
	"encoding/json"
	"testing"
)

func FuzzWebSocketPTYControlFrame(f *testing.F) {
	for _, seed := range []string{
		`{"type":"resize","cols":80,"rows":24}`,
		`{"type":"resize","cols":1000000,"rows":1000000}`,
		`{"type":"close"}`,
		`{"type":"resize","cols":-1,"rows":24}`,
		`{"type":"\u001b[?2026$p","cols":38,"rows":2}`,
	} {
		f.Add(seed)
	}

	f.Fuzz(func(t *testing.T, input string) {
		var frame wsPTYControlFrame
		_ = json.Unmarshal([]byte(input), &frame)
		if frame.Type == "resize" && frame.Cols > 0 && frame.Rows > 0 {
			cols, rows := normalizePTYSize(frame.Cols, frame.Rows)
			if cols <= 0 || rows <= 0 {
				t.Fatalf("normalizePTYSize(%d, %d) = %dx%d, expected positive dimensions", frame.Cols, frame.Rows, cols, rows)
			}
			if cols > maxPTYDimension || rows > maxPTYDimension {
				t.Fatalf("normalizePTYSize(%d, %d) = %dx%d, expected <= %d", frame.Cols, frame.Rows, cols, rows, maxPTYDimension)
			}
		}
	})
}
