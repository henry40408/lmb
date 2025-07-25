use bon::bon;
use mlua::prelude::*;
use thiserror::Error;

#[derive(Debug, Error)]
enum LmbError {
    #[error("Lua error: {0}")]
    LuaError(#[from] mlua::Error),
    #[error("Expected a Lua function, but got {actual} instead")]
    FromLuaConversionError { actual: Box<str> },
}

type LmbResult<T> = Result<T, LmbError>;

#[derive(Debug)]
struct Runner {
    func: LuaFunction,
    source: Box<str>,
    vm: Lua,
}

#[bon]
impl Runner {
    #[builder]
    pub fn new<S: AsRef<str>>(source: S) -> LmbResult<Self> {
        let source = source.as_ref();

        let vm = Lua::new();
        vm.sandbox(true)?;

        let func: LuaValue = vm.load(source).eval()?;
        let LuaValue::Function(func) = func else {
            return Err(LmbError::FromLuaConversionError {
                actual: func.type_name().into(),
            });
        };

        Ok(Self {
            func,
            vm, // otherwise the Lua VM would be destroyed
            source: source.into(),
        })
    }

    pub fn call(&self) -> LmbResult<()> {
        Ok(self.func.call::<()>(())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_call() {
        let source = include_str!("fixtures/hello.lua");
        let runner = Runner::builder().source(&source).build().unwrap();
        runner.call().unwrap();
    }
}
