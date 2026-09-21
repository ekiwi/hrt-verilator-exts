#!/bin/sh

set -eu

repository_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$repository_directory"
binary="$repository_directory/target/debug/formal"

# Build the project before running tests
cargo build

for build_file in tests/*/build.ninja; do
    fixture_directory=$(dirname -- "$build_file")
    ast="$fixture_directory/build/ast.json"

    btor_output="$fixture_directory/build/test.btor"
    ron_output="$fixture_directory/build/test.ron"

    # Run verilator with ninja to generate the AST
    rm -rf "$fixture_directory/build"
    ninja -C "$fixture_directory"

    # Generate the BTOR and RON outputs using the built binary
    if [ "$(basename -- "$fixture_directory")" = gecko_core ]; then
        echo "Running $binary $ast --clock clk --ron-output $ron_output"
        "$binary" "$ast" --clock clk --ron-output "$ron_output"
    else
        echo "Running $binary $ast --clock clk --reset '!reset_n' --output $btor_output --ron-output $ron_output"
        "$binary" "$ast" --clock clk --reset '!reset_n' --output "$btor_output" --ron-output "$ron_output"
    fi
done

cargo test --workspace "$@"
