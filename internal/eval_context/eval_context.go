package eval_context

import (
	"bufio"
	"context"
	"io"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/cespare/xxhash"
	httpMod "github.com/cjoudrey/gluahttp"
	urlMod "github.com/cjoudrey/gluaurl"
	logMod "github.com/cosmotek/loguago"
	"github.com/henry40408/lmb/internal/eval_context/modules/io_mod"
	"github.com/henry40408/lmb/internal/eval_context/modules/lmb_mod"
	"github.com/henry40408/lmb/internal/lua_convert"
	"github.com/henry40408/lmb/internal/store"
	jsonMod "github.com/layeh/gopher-json"
	"github.com/rs/zerolog/log"
	cryptoMod "github.com/tengattack/gluacrypto"
	regexMod "github.com/yuin/gluare"
	lua "github.com/yuin/gopher-lua"
	"github.com/yuin/gopher-lua/ast"
	"github.com/yuin/gopher-lua/parse"
)

type EvalContext struct {
	compiled sync.Map
	input    *bufio.Reader
	store    *store.Store
}

func NewEvalContext(store *store.Store, input io.Reader) *EvalContext {
	return &EvalContext{
		compiled: sync.Map{},
		input:    bufio.NewReader(input),
		store:    store,
	}
}

func NewTestEvalContext(input io.Reader) (*EvalContext, *store.Store) {
	store, err := store.NewStore(":memory:")
	if err != nil {
		log.Fatal().Err(err).Msg("")
	}
	return NewEvalContext(store, input), store
}

func (e *EvalContext) initState(ctx context.Context, state *sync.Map, w io.Writer) *lua.LState {
	L := lua.NewState(lua.Options{SkipOpenLibs: true})
	L.SetContext(ctx)
	for _, pair := range []struct {
		n string
		f lua.LGFunction
	}{
		{lua.LoadLibName, lua.OpenPackage},
		{lua.BaseLibName, lua.OpenBase},
		{lua.MathLibName, lua.OpenMath},
		{lua.StringLibName, lua.OpenString},
		{lua.TabLibName, lua.OpenTable},
	} {
		if err := L.CallByParam(lua.P{
			Fn:      L.NewFunction(pair.f),
			NRet:    0,
			Protect: true,
		}, lua.LString(pair.n)); err != nil {
			panic(err)
		}
	}

	cryptoMod.Preload(L)

	L.PreloadModule("http", httpMod.NewHttpModule(&http.Client{}).Loader)
	L.PreloadModule("json", jsonMod.Loader)

	logger := logMod.NewLogger(log.Logger)
	L.PreloadModule("logger", logger.Loader)

	L.PreloadModule("re", regexMod.Loader)
	L.PreloadModule("url", urlMod.Loader)

	L.PreloadModule("io", io_mod.NewIoMod(e.input, w).Loader)
	L.PreloadModule("@lmb", lmb_mod.NewLmbModule(state, e.store).Loader)
	return L
}

func (e *EvalContext) Compile(reader io.Reader, name string) (*lua.FunctionProto, error) {
	start := time.Now()

	parsed, err := e.Parse(reader, name)
	if err != nil {
		return nil, err
	}
	compiled, err := lua.Compile(parsed, name)
	if err != nil {
		return nil, err
	}

	duration := time.Since(start)
	log.Trace().Str("name", name).Str("duration", duration.String()).Msg("compiled")

	return compiled, nil
}

func (e *EvalContext) Eval(ctx context.Context, compiled *lua.FunctionProto, state *sync.Map, writer io.Writer) (interface{}, error) {
	L := e.initState(ctx, state, writer)
	defer L.Close()

	lf := L.NewFunctionFromProto(compiled)
	L.Push(lf)
	if err := L.PCall(0, lua.MultRet, nil); err != nil {
		return nil, err
	}

	if L.GetTop() > 0 {
		result := L.Get(-1)
		L.Pop(1)
		return lua_convert.FromLuaValue(result), nil
	}

	return nil, nil
}

func (e *EvalContext) findOrCompile(reader io.ReadSeeker) (*lua.FunctionProto, error) {
	hasher := xxhash.New()
	if _, err := io.Copy(hasher, reader); err != nil {
		return nil, err
	}

	id := hasher.Sum64()
	if found, ok := e.compiled.Load(id); ok {
		return found.(*lua.FunctionProto), nil
	}

	reader.Seek(0, io.SeekStart)
	compiled, err := e.Compile(reader, strconv.FormatUint(id, 10))
	if err != nil {
		return nil, err
	}

	found, _ := e.compiled.LoadOrStore(id, compiled)
	actual := found.(*lua.FunctionProto)
	return actual, nil
}

func (e *EvalContext) EvalReader(ctx context.Context, reader io.ReadSeeker, state *sync.Map, writer io.Writer) (interface{}, error) {
	compiled, err := e.findOrCompile(reader)
	if err != nil {
		return nil, err
	}
	return e.Eval(ctx, compiled, state, writer)
}

func (e *EvalContext) EvalScript(ctx context.Context, script string, state *sync.Map, writer io.Writer) (interface{}, error) {
	L := e.initState(ctx, state, writer)
	defer L.Close()

	compiled, err := e.findOrCompile(strings.NewReader(script))
	if err != nil {
		return nil, err
	}

	return e.Eval(ctx, compiled, state, writer)
}

func (e *EvalContext) Parse(reader io.Reader, name string) ([]ast.Stmt, error) {
	return parse.Parse(reader, name)
}
