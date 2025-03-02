# Use wasm32-wasi-clang wrapper for WASI compilation
WASM_CC = wasm32-wasi-clang

# We'll pass these flags to allow undefined references (which will be resolved at runtime).
CFLAGS = -Wl,--allow-undefined

# Default target: build both program_a.wasm and program_b.wasm
all: build/program_b.wasm build/program_d.wasm build/program_a.wasm build/program_c.wasm

# Pattern rule: compile any .c file into a .wasm in the build directory.
build/%.wasm: %.c
	mkdir -p build
	$(WASM_CC) $(CFLAGS) -o $@ $<

# Clean rule to remove compiled WASM binaries.
clean:
	rm -rf build/*.wasm