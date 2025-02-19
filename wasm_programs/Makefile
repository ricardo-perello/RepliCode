# Use wasm32-wasi-clang wrapper for WASI compilation
WASM_CC = wasm32-wasi-clang

all: build/hello.wasm

build/hello.wasm: hello.c
	$(WASM_CC) -o build/hello.wasm hello.c

# Clean rule to remove compiled WASM binaries
clean:
	rm -rf build/*.wasm