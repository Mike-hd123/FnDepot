// Package bgeemb implements a self-contained BGE embedder in Go.
//
// It loads an ONNX model via onnxruntime_go (cgo -> onnxruntime shared
// library), tokenizes text (WordPiece), runs inference, mean-pools with the
// attention mask, and L2-normalizes. Two model layouts are supported:
//
//   - zh (default): bge-large-zh (1024d, Chinese) driven by a HF tokenizer.json.
//   - en (legacy):  bge-small-en-v1.5 (384d, English) driven by a BERT vocab.txt.
//
// The runtime model directory is selected by the model name in use
// (HYATLAS_EMBED_MODEL / modelDirLayout). Kept on-disk so the fpkg ships the
// .onnx + tokenizer alongside the (dynamic-glibc) binary.
//
// Memory: the session is created with EnableCpuMemArena(true) +
// EnableMemoryPattern(true); combined with a single-weight mmap this keeps
// resident memory near the llama.cpp mmap level instead of a raw huge RSS.

package bgeemb

import (
	"context"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"

	ort "github.com/yalue/onnxruntime_go"
)

// Model info: the upstream v4 branch halves this to a single 1024d Chinese
// layout; here we keep both the legacy 384d and the new 1024d selectable.
const (
	enHidden = 384
	zhHidden = 1024
	zhMaxSeq = 512 // bge-large-zh f16/int8 supports up to 512 tokens
	enMaxSeq = 128
)

// BGE is a thread-safe BGE embedder. The session is created from a file so the
// ONNX external .data weights are loaded relative to the model file's dir.
type BGE struct {
	sess   *ort.DynamicAdvancedSession
	tok    *Tokenizer // zh tokenizer (nil for en layout)
	vocab  map[string]int64
	hidden int
	maxSeq int
	mu     sync.Mutex
}

// Embed is the package-level helper used by the embed path.
func (b *BGE) Embed(ctx context.Context, text string) ([]float32, error) {
	return b.EmbedOne(text)
}

// HiddenDim returns the model embed dimension (384 en / 1024 zh).
func (b *BGE) HiddenDim() int { return b.hidden }

// EmbedOne returns a normalized vector for a single string (mean pooling + L2).
func (b *BGE) EmbedOne(text string) ([]float32, error) {
	var ids, mask []int64
	if b.tok != nil {
		ids, mask = b.tok.Encode(text, b.maxSeq)
	} else {
		ids, mask = tokenizeV1(b.vocab, text, b.maxSeq)
	}
	if len(ids) == 0 {
		ids = []int64{-1}
		mask = []int64{0}
	}
	seq := len(ids)
	shape := ort.NewShape(1, int64(seq))
	inT, err := ort.NewTensor(shape, ids)
	if err != nil {
		return nil, err
	}
	maskT, err := ort.NewTensor(shape, mask)
	if err != nil {
		return nil, err
	}
	tt := make([]int64, seq) // token_type_ids, all 0 (single segment)
	ttT, err := ort.NewTensor(shape, tt)
	if err != nil {
		return nil, err
	}
	// output: [1, seq, hidden]
	flat := make([]float32, 1*seq*b.hidden)
	outT, err := ort.NewTensor(ort.NewShape(1, int64(seq), int64(b.hidden)), flat)
	if err != nil {
		return nil, err
	}

	b.mu.Lock()
	runErr := b.sess.Run([]ort.Value{inT, maskT, ttT}, []ort.Value{outT})
	b.mu.Unlock()
	if runErr != nil {
		return nil, runErr
	}

	_ = inT.Destroy()
	_ = maskT.Destroy()
	_ = ttT.Destroy()
	out := outT.GetData()
	// bge-large-zh uses the [CLS] token as the sentence embedding (HF model
	// outputs pooler_output derived from CLS; llama.cpp --pooling cls matches).
	h := b.hidden
	res := make([]float32, h)
	copy(res, out[:h])
	// L2 normalize
	var norm float64
	for _, x := range res {
		norm += float64(x) * float64(x)
	}
	if norm > 0 {
		r := float32(1 / math.Sqrt(norm))
		for j := range res {
			res[j] *= r
		}
	}
	_ = outT.Destroy()
	return res, nil
}

