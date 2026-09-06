package bgeemb

// tokenizer_zh.go — WordPiece tokenizer driven by a HuggingFace tokenizer.json.
//
// Reimplements the exact semantics of HuggingFace's BertNormalizer (lowercase,
// clean_text, handle_chinese_chars) + BertPreTokenizer (whitespace+punct split)
// + WordPiece model (greedy longest-match, "##" continuation) in pure Go, so
// the v4 server can embed Chinese (bge-large-zh) without any external runtime.
//
// The vocab is read from the same tokenizer.json that ships with the model
// (HF "fast" tokenizer artifact, BertNormalizer + WordPiece). Special tokens
// [CLS]=101 / [SEP]=102 / [UNK]=100 / [PAD]=0 are looked up from added_tokens.

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"unicode"
)

// TokenizerConfig is the subset of tokenizer.json we consume.
type TokenizerConfig struct {
	AddedTokens []struct {
		ID      int64  `json:"id"`
		Content string `json:"content"`
	} `json:"added_tokens"`
	Model struct {
		Type                  string            `json:"type"`
		UnkToken              string            `json:"unk_token"`
		ContinuingPrefix      string            `json:"continuing_subword_prefix"`
		MaxInputCharsPerWord  int               `json:"max_input_chars_per_word"`
		Vocab                 map[string]int64  `json:"vocab"`
	} `json:"model"`
}

// Tokenizer is an immutable, thread-safe WordPiece tokenizer.
type Tokenizer struct {
	vocab    map[string]int64
	unkID    int64
	clsID    int64
	sepID    int64
	padID    int64
	contPrev string // "##"
	maxChars int
}

// LoadTokenizer parses a HF tokenizer.json (BertNormalizer + WordPiece layout).
func LoadTokenizer(path string) (*Tokenizer, error) {
	b, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("tokenizer.json: %w", err)
	}
	var cfg TokenizerConfig
	if err := json.Unmarshal(b, &cfg); err != nil {
		return nil, fmt.Errorf("tokenizer.json parse: %w", err)
	}
	if cfg.Model.Type != "WordPiece" {
		return nil, fmt.Errorf("tokenizer.json: unsupported model type %q (want WordPiece)", cfg.Model.Type)
	}
	if len(cfg.Model.Vocab) == 0 {
		return nil, fmt.Errorf("tokenizer.json: empty vocab")
	}
	t := &Tokenizer{
		vocab:    cfg.Model.Vocab,
		contPrev: cfg.Model.ContinuingPrefix,
		maxChars: cfg.Model.MaxInputCharsPerWord,
	}
	if t.contPrev == "" {
		t.contPrev = "##"
	}
	if t.maxChars <= 0 {
		t.maxChars = 100
	}
	special := map[string]*int64{
		"[UNK]": &t.unkID, "[CLS]": &t.clsID, "[SEP]": &t.sepID, "[PAD]": &t.padID,
	}
	for name, idp := range special {
		if id, ok := cfg.Model.Vocab[name]; ok {
			*idp = id
		}
	}
	for _, at := range cfg.AddedTokens {
		switch at.Content {
		case "[UNK]":
			t.unkID = at.ID
		case "[CLS]":
			t.clsID = at.ID
		case "[SEP]":
			t.sepID = at.ID
		case "[PAD]":
			t.padID = at.ID
		}
	}
	if t.unkID == 0 && cfg.Model.UnkToken != "" {
		if id, ok := cfg.Model.Vocab[cfg.Model.UnkToken]; ok {
			t.unkID = id
		}
	}
	return t, nil
}

// bertNormalize mirrors HuggingFace BertNormalizer(clean_text=true,
// handle_chinese_chars=true, strip_accents=None(=strip when lowercase),
// lowercase=true): strip control/format chars, space-out CJK, lowercase,
// strip combining marks. All whitespace runs collapse to a single ASCII space.
func bertNormalize(s string) string {
	var b strings.Builder
	b.Grow(len(s) + 16)
	for _, r := range s {
		switch {
		case r == 0 || r == 0xFFFD || isControlRune(r):
			// drop
		case isWhitespaceRune(r):
			b.WriteByte(' ')
		case isCJKRune(r):
			b.WriteByte(' ')
			b.WriteRune(r)
			b.WriteByte(' ')
		default:
			lo := unicode.ToLower(r)
			if lo > 0x7F && unicode.Is(unicode.Mn, lo) {
				continue // strip accents (combining marks), HF lowercase default
			}
			b.WriteRune(lo)
		}
	}
	return b.String()
}

