package bgeemb

// bge_legacy_en.go — V1 (bge-small-en-v1.5) tokenizer path retained for the
// legacy English layout. Used only when the model is the small-en variant and
// no tokenizer.json is present (the upstream v4 branch hardcodes this).

import (
	"fmt"
	"os"
	"strings"
	"unicode/utf8"
)

const (
	enCLSID int64 = 101
	enSEPID int64 = 102
	enUNKID int64 = 100
	enPADID int64 = 0
)

// loadVocab reads a BERT vocab.txt into a token -> id map.
func loadVocab(path string) (map[string]int64, error) {
	b, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("vocab: %w", err)
	}
	lines := strings.Split(strings.ReplaceAll(string(b), "\r\n", "\n"), "\n")
	vocab := make(map[string]int64, len(lines))
	for i, w := range lines {
		if w == "" {
			continue
		}
		vocab[w] = int64(i)
	}
	return vocab, nil
}

// longestToken returns the rune length of the longest vocab entry. No longer
// substring can match, so wordpiece never needs to try one (ported from
// upstream v5.0.0: bounds the scan for one huge token such as a pasted hash).
func longestToken(vocab map[string]int64) int {
	n := 0
	for w := range vocab {
		if l := utf8.RuneCountInString(w); l > n {
			n = l
		}
	}
	return n
}

// tokenizeV1 does BERT-style lowercase WordPiece to input_ids + attention_mask.
// Mirrors the original v4 bge.go path. maxTok is the vocab's longest entry
// (see longestToken); <=0 falls back to the unbounded legacy scan.
func tokenizeV1(vocab map[string]int64, text string, maxLen, maxTok int) ([]int64, []int64) {
	text = strings.ToLower(strings.TrimSpace(text))
	words := strings.Fields(text)
	if len(words) == 0 {
		words = []string{"[unk]"}
	}
	ids := []int64{enCLSID}
	for _, w := range words {
		for _, piece := range wordpieceV1(vocab, w, 200, maxTok) {
			ids = append(ids, piece)
		}
	}
	if len(ids) >= maxLen-1 {
		ids = ids[:maxLen-1]
	}
	ids = append(ids, enSEPID)
	seq := len(ids)
	mask := make([]int64, seq)
	for i := range mask {
		mask[i] = 1
	}
	return ids, mask
}

// wordpieceV1 greedily splits a word into subword pieces using the vocab.
// maxTok bounds the substring scan (no vocab entry is longer), so one huge
// word cannot turn the scan quadratic.
func wordpieceV1(vocab map[string]int64, word string, maxPieces, maxTok int) []int64 {
	if id, ok := vocab[word]; ok {
		return []int64{id}
	}
	candidates := []rune(word)
	idx := 0
	allPieces := make([]int64, 0, len(candidates))
	for idx < len(candidates) {
		best := -1
		hi := len(candidates)
		if maxTok > 0 && idx+maxTok < hi {
			hi = idx + maxTok
		}
		for end := hi; end > idx; end-- {
			sub := strings.ToLower(string(candidates[idx:end]))
			if _, ok := vocab[sub]; ok {
				best = end
				break
			}
		}
		if best == -1 {
			allPieces = append(allPieces, enUNKID)
			idx++
			continue
		}
		sub := strings.ToLower(string(candidates[idx:best]))
		if idx != 0 {
			sub = "##" + sub
		}
		if id, ok := vocab[sub]; ok {
			allPieces = append(allPieces, id)
		} else {
			allPieces = append(allPieces, enUNKID)
		}
		idx = best
	}
	if len(allPieces) > maxPieces {
		allPieces = allPieces[:maxPieces]
	}
	return allPieces
}