// EmbedBatch embeds multiple strings sequentially.
func (b *BGE) EmbedBatch(ctx context.Context, texts []string) ([][]float32, error) {
	out := make([][]float32, len(texts))
	for i, t := range texts {
		v, err := b.EmbedOne(t)
		if err != nil {
			return nil, err
		}
		out[i] = v
	}
	return out, nil
}

// Destroy frees the onnxruntime session + environment.
func (b *BGE) Destroy() {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.sess != nil {
		_ = b.sess.Destroy()
		b.sess = nil
	}
	_ = ort.DestroyEnvironment()
}

// meanPool averages the hidden states over the valid (mask==1) tokens and L2-normalizes.
func meanPool(out []float32, mask []int64, seq, hidden int) []float32 {
	vec := make([]float64, hidden)
	var count int
	for i := 0; i < seq; i++ {
		if mask[i] == 0 {
			continue
		}
		count++
		row := out[i*hidden : (i+1)*hidden]
		for j := 0; j < hidden; j++ {
			vec[j] += float64(row[j])
		}
	}
	if count == 0 {
		count = 1
	}
	res := make([]float32, hidden)
	var norm float64
	for j := 0; j < hidden; j++ {
		res[j] = float32(vec[j] / float64(count))
		norm += float64(res[j]) * float64(res[j])
	}
	if norm > 0 {
		r := float32(1 / math.Sqrt(norm))
		for j := range res {
			res[j] *= r
		}
	}
	return res
}

// runtimeLibName returns the platform-appropriate onnxruntime shared library name.
func runtimeLibName() string {
	switch runtime.GOOS {
	case "windows":
		return "onnxruntime.dll"
	case "darwin":
		return "libonnxruntime.dylib"
	default:
		return "libonnxruntime.so"
	}
}

func findLibFallback(baseDir string) (string, error) {
	entries, err := os.ReadDir(baseDir)
	if err != nil {
		return "", err
	}
	for _, e := range entries {
		if strings.HasPrefix(e.Name(), "onnxruntime") || strings.HasPrefix(e.Name(), "libonnxruntime") {
			return filepath.Join(baseDir, e.Name()), nil
		}
	}
	return "", fmt.Errorf("no onnxruntime* file found in %s", baseDir)
}

// sessionOptions builds the CPU session options with memory optimization on.
func sessionOptions() (*ort.SessionOptions, error) {
	o, err := ort.NewSessionOptions()
	if err != nil {
		return nil, err
	}
	// Memory optimization: enable both CPU arena + memory pattern. Together with
	// the .data-based weights (loaded via CreateSessionFromFile) this keeps
	// resident memory near the llama.cpp mmap level instead of a raw huge RSS.
	_ = o.SetCpuMemArena(true)
	_ = o.SetMemPattern(true)
	// int8 graph is cheap; cap intra-op threads to the small core count.
	_ = o.SetIntraOpNumThreads(4)
	return o, nil
}

// newSession creates a DynamicAdvancedSession from the model file (no ONNX
// bytes buffered in Go), enabling the memory optimizations above.
func newSession(modelPath string) (*ort.DynamicAdvancedSession, error) {
	opts, err := sessionOptions()
	if err != nil {
		return nil, err
	}
	defer func() { _ = opts.Destroy() }()
	// CreateSessionFromFile so ORT mmaps / reads the external .data weights
	// relative to the model file (no manual chdir needed, unlike NewDynamicSession).
	s, err := ort.NewDynamicAdvancedSession(modelPath,
		[]string{"input_ids", "attention_mask", "token_type_ids"},
		[]string{"last_hidden_state"}, opts)
	if err != nil {
		return nil, err
	}
	return s, nil
}

