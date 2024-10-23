package lmb_mod

import (
	"sync"
	"testing"

	"github.com/henry40408/lmb/internal/eval_context/modules/testutil"
	"github.com/henry40408/lmb/internal/lua_convert"
	"github.com/henry40408/lmb/internal/store"
	"github.com/stretchr/testify/assert"
)

func TestState(t *testing.T) {
	L := testutil.NewLuaTestState()
	defer L.Close()

	var state sync.Map
	state.Store("a", int64(1))

	store, err := store.NewStore(":memory:")
	assert.NoError(t, err)
	L.PreloadModule("@lmb", NewLmbModule(&state, store).Loader)

	err = L.DoString(`
  local m = require('@lmb')
  m.state['b'] = m.state['a'] + 1
  return true
  `)
	assert.NoError(t, err)

	assert.Greater(t, L.GetTop(), 0)

	res := lua_convert.FromLuaValue(L.Get(-1))
	assert.Equal(t, true, res)

	a, _ := state.Load("a")
	b, _ := state.Load("b")
	assert.Equal(t, int64(1), a)
	assert.Equal(t, int64(2), b)
}
