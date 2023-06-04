# ARGS
ARG project=tree-sitter-graph

# Setup Fuzz-Rust Builder
FROM ghcr.io/nottirb/fuzz-rust:latest as builder
ADD . /${project}
WORKDIR /${project}
RUN sed -i "s/fn new(source:/pub fn new(source:/" ./src/parser.rs \
	&& sed -i "s/fn parse_into_file/pub fn parse_into_file/" ./src/parser.rs \
	&& sed -i "s/struct Parser/pub struct Parser/" ./src/parser.rs \
	&& sed -i "s/mod parser;/pub mod parser;/" ./src/lib.rs \
	&& cd ./fuzz && cargo +nightly hfuzz build

# Setup Fuzz-Env
FROM ubuntu:20.04
COPY --from=builder ${project}/fuzz/hfuzz_target/x86_64-unknown-linux-gnu/release/parse /
