use wasmtime::*;
use wasmtime_wasi::sync::WasiCtxBuilder;

fn main() -> anyhow::Result<()> {
    // Create engine, module, and a WASI context
    let engine = Engine::default();
    let module = Module::from_file(&engine, "../../wasm_programs/build/hello.wasm")?;
    let wasi_ctx = WasiCtxBuilder::new().inherit_stdio().build();

    // Create a Store to hold the WASI context
    let mut store = Store::new(&engine, wasi_ctx);

    // Create a Linker and define WASI functions on it
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::sync::add_to_linker(&mut linker, |ctx| ctx)?;

    // Instantiate using the linker instead of passing empty imports
    let instance = linker.instantiate(&mut store, &module)?;

    // Now `_start` should be exported as part of WASI's entry point
    let start = instance
        .get_typed_func::<(), ()>(&mut store, "_start")?;

    // Call the entry point
    start.call(&mut store, ())?;

    Ok(())
}