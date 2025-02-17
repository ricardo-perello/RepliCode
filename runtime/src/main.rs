use wasmtime::*;
use wasmtime_wasi::sync::WasiCtxBuilder;

fn main() -> anyhow::Result<()> {
    let engine = Engine::default();
    let module = Module::from_file(&engine, "../wasm_programs/build/hello.wasm")?;
    let mut store = Store::new(&engine, WasiCtxBuilder::new().build());

    let instance = Instance::new(&mut store, &module, &[])?;
    let start = instance.get_typed_func::<(), ()>(&mut store, "_start")?;

    start.call(&mut store, ())?;
    Ok(())
}