// New loads the layout for the given model name/dir. modelName drives the
// model file + tokenizer layout; "" defaults to the single-dir zh layout.
func New(baseDir, modelName string) (*BGE, error) {
	modelName = strings.TrimSpace(modelName)
	if modelName == "" {
		modelName = "bge-large-zh"
	}
	modelDir := baseDir
	if sub := modelLayerDir(modelName); sub != "" {
		// allow a shared baseDir with per-model subdirs: models/<sub>/...
		modelDir = filepath.Join(baseDir, sub)
	}

	lib := runtimeLibName()
	libPath := filepath.Join(modelDir, lib)
	if _, err := os.Stat(libPath); err != nil {
		if fb, ferr := findLibFallback(modelDir); ferr == nil {
			libPath = fb
		} else {
			return nil, fmt.Errorf("onnxruntime library %s not found in %s (and no fallback present): %w",
				lib, modelDir, err)
		}
	}

	ort.SetSharedLibraryPath(libPath)
	if err := ort.InitializeEnvironment(); err != nil {
		return nil, fmt.Errorf("ort init: %w", err)
	}

	modelFile, modelPrefix, hidden, maxSeq, err := resolveModelLayout(modelName)
	if err != nil {
		_ = ort.DestroyEnvironment()
		return nil, err
	}
	modelPath := filepath.Join(modelDir, modelFile)
	sess, err := newSession(modelPath)
	if err != nil {
		_ = ort.DestroyEnvironment()
		return nil, fmt.Errorf("new session (%s): %w", modelPath, err)
	}

	b := &BGE{sess: sess, hidden: hidden, maxSeq: maxSeq}

	// tokenizer per layout
	tkPath := filepath.Join(modelDir, "tokenizer.json")
	if _, err := os.Stat(tkPath); err == nil {
		t, terr := LoadTokenizer(tkPath)
		if terr != nil {
			sess.Destroy()
			_ = ort.DestroyEnvironment()
			return nil, fmt.Errorf("load %s tokenizer: %w", modelName, terr)
		}
		b.tok = t
	} else if modelPrefix == "bge-small-en" {
		vocabPath := filepath.Join(modelDir, "vocab.txt")
		v, verr := loadVocab(vocabPath)
		if verr != nil {
			sess.Destroy()
			_ = ort.DestroyEnvironment()
			return nil, verr
		}
		b.vocab = v
	} else {
		sess.Destroy()
		_ = ort.DestroyEnvironment()
		return nil, fmt.Errorf("no tokenizer.json found for %s in %s", modelName, modelDir)
	}
	return b, nil
}

// resolveModelLayout maps a model name to (model file, id-prefix, hidden dim, max seq).
func resolveModelLayout(name string) (file, prefix string, hidden, maxSeq int, err error) {
	lower := strings.ToLower(name)
	switch {
	case strings.Contains(lower, "large-zh"), strings.HasPrefix(lower, "bge-large-zh"):
		return "model_int8.onnx", "bge-large-zh", zhHidden, zhMaxSeq, nil
	case strings.Contains(lower, "small-en"), strings.HasPrefix(lower, "bge-small-en"):
		return "bge-small-en-v1.5.onnx", "bge-small-en", enHidden, enMaxSeq, nil
	default:
		return "", "", 0, 0, fmt.Errorf("unsupported model %q (want bge-large-zh or bge-small-en)", name)
	}
}

func lookupModelFileByPrefix(modelName string, candidates []string) string {
	// The Chinese onnx file must live next to the model: try exact first, then
	// the int8 name used by the bench pipeline. Caller ensures the dir exists.
	for _, c := range candidates {
		f := c + ".onnx"
		if _, err := os.Stat(filepath.Join(".", ".", f)); err == nil {
			// don't stat-require here; New() resolves against modelDir
			return f
		}
	}
	return "model_int8.onnx"
}

func modelLayerDir(name string) string {
	lower := strings.ToLower(name)
	// zh layout ships flat (model_int8.onnx + tokenizer.json + lib at baseDir);
	// only the legacy en model uses a subdirectory.
	switch {
	case strings.Contains(lower, "small-en"):
		return "bge-small-en"
	default:
		return ""
	}
}