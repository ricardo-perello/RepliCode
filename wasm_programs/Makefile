WASI_SDK_PATH = $(shell brew --prefix wasi-sdk)/share/wasi-sysroot

all: build/hello.wasm

build/hello.wasm: hello.c
	clang --target=wasm32-wasi --sysroot=$(WASI_SDK_PATH) -o build/hello.wasm hello.c