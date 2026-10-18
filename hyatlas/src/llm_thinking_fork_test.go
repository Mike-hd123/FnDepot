package main

// fork (fnos) tests: the request-body switches added for this packaging —
// top-level enable_thinking, an optional max_tokens, and the recovery when a
// channel rejects the unknown field.
//
// Measured on the box: with a 1,905-token consolidation prompt the reasoning-first
// routes return content="" and finish_reason=length after burning the whole output
// budget (95s / 6,000 tokens), so every cluster call died at the 10-minute pass
// bound with nothing parsed. Sending "enable_thinking": false at the TOP LEVEL
// (reasoning_effort and chat_template_kwargs are ignored by these channels) makes
// the same call answer in 12.4s with 732 tokens and a complete JSON object.

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

// captureServer replies with canned bodies per request and records each request body.
func captureServer(t *testing.T, replies []string) (*httptest.Server, *[]map[string]any) {
	t.Helper()
	var seen []map[string]any
	n := 0
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		var spec map[string]any
		if err := json.Unmarshal(body, &spec); err != nil {
			t.Errorf("request body is not JSON: %v (%.120s)", err, body)
		}
		seen = append(seen, spec)
		idx := n
		n++
		if idx >= len(replies) {
			idx = len(replies) - 1
		}
		// A reply starting with "HTTP " is a status the server should echo.
		if len(replies[idx]) > 5 && replies[idx][:5] == "HTTP " {
			code, _ := strconv.Atoi(replies[idx][5:])
			w.WriteHeader(code)
			_, _ = w.Write([]byte(`{"error":"unknown field"}`))
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(replies[idx]))
	}))
	t.Cleanup(srv.Close)
	return srv, &seen
}

const okReply = `{"choices":[{"message":{"content":"{\"operations\":[]}"},"finish_reason":"stop"}]}`

func TestChatOnceSendsEnableThinkingAndNoMaxTokensByDefault(t *testing.T) {
	srv, seen := captureServer(t, []string{okReply})
	defer srv.Close()
	l := NewLLMClient(srv.URL, "k", "m")
	l.DisableThinking = true // the shipped default for this packaging

	got, err := l.chatOnce(context.Background(), srv.URL, "m", "k",
		[]map[string]string{{"role": "user", "content": "hi"}}, 0.2)
	if err != nil {
		t.Fatalf("chatOnce: %v", err)
	}
	if got != `{"operations":[]}` {
		t.Fatalf("content = %q", got)
	}
	if len(*seen) != 1 {
		t.Fatalf("requests = %d, want 1", len(*seen))
	}
	spec := (*seen)[0]
	v, ok := spec["enable_thinking"]
	if !ok {
		t.Fatal("enable_thinking missing from the request body")
	}
	if b, isBool := v.(bool); !isBool || b {
		t.Fatalf("enable_thinking = %v, want false", v)
	}
	if _, present := spec["max_tokens"]; present {
		t.Fatalf("max_tokens must not be sent when unset: %v", spec["max_tokens"])
	}
}

func TestChatOnceSendsMaxTokensWhenSet(t *testing.T) {
	srv, seen := captureServer(t, []string{okReply})
	defer srv.Close()
	l := NewLLMClient(srv.URL, "k", "m")
	l.DisableThinking = true
	l.MaxTokens = 1234

	if _, err := l.chatOnce(context.Background(), srv.URL, "m", "k",
		[]map[string]string{{"role": "user", "content": "hi"}}, 0.2); err != nil {
		t.Fatalf("chatOnce: %v", err)
	}
	spec := (*seen)[0]
	// JSON numbers decode as float64.
	if mt, _ := spec["max_tokens"].(float64); mt != 1234 {
		t.Fatalf("max_tokens = %v, want 1234", spec["max_tokens"])
	}
}

func TestChatOnceRetriesWithoutThinkingOn400(t *testing.T) {
	srv, seen := captureServer(t, []string{"HTTP 400", okReply})
	defer srv.Close()
	l := NewLLMClient(srv.URL, "k", "m")
	l.DisableThinking = true

	got, err := l.chatOnce(context.Background(), srv.URL, "m", "k",
		[]map[string]string{{"role": "user", "content": "hi"}}, 0.2)
	if err != nil {
		t.Fatalf("the 400 must be recovered by the retry, got: %v", err)
	}
	if got != `{"operations":[]}` {
		t.Fatalf("content = %q", got)
	}
	if len(*seen) != 2 {
		t.Fatalf("requests = %d, want 2 (retry after the 400)", len(*seen))
	}
	if _, ok := (*seen)[0]["enable_thinking"]; !ok {
		t.Fatal("first request should have carried enable_thinking")
	}
	if _, ok := (*seen)[1]["enable_thinking"]; ok {
		t.Fatal("the retry must drop enable_thinking, or a strict channel 400s forever")
	}
}

func TestChatOnceKeeps400WhenThinkingOff(t *testing.T) {
	srv, seen := captureServer(t, []string{"HTTP 400", okReply})
	defer srv.Close()
	l := NewLLMClient(srv.URL, "k", "m")
	l.DisableThinking = false // operator opted out: upstream shape, no invented field

	_, err := l.chatOnce(context.Background(), srv.URL, "m", "k",
		[]map[string]string{{"role": "user", "content": "hi"}}, 0.2)
	if err == nil {
		t.Fatal("want the 400 to surface as an error")
	}
	if len(*seen) != 1 {
		t.Fatalf("requests = %d, want 1 (no retry when we never sent the field)", len(*seen))
	}
}

func TestChatOnceEmptyReplyStillNamedPlainly(t *testing.T) {
	srv, _ := captureServer(t, []string{`{"choices":[{"message":{"content":""},"finish_reason":"length"}]}`})
	defer srv.Close()
	l := NewLLMClient(srv.URL, "k", "m")
	l.DisableThinking = true

	_, err := l.chatOnce(context.Background(), srv.URL, "m", "k",
		[]map[string]string{{"role": "user", "content": "hi"}}, 0.2)
	if err == nil {
		t.Fatal("an empty message must be an error, not a silent empty answer")
	}
	// finish_reason must be reported: it separates "cut off by the provider" from
	// "answered with nothing", and the two want different fixes.
	if !strings.Contains(err.Error(), "finish_reason") {
		t.Fatalf("error should name the finish reason, got: %v", err)
	}
}
