package main

import (
	"bytes"
	"context"
	"os"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"testing"

	"github.com/henry40408/lmb/internal/eval_context"
	"github.com/henry40408/lmb/internal/store"
	"github.com/stretchr/testify/assert"
	"github.com/yuin/goldmark"
	"github.com/yuin/goldmark/ast"
	"github.com/yuin/goldmark/text"
)

var (
	IN_PATTERN  = regexp.MustCompile(`--\s*input:\s*(.+)`)
	OUT_PATTERN = regexp.MustCompile(`--\s*output:\s*(.+)`)
)

func TestGuide(t *testing.T) {
	blocks, err := extractCodeBlocks("guides/lua.md")
	assert.NoError(t, err)

	store, err := store.NewStore(":memory:")
	assert.NoError(t, err)

	for _, block := range blocks {
		var w bytes.Buffer
		var state sync.Map

		inMatches := IN_PATTERN.FindStringSubmatch(block)
		input := ""
		if len(inMatches) > 1 {
			// 1. Replace '\\n' with '\n' to simulate multi-line input with one line.
			// 2. Remove extra '\n' on Windows
			input = strings.ReplaceAll(strings.ReplaceAll(inMatches[1], "\\n", "\n"), "\r", "")
		}
		e := eval_context.NewEvalContext(store, strings.NewReader(input))

		c, err := e.Compile(strings.NewReader(block), "")
		assert.NoError(t, err)

		assert.NoError(t, err)
		res, err := e.Eval(context.Background(), c, &state, &w)
		assert.NoError(t, err)

		if w.Len() > 0 {
			res = w.String()
		}

		outMatches := OUT_PATTERN.FindStringSubmatch(block)
		if len(outMatches) > 1 {
			// 1. Replace '\\n' with '\n' to simulate multi-line input with one line.
			// 2. Remove extra '\n' on Windows
			expected := strings.ReplaceAll(strings.ReplaceAll(outMatches[1], "\\n", "\n"), "\r", "")
			if n, err := strconv.ParseFloat(expected, 64); err == nil {
				if float64(n) == float64(int64(n)) {
					assert.Equal(t, int64(n), res)
				} else {
					assert.Equal(t, n, res)
				}
			} else {
				assert.Equal(t, expected, res)
			}
		} else {
			assert.Nil(t, res)
		}
	}
}

func extractCodeBlocks(filename string) ([]string, error) {
	content, err := os.ReadFile(filename)
	if err != nil {
		return nil, err
	}

	doc := goldmark.DefaultParser().Parse(text.NewReader(content))
	var codeBlocks []string

	ast.Walk(doc, func(n ast.Node, entering bool) (ast.WalkStatus, error) {
		if entering && n.Kind() == ast.KindFencedCodeBlock {
			cb := n.(*ast.FencedCodeBlock)
			if string(cb.Language(content)) == "lua" {
				var buf bytes.Buffer
				for i := 0; i < cb.Lines().Len(); i++ {
					line := cb.Lines().At(i)
					buf.Write(line.Value(content))
				}
				codeBlocks = append(codeBlocks, buf.String())
			}
		}
		return ast.WalkContinue, nil
	})

	return codeBlocks, nil
}