func isWhitespaceRune(r rune) bool {
	switch r {
	case ' ', '\t', '\n', '\r', '\f', '\v':
		return true
	}
	return unicode.IsSpace(r)
}

func isControlRune(r rune) bool {
	switch r {
	case '\t', '\n', '\r':
		return false // HF treats these as whitespace, not control
	}
	return unicode.IsControl(r)
}

// isCJKRune mirrors HF handle_chinese_chars: CJK blocks that get spaced out.
func isCJKRune(r rune) bool {
	switch {
	case r >= 0x4E00 && r <= 0x9FFF: // CJK Unified Ideographs
		return true
	case r >= 0x3400 && r <= 0x4DBF: // CJK Extension A
		return true
	case r >= 0xF900 && r <= 0xFAFF: // CJK Compatibility Ideographs
		return true
	case r >= 0x20000 && r <= 0x2A6DF: // Extension B
		return true
	case r >= 0x2A700 && r <= 0x2B73F, r >= 0x2B740 && r <= 0x2B81F,
		r >= 0x2B820 && r <= 0x2CEAF, r >= 0x2CEB0 && r <= 0x2EBEF:
		return true
	case r >= 0x3000 && r <= 0x303F: // CJK Symbols and Punctuation
		return true
	case r >= 0x3040 && r <= 0x30FF: // Hiragana / Katakana
		return true
	case r >= 0x31C0 && r <= 0x31EF: // CJK Strokes
		return true
	case r >= 0xFF00 && r <= 0xFFEF: // Fullwidth forms
		return true
	}
	return false
}

// isPunctRune mirrors HF BertPreTokenizer punctuation class (P* categories).
func isPunctRune(r rune) bool {
	return unicode.IsPunct(r) || unicode.IsSymbol(r)
}

// preTokenize mirrors BertPreTokenizer: split on whitespace and isolate
// punctuation, each into its own piece.
func preTokenize(s string) []string {
	var words []string
	var cur strings.Builder
	flush := func() {
		if cur.Len() > 0 {
			words = append(words, cur.String())
			cur.Reset()
		}
	}
	for _, r := range s {
		switch {
		case isWhitespaceRune(r):
			flush()
		case isPunctRune(r):
			flush()
			words = append(words, string(r))
		default:
			cur.WriteRune(r)
		}
	}
	flush()
	return words
}

// wordpiece encodes one pre-tokenized word; ok=false when the word cannot be
// fully covered (HF WordPiece then emits [UNK] for the whole word).
func (t *Tokenizer) wordpiece(word string) ([]int64, bool) {
	runes := []rune(word)
	if len(runes) > t.maxChars {
		return []int64{t.unkID}, false
	}
	ids := make([]int64, 0, len(runes))
	start := 0
	for start < len(runes) {
		end := len(runes)
		matchID, found := int64(0), false
		for end > start {
			sub := string(runes[start:end])
			if start > 0 {
				sub = t.contPrev + sub
			}
			if id, ok := t.vocab[sub]; ok {
				matchID, found = id, true
				break
			}
			end--
		}
		if !found {
			return []int64{t.unkID}, false
		}
		ids = append(ids, matchID)
		start = end
	}
	return ids, true
}

// Encode tokenizes text to input_ids + attention_mask with [CLS] ... [SEP].
// MaxLen counts the special tokens (matching HF truncation: max_length
// includes [CLS]/[SEP]). Whitespace-only/empty text yields [CLS][SEP].
func (t *Tokenizer) Encode(text string, maxLen int) (ids, mask []int64) {
	normalized := bertNormalize(text)
	words := preTokenize(normalized)
	out := make([]int64, 1, maxLen)
	out[0] = t.clsID
	for _, w := range words {
		if len(out) >= maxLen-1 {
			break
		}
		pieces, ok := t.wordpiece(w)
		if !ok {
			pieces = []int64{t.unkID}
		}
		for _, p := range pieces {
			if len(out) >= maxLen-1 {
				break
			}
			out = append(out, p)
		}
	}
	out = append(out, t.sepID)
	mask = make([]int64, len(out))
	for i := range mask {
		mask[i] = 1
	}
	return out, mask
}
