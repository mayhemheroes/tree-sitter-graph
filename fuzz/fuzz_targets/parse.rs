use honggfuzz::fuzz;
use tree_sitter_graph::parser::Parser;

fn main() {
    loop {
        fuzz!(|data: &[u8]| {
            let source: &str = std::str::from_utf8(data).unwrap();
            let _ = Parser::new(source);
        });
    }
